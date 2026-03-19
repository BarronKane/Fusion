//! Cortex-M bare-metal thread backend.
//!
//! Bare-metal Cortex-M does not grow OS threads out of the floorboards on its own. What this
//! backend can surface honestly today is the current execution context and, for SoCs that can
//! identify the active core, the current logical CPU location.

use core::time::Duration;

use crate::pal::thread::{
    RawThreadEntry, ThreadAuthoritySet, ThreadBase, ThreadConfig, ThreadError, ThreadGuarantee,
    ThreadId, ThreadIdentityStability, ThreadLifecycle, ThreadLifecycleCaps,
    ThreadLifecycleSupport, ThreadLocalitySupport, ThreadObservation, ThreadObservationControl,
    ThreadPlacementCaps, ThreadPlacementControl, ThreadPlacementOutcome, ThreadPlacementRequest,
    ThreadPlacementSupport, ThreadPriorityRange, ThreadRunState, ThreadSchedulerClass,
    ThreadSchedulerControl, ThreadSchedulerModel, ThreadSchedulerObservation,
    ThreadSchedulerRequest, ThreadSchedulerSupport, ThreadStackObservation,
    ThreadStackObservationControl, ThreadSupport, ThreadSuspendControl, ThreadTermination,
};

/// Cortex-M owned thread handle placeholder.
#[derive(Debug, Default, PartialEq, Eq, Hash)]
pub struct CortexMThreadHandle;

/// Cortex-M thread provider handle.
#[derive(Debug, Clone, Copy, Default)]
pub struct CortexMThread;

/// Selected thread handle type for Cortex-M builds.
pub type PlatformThreadHandle = CortexMThreadHandle;

/// Target-selected thread provider alias for Cortex-M builds.
pub type PlatformThread = CortexMThread;

/// Returns the process-wide Cortex-M thread provider handle.
#[must_use]
pub const fn system_thread() -> PlatformThread {
    PlatformThread::new()
}

impl CortexMThread {
    /// Creates a new Cortex-M thread provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ThreadBase for CortexMThread {
    type Handle = CortexMThreadHandle;

    fn support(&self) -> ThreadSupport {
        super::super::hal::soc::board::current_execution_location().map_or_else(
            |_| ThreadSupport::unsupported(),
            |observation| ThreadSupport {
                lifecycle: ThreadLifecycleSupport {
                    caps: ThreadLifecycleCaps::CURRENT_THREAD_ID
                        .union(ThreadLifecycleCaps::CURRENT_OBSERVE),
                    identity_stability: ThreadIdentityStability::SystemLifetime,
                    authorities: observation.authorities,
                    implementation: crate::pal::thread::ThreadImplementationKind::Native,
                },
                placement: ThreadPlacementSupport {
                    caps: ThreadPlacementCaps::CURRENT_CPU_OBSERVE
                        .union(ThreadPlacementCaps::EFFECTIVE_OBSERVE),
                    logical_cpu_affinity: ThreadGuarantee::Unsupported,
                    package_affinity: ThreadGuarantee::Unsupported,
                    numa_affinity: ThreadGuarantee::Unsupported,
                    core_class_affinity: ThreadGuarantee::Unsupported,
                    observation: ThreadGuarantee::Verified,
                    authorities: observation.authorities,
                    implementation: crate::pal::thread::ThreadImplementationKind::Native,
                },
                scheduler: ThreadSchedulerSupport {
                    caps: crate::pal::thread::ThreadSchedulerCaps::empty(),
                    model: ThreadSchedulerModel::Unknown,
                    priority: ThreadGuarantee::Unsupported,
                    realtime: ThreadGuarantee::Unsupported,
                    deadline: ThreadGuarantee::Unsupported,
                    observation: ThreadGuarantee::Unsupported,
                    default_priority_range: None,
                    authorities: ThreadAuthoritySet::empty(),
                    implementation: crate::pal::thread::ThreadImplementationKind::Unsupported,
                },
                stack: crate::pal::thread::ThreadStackSupport::unsupported(),
                locality: ThreadLocalitySupport::unsupported(),
            },
        )
    }
}

// SAFETY: this backend never creates or joins OS-style threads; it only surfaces current-core
// observation where the selected SoC can do so honestly.
unsafe impl ThreadLifecycle for CortexMThread {
    unsafe fn spawn(
        &self,
        _config: &ThreadConfig<'_>,
        _entry: RawThreadEntry,
        _context: *mut (),
    ) -> Result<Self::Handle, ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn current_thread_id(&self) -> Result<ThreadId, ThreadError> {
        super::super::hal::soc::board::current_thread_id()
    }

    fn join(&self, _handle: Self::Handle) -> Result<ThreadTermination, ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn detach(&self, _handle: Self::Handle) -> Result<(), ThreadError> {
        Err(ThreadError::unsupported())
    }
}

impl ThreadSuspendControl for CortexMThread {
    fn suspend(&self, _handle: &Self::Handle) -> Result<(), ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn resume(&self, _handle: &Self::Handle) -> Result<(), ThreadError> {
        Err(ThreadError::unsupported())
    }
}

impl ThreadSchedulerControl for CortexMThread {
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
}

impl ThreadPlacementControl for CortexMThread {
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

impl ThreadObservationControl for CortexMThread {
    fn observe_current(&self) -> Result<ThreadObservation, ThreadError> {
        let id = super::super::hal::soc::board::current_thread_id()?;
        let observation = super::super::hal::soc::board::current_execution_location()?;

        Ok(ThreadObservation {
            id,
            run_state: ThreadRunState::Running,
            location: observation.location,
            scheduler: ThreadSchedulerObservation::unknown(),
            placement: ThreadPlacementOutcome {
                guarantee: ThreadGuarantee::Verified,
                phase: crate::pal::thread::ThreadPlacementPhase::Inherit,
                location: observation.location,
            },
        })
    }

    fn observe(&self, _handle: &Self::Handle) -> Result<ThreadObservation, ThreadError> {
        Err(ThreadError::unsupported())
    }
}

impl ThreadStackObservationControl for CortexMThread {
    fn observe_current_stack(&self) -> Result<ThreadStackObservation, ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn observe_stack(&self, _handle: &Self::Handle) -> Result<ThreadStackObservation, ThreadError> {
        Err(ThreadError::unsupported())
    }
}
