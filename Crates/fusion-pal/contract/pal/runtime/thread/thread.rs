//! Backend-neutral thread vocabulary and low-level fusion-pal contracts.
//!
//! Threading is the first fusion-pal surface where truthful capability reporting depends on more
//! than operating-system mechanics alone. Effective guarantees may need agreement between:
//! - operating-system scheduling and affinity mechanisms
//! - ISA and microarchitectural behavior
//! - discovered machine topology such as sockets, clusters, and NUMA domains
//! - firmware or hypervisor mediation when they sit in the control path
//!
//! The contract here therefore models both requested policy and effective guarantee
//! strength. Backends are expected to report only the greatest lower bound they can justify
//! across the relevant authorities for each capability, rather than claiming that an OS API
//! name automatically implies a hardware reality.
//!
//! Placement requests intentionally consume topology identifiers rather than producing them.
//! A sibling hardware authority such as the selected PAL surface or the backend-neutral
//! [`crate::contract::pal`] contract is expected to enumerate valid logical CPUs, packages,
//! NUMA nodes, and heterogeneous core classes before callers construct thread placement requests.

mod caps;
mod config;
mod error;
mod id;
mod placement;
mod scheduler;
mod stack;
mod unsupported;

pub use crate::contract::pal::HardwareTopologyNodeId;
pub use caps::*;
pub use config::*;
pub use error::*;
pub use id::*;
pub use placement::*;
pub use scheduler::*;
pub use stack::*;
pub use unsupported::*;

use core::time::Duration;

/// Raw thread entry point type used by fusion-pal backends.
///
/// # Safety
///
/// The backend invokes this entry with the exact `context` pointer supplied to `spawn`.
/// The backend is responsible for whatever trampoline is required to bridge between the
/// platform's native thread-entry ABI and this fusion-pal-level signature. The function must
/// uphold whatever invariants are attached to that pointer and must not unwind across the
/// backend thread entry boundary.
pub type RawThreadEntry = unsafe fn(*mut ()) -> ThreadEntryReturn;

/// Opaque thread return code surfaced by a fusion-pal join operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadExitCode(pub usize);

/// Normal return record produced directly by a thread entry function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadEntryReturn {
    /// Thread-defined normal exit code.
    pub code: ThreadExitCode,
}

impl ThreadEntryReturn {
    /// Returns a normal thread-entry return record with an exit code.
    #[must_use]
    pub const fn new(code: usize) -> Self {
        Self {
            code: ThreadExitCode(code),
        }
    }
}

/// Coarse thread termination classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadTerminationKind {
    /// The thread returned normally from its entry point.
    Returned,
    /// The thread was canceled or externally terminated by the operating environment.
    Canceled,
    /// The thread aborted or terminated for an unspecified fatal reason.
    Aborted,
    /// The thread terminated because of a signal, trap, or analogous asynchronous fault.
    Signaled,
    /// The backend can report termination but cannot classify it more precisely.
    Unknown,
}

/// Termination record returned when joining a thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadTermination {
    /// Coarse classification of how the thread terminated.
    pub kind: ThreadTerminationKind,
    /// Optional thread-defined exit code when the backend can surface one.
    pub code: Option<ThreadExitCode>,
}

impl ThreadTermination {
    /// Returns a normal-return termination record with an exit code.
    #[must_use]
    pub const fn returned(code: usize) -> Self {
        Self {
            kind: ThreadTerminationKind::Returned,
            code: Some(ThreadExitCode(code)),
        }
    }

    /// Returns a normal-return termination record synthesized from a thread entry return.
    #[must_use]
    pub const fn from_entry_return(entry: ThreadEntryReturn) -> Self {
        Self {
            kind: ThreadTerminationKind::Returned,
            code: Some(entry.code),
        }
    }
}

/// Coarse execution state that may be observable for a thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadRunState {
    /// The thread exists but has not started executing user entry yet.
    Starting,
    /// The thread is runnable but not currently executing.
    Runnable,
    /// The thread is actively executing on a processor.
    Running,
    /// The thread is blocked waiting for an event, primitive, or scheduler resource.
    Blocked,
    /// The thread has exited and may be joinable for result collection.
    Exited,
    /// The backend cannot classify the current run state precisely.
    Unknown,
}

/// Snapshot of observable thread state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadObservation {
    /// Stable thread identifier when one is known.
    pub id: ThreadId,
    /// Coarse run-state classification.
    pub run_state: ThreadRunState,
    /// Current execution location when observable.
    pub location: ThreadExecutionLocation,
    /// Effective scheduler policy when observable.
    pub scheduler: ThreadSchedulerObservation,
    /// Effective placement outcome when observable.
    pub placement: ThreadPlacementOutcome,
}

/// Base thread support surface for a selected fusion-pal backend.
pub trait ThreadBase {
    /// Backend-defined owned thread handle type.
    type Handle: Send + Sync;

    /// Reports the thread support and effective guarantee surface.
    fn support(&self) -> ThreadSupport;
}

/// Thread lifecycle operations implemented by a selected fusion-pal backend.
///
/// # Safety
///
/// `spawn` accepts a raw entry function and opaque context pointer. The caller must ensure
/// that the pointer remains valid for the thread entry and that the entry function does not
/// unwind across the backend thread boundary.
pub unsafe trait ThreadLifecycle: ThreadBase {
    /// Spawns a new thread using the provided configuration and entry point.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `context` remains valid for the lifetime requirements of
    /// `entry`, that `entry` upholds those context invariants, and that `entry` does not
    /// unwind across the backend thread boundary. The backend is expected to provide any
    /// platform-specific trampoline needed to connect the native thread-entry ABI to
    /// `RawThreadEntry`.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot create the thread with the requested
    /// lifecycle, placement, scheduler, or stack contract honestly.
    unsafe fn spawn(
        &self,
        config: &ThreadConfig<'_>,
        entry: RawThreadEntry,
        context: *mut (),
    ) -> Result<Self::Handle, ThreadError>;

