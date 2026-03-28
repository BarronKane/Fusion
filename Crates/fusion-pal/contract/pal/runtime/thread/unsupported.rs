//! Backend-neutral unsupported thread implementations.
//!
//! These types intentionally realize the fusion-pal thread contracts in an always-unsupported
//! form. Hosted stubs and future unsupported targets can reuse them without inventing
//! platform-specific folklore just to satisfy trait bounds.

use core::time::Duration;

use super::{
    RawThreadEntry,
    ThreadBase,
    ThreadConfig,
    ThreadError,
    ThreadId,
    ThreadLifecycle,
    ThreadObservation,
    ThreadObservationControl,
    ThreadPlacementControl,
    ThreadPlacementOutcome,
    ThreadPlacementRequest,
    ThreadPriorityRange,
    ThreadSchedulerClass,
    ThreadSchedulerControl,
    ThreadSchedulerObservation,
    ThreadSchedulerRequest,
    ThreadStackObservation,
    ThreadStackObservationControl,
    ThreadSupport,
    ThreadSuspendControl,
    ThreadTermination,
};

/// Unsupported thread provider placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedThread;

/// Unsupported owned thread handle placeholder.
#[derive(Debug, Default, PartialEq, Eq, Hash)]
pub struct UnsupportedThreadHandle;

impl UnsupportedThread {
    /// Creates a new unsupported thread provider placeholder.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ThreadBase for UnsupportedThread {
    type Handle = UnsupportedThreadHandle;

    fn support(&self) -> ThreadSupport {
        ThreadSupport::unsupported()
    }
}

// SAFETY: this backend never successfully spawns, joins, or detaches threads and therefore
// cannot violate lifecycle invariants through its unsupported surface.
unsafe impl ThreadLifecycle for UnsupportedThread {
    unsafe fn spawn(
        &self,
        _config: &ThreadConfig<'_>,
        _entry: RawThreadEntry,
        _context: *mut (),
    ) -> Result<Self::Handle, ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn current_thread_id(&self) -> Result<ThreadId, ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn join(&self, _handle: Self::Handle) -> Result<ThreadTermination, ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn detach(&self, _handle: Self::Handle) -> Result<(), ThreadError> {
        Err(ThreadError::unsupported())
    }
}

impl ThreadSuspendControl for UnsupportedThread {
    fn suspend(&self, _handle: &Self::Handle) -> Result<(), ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn resume(&self, _handle: &Self::Handle) -> Result<(), ThreadError> {
        Err(ThreadError::unsupported())
    }
}

impl ThreadSchedulerControl for UnsupportedThread {
    fn priority_range(
        &self,
        _class: ThreadSchedulerClass,
    ) -> Result<Option<ThreadPriorityRange>, ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn set_scheduler(
        &self,
        _handle: &Self::Handle,
        _request: &ThreadSchedulerRequest,
    ) -> Result<ThreadSchedulerObservation, ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn scheduler(&self, _handle: &Self::Handle) -> Result<ThreadSchedulerObservation, ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn yield_now(&self) -> Result<(), ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn sleep_for(&self, _duration: Duration) -> Result<(), ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn monotonic_now(&self) -> Result<Duration, ThreadError> {
        Err(ThreadError::unsupported())
    }
}

impl ThreadPlacementControl for UnsupportedThread {
    fn set_placement(
        &self,
        _handle: &Self::Handle,
        _request: &ThreadPlacementRequest<'_>,
    ) -> Result<ThreadPlacementOutcome, ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn placement(&self, _handle: &Self::Handle) -> Result<ThreadPlacementOutcome, ThreadError> {
        Err(ThreadError::unsupported())
    }
}

impl ThreadObservationControl for UnsupportedThread {
    fn observe_current(&self) -> Result<ThreadObservation, ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn observe(&self, _handle: &Self::Handle) -> Result<ThreadObservation, ThreadError> {
        Err(ThreadError::unsupported())
    }
}

impl ThreadStackObservationControl for UnsupportedThread {
    fn observe_current_stack(&self) -> Result<ThreadStackObservation, ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn observe_stack(&self, _handle: &Self::Handle) -> Result<ThreadStackObservation, ThreadError> {
        Err(ThreadError::unsupported())
    }
}
