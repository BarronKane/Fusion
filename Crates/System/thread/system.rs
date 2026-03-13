//! System thread provider facade.

use core::time::Duration;

use fusion_pal::sys::thread::{PlatformThread, system_thread as pal_system_thread};

use super::{
    RawThreadEntry, ThreadConfig, ThreadError, ThreadId, ThreadObservation, ThreadPlacementOutcome,
    ThreadPlacementRequest, ThreadPriorityRange, ThreadSchedulerClass, ThreadSchedulerObservation,
    ThreadSchedulerRequest, ThreadStackObservation, ThreadSupport, ThreadTermination,
};
use crate::thread::handle::ThreadHandle;

/// System thread provider wrapper around the selected PAL backend.
#[derive(Debug, Clone, Copy)]
pub struct ThreadSystem {
    inner: PlatformThread,
}

impl ThreadSystem {
    /// Creates a wrapper for the selected platform thread provider.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: pal_system_thread(),
        }
    }

    /// Reports the supported thread surface.
    #[must_use]
    pub fn support(&self) -> ThreadSupport {
        fusion_pal::sys::thread::ThreadBase::support(&self.inner)
    }

    /// Spawns a thread using the raw PAL-level entry signature.
    ///
    /// # Safety
    ///
    /// The caller must ensure the raw entry and opaque context uphold the PAL thread
    /// contract for the selected backend.
    ///
    /// # Errors
    ///
    /// Returns any honest backend thread-creation failure, including lifecycle, scheduler,
    /// placement, or stack-policy rejection.
    pub unsafe fn spawn_raw(
        &self,
        config: &ThreadConfig<'_>,
        entry: RawThreadEntry,
        context: *mut (),
    ) -> Result<ThreadHandle, ThreadError> {
        // SAFETY: the caller upholds the raw PAL spawn contract.
        let handle = unsafe {
            fusion_pal::sys::thread::ThreadLifecycle::spawn(&self.inner, config, entry, context)?
        };
        Ok(ThreadHandle::new(handle))
    }

    /// Returns the identifier of the current thread.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot surface a stable current-thread identifier.
    pub fn current_thread_id(&self) -> Result<ThreadId, ThreadError> {
        fusion_pal::sys::thread::ThreadLifecycle::current_thread_id(&self.inner)
    }

    /// Joins a joinable thread and returns its termination record.
    ///
    /// # Errors
    ///
    /// Returns an error if the handle is detached, invalid, or the backend cannot complete
    /// the join honestly.
    #[allow(clippy::needless_pass_by_value)]
    pub fn join(&self, handle: ThreadHandle) -> Result<ThreadTermination, ThreadError> {
        let ThreadHandle { inner } = handle;
        fusion_pal::sys::thread::ThreadLifecycle::join(&self.inner, inner)
    }

    /// Detaches a thread handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the handle is not detachable or the backend cannot detach it
    /// honestly.
    #[allow(clippy::needless_pass_by_value)]
    pub fn detach(&self, handle: ThreadHandle) -> Result<(), ThreadError> {
        let ThreadHandle { inner } = handle;
        fusion_pal::sys::thread::ThreadLifecycle::detach(&self.inner, inner)
    }

    /// Suspends a thread when the backend supports it.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend cannot suspend the handle honestly or does not
    /// support suspension at all.
    pub fn suspend(&self, handle: &ThreadHandle) -> Result<(), ThreadError> {
        fusion_pal::sys::thread::ThreadSuspendControl::suspend(&self.inner, &handle.inner)
    }

    /// Resumes a suspended thread when the backend supports it.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend cannot resume the handle honestly or does not
    /// support resume at all.
    pub fn resume(&self, handle: &ThreadHandle) -> Result<(), ThreadError> {
        fusion_pal::sys::thread::ThreadSuspendControl::resume(&self.inner, &handle.inner)
    }

    /// Queries the class-specific numeric priority range.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot observe the requested class range honestly.
    pub fn priority_range(
        &self,
        class: ThreadSchedulerClass,
    ) -> Result<Option<ThreadPriorityRange>, ThreadError> {
        fusion_pal::sys::thread::ThreadSchedulerControl::priority_range(&self.inner, class)
    }

    /// Applies scheduler policy to a thread handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot apply or honestly degrade the requested
    /// scheduler policy.
    pub fn set_scheduler(
        &self,
        handle: &ThreadHandle,
        request: &ThreadSchedulerRequest,
    ) -> Result<ThreadSchedulerObservation, ThreadError> {
        fusion_pal::sys::thread::ThreadSchedulerControl::set_scheduler(
            &self.inner,
            &handle.inner,
            request,
        )
    }

    /// Queries the effective scheduler policy for a thread handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot observe the effective scheduler state.
    pub fn scheduler(
        &self,
        handle: &ThreadHandle,
    ) -> Result<ThreadSchedulerObservation, ThreadError> {
        fusion_pal::sys::thread::ThreadSchedulerControl::scheduler(&self.inner, &handle.inner)
    }

    /// Yields the current thread to the scheduler.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot honestly yield the current thread.
    pub fn yield_now(&self) -> Result<(), ThreadError> {
        fusion_pal::sys::thread::ThreadSchedulerControl::yield_now(&self.inner)
    }

    /// Sleeps the current thread for a relative duration.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot honestly sleep for the requested duration.
    pub fn sleep_for(&self, duration: Duration) -> Result<(), ThreadError> {
        fusion_pal::sys::thread::ThreadSchedulerControl::sleep_for(&self.inner, duration)
    }

    /// Applies placement policy to a thread handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot apply or honestly degrade the requested
    /// placement policy.
    pub fn set_placement(
        &self,
        handle: &ThreadHandle,
        request: &ThreadPlacementRequest<'_>,
    ) -> Result<ThreadPlacementOutcome, ThreadError> {
        fusion_pal::sys::thread::ThreadPlacementControl::set_placement(
            &self.inner,
            &handle.inner,
            request,
        )
    }

    /// Queries effective placement for a thread handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot observe the effective placement honestly.
    pub fn placement(&self, handle: &ThreadHandle) -> Result<ThreadPlacementOutcome, ThreadError> {
        fusion_pal::sys::thread::ThreadPlacementControl::placement(&self.inner, &handle.inner)
    }

    /// Observes the current thread.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot produce a truthful current-thread observation.
    pub fn observe_current(&self) -> Result<ThreadObservation, ThreadError> {
        fusion_pal::sys::thread::ThreadObservationControl::observe_current(&self.inner)
    }

    /// Observes a specific thread handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot observe the requested handle honestly.
    pub fn observe(&self, handle: &ThreadHandle) -> Result<ThreadObservation, ThreadError> {
        fusion_pal::sys::thread::ThreadObservationControl::observe(&self.inner, &handle.inner)
    }

    /// Observes stack information for the current thread.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot observe current-thread stack state honestly.
    pub fn observe_current_stack(&self) -> Result<ThreadStackObservation, ThreadError> {
        fusion_pal::sys::thread::ThreadStackObservationControl::observe_current_stack(&self.inner)
    }

    /// Observes stack information for a specific thread handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot observe the requested handle's stack state
    /// honestly.
    pub fn observe_stack(
        &self,
        handle: &ThreadHandle,
    ) -> Result<ThreadStackObservation, ThreadError> {
        fusion_pal::sys::thread::ThreadStackObservationControl::observe_stack(
            &self.inner,
            &handle.inner,
        )
    }
}

impl Default for ThreadSystem {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns the process-wide system thread provider wrapper.
#[must_use]
pub const fn system_thread() -> ThreadSystem {
    ThreadSystem::new()
}