    /// Returns the identifier of the current thread.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot surface a stable current-thread identity.
    fn current_thread_id(&self) -> Result<ThreadId, ThreadError>;

    /// Joins a joinable thread and returns its termination record.
    ///
    /// The returned termination record may either be synthesized directly from the thread
    /// entry's normal return value or produced by the backend when the thread is canceled,
    /// aborted, signaled, or otherwise terminates outside the normal entry-return path.
    ///
    /// # Errors
    ///
    /// Returns an error if the thread is detached, not joinable, or the backend cannot
    /// complete the join honestly.
    fn join(&self, handle: Self::Handle) -> Result<ThreadTermination, ThreadError>;

    /// Detaches a thread handle so its resources are released without a future join.
    ///
    /// # Errors
    ///
    /// Returns an error if the handle is not detachable or the backend cannot perform the
    /// detach honestly.
    fn detach(&self, handle: Self::Handle) -> Result<(), ThreadError>;
}

/// External thread suspension control for a selected fusion-pal backend.
pub trait ThreadSuspendControl: ThreadBase {
    /// Suspends a thread handle.
    ///
    /// Suspension is intentionally not part of the baseline lifecycle contract because it
    /// is unavailable or ill-advised on many hosted systems, but it is a real workflow on
    /// several RTOS targets.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend does not support suspension for the handle or
    /// cannot perform the suspend honestly.
    fn suspend(&self, handle: &Self::Handle) -> Result<(), ThreadError>;

    /// Resumes a previously suspended thread handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend does not support resume for the handle or cannot
    /// perform the resume honestly.
    fn resume(&self, handle: &Self::Handle) -> Result<(), ThreadError>;
}

/// Scheduler control and voluntary execution operations for a selected fusion-pal backend.
pub trait ThreadSchedulerControl: ThreadBase {
    /// Returns the valid numeric priority range for a scheduler class, when one exists.
    ///
    /// Some scheduler classes either do not use numeric priorities at all or use backend-
    /// specific semantics that cannot be described honestly as a simple inclusive range.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot observe or justify the valid range for the
    /// requested class honestly.
    fn priority_range(
        &self,
        class: ThreadSchedulerClass,
    ) -> Result<Option<ThreadPriorityRange>, ThreadError>;

    /// Applies scheduler policy to a thread handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot apply or honestly degrade the requested
    /// scheduler policy.
    fn set_scheduler(
        &self,
        handle: &Self::Handle,
        request: &ThreadSchedulerRequest,
    ) -> Result<ThreadSchedulerObservation, ThreadError>;

    /// Queries the effective scheduler policy for a thread handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot observe the effective scheduler state.
    fn scheduler(&self, handle: &Self::Handle) -> Result<ThreadSchedulerObservation, ThreadError>;

    /// Yields the current thread to the scheduler.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot honestly perform a scheduler yield.
    fn yield_now(&self) -> Result<(), ThreadError>;

    /// Sleeps the current thread for a relative duration.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot honestly perform the relative sleep.
    fn sleep_for(&self, duration: Duration) -> Result<(), ThreadError>;

    /// Returns the current monotonic time observed by the backend scheduler surface.
    ///
    /// The returned duration is measured against a backend-defined monotonic origin. Callers may
    /// compare values from the same running system for elapsed-time calculations, but must not
    /// assign portable wall-clock meaning to the absolute origin.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot surface a truthful monotonic timestamp.
    fn monotonic_now(&self) -> Result<Duration, ThreadError>;
}

/// Placement and locality control for a selected fusion-pal backend.
pub trait ThreadPlacementControl: ThreadBase {
    /// Applies placement policy to a thread handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot apply or honestly degrade the requested
    /// placement policy.
    fn set_placement(
        &self,
        handle: &Self::Handle,
        request: &ThreadPlacementRequest<'_>,
    ) -> Result<ThreadPlacementOutcome, ThreadError>;

    /// Queries the effective placement outcome for a thread handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot observe the effective placement state.
    fn placement(&self, handle: &Self::Handle) -> Result<ThreadPlacementOutcome, ThreadError>;
}

/// Thread observation operations for a selected fusion-pal backend.
pub trait ThreadObservationControl: ThreadBase {
    /// Observes the current thread.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot produce a current-thread observation honestly.
    fn observe_current(&self) -> Result<ThreadObservation, ThreadError>;

    /// Observes a specific thread handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot observe the requested thread honestly.
    fn observe(&self, handle: &Self::Handle) -> Result<ThreadObservation, ThreadError>;
}

/// Stack-usage observation for a selected fusion-pal backend.
pub trait ThreadStackObservationControl: ThreadBase {
    /// Observes stack usage for the current thread.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot honestly observe current-thread stack usage
    /// or high-water-mark information.
    fn observe_current_stack(&self) -> Result<ThreadStackObservation, ThreadError>;

    /// Observes stack usage for a specific thread handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot honestly observe the requested thread's
    /// stack usage or high-water-mark information.
    fn observe_stack(&self, handle: &Self::Handle) -> Result<ThreadStackObservation, ThreadError>;
}

#[cfg(test)]
mod tests {
    use super::{ThreadEntryReturn, ThreadTermination, ThreadTerminationKind};

    #[test]
    fn termination_from_entry_return_stays_normal() {
        let termination = ThreadTermination::from_entry_return(ThreadEntryReturn::new(7));

        assert_eq!(termination.kind, ThreadTerminationKind::Returned);
        assert_eq!(termination.code, ThreadTermination::returned(7).code);
    }
}
