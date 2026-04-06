//! Cortex-M bare-metal thread backend.
//!
//! Bare-metal Cortex-M does not grow OS threads out of the floorboards on its own. What this
//! backend can surface honestly today is the current execution context and, for SoCs that can
//! identify the active core, the current logical CPU location.

use core::arch::asm;
use core::time::Duration;

use crate::contract::pal::runtime::thread::{
    RawThreadEntry,
    ThreadAuthoritySet,
    ThreadBaseContract,
    ThreadConfig,
    ThreadError,
    ThreadGuarantee,
    ThreadId,
    ThreadIdentityStability,
    ThreadLifecycle,
    ThreadLifecycleCaps,
    ThreadLifecycleSupport,
    ThreadLocalitySupport,
    ThreadObservation,
    ThreadObservationControlContract,
    ThreadPlacementCaps,
    ThreadPlacementControlContract,
    ThreadPlacementOutcome,
    ThreadPlacementRequest,
    ThreadPlacementSupport,
    ThreadPriorityRange,
    ThreadRunState,
    ThreadSchedulerCaps,
    ThreadSchedulerClass,
    ThreadSchedulerControlContract,
    ThreadSchedulerModel,
    ThreadSchedulerObservation,
    ThreadSchedulerRequest,
    ThreadSchedulerSupport,
    ThreadStackObservation,
    ThreadStackObservationControlContract,
    ThreadSupport,
    ThreadSuspendControlContract,
    ThreadTermination,
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

impl ThreadBaseContract for CortexMThread {
    type Handle = CortexMThreadHandle;

    fn support(&self) -> ThreadSupport {
        let scheduler = scheduler_support();

        crate::pal::soc::cortex_m::hal::soc::board::current_execution_location().map_or_else(
            |_| ThreadSupport {
                lifecycle: ThreadLifecycleSupport::unsupported(),
                placement: ThreadPlacementSupport::unsupported(),
                scheduler,
                stack: crate::contract::pal::runtime::thread::ThreadStackSupport::unsupported(),
                locality: ThreadLocalitySupport::unsupported(),
            },
            |observation| ThreadSupport {
                lifecycle: ThreadLifecycleSupport {
                    caps: ThreadLifecycleCaps::CURRENT_THREAD_ID
                        .union(ThreadLifecycleCaps::CURRENT_OBSERVE),
                    identity_stability: ThreadIdentityStability::SystemLifetime,
                    authorities: observation.authorities,
                    implementation:
                        crate::contract::pal::runtime::thread::ThreadImplementationKind::Native,
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
                    implementation:
                        crate::contract::pal::runtime::thread::ThreadImplementationKind::Native,
                },
                scheduler,
                stack: crate::contract::pal::runtime::thread::ThreadStackSupport::unsupported(),
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
        crate::pal::soc::cortex_m::hal::soc::board::current_thread_id()
    }

    fn join(&self, _handle: Self::Handle) -> Result<ThreadTermination, ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn detach(&self, _handle: Self::Handle) -> Result<(), ThreadError> {
        Err(ThreadError::unsupported())
    }
}

impl ThreadSuspendControlContract for CortexMThread {
    fn suspend(&self, _handle: &Self::Handle) -> Result<(), ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn resume(&self, _handle: &Self::Handle) -> Result<(), ThreadError> {
        Err(ThreadError::unsupported())
    }
}

impl ThreadSchedulerControlContract for CortexMThread {
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
        // SAFETY: YIELD is an architected Cortex-M hint instruction. It does not dereference
        // memory or manipulate Rust-visible state beyond providing a voluntary execution hint.
        unsafe { asm!("yield", options(nomem, nostack, preserves_flags)) };
        Ok(())
    }

    fn sleep_for(&self, duration: Duration) -> Result<(), ThreadError> {
        if duration.is_zero() {
            return Ok(());
        }

        if !crate::pal::soc::cortex_m::hal::soc::board::event_timeout_supported() {
            return Err(ThreadError::unsupported());
        }

        crate::sys::vector::ensure_runtime_reserved_wake_vectors().map_err(map_vector_error)?;

        crate::pal::soc::cortex_m::hal::soc::board::arm_event_timeout(duration)
            .map_err(map_hardware_error)?;

        let sleep_result = loop {
            // SAFETY: WFI is an architected Cortex-M idle instruction. It does not violate Rust
            // aliasing or memory safety and simply yields until an interrupt becomes pending.
            unsafe { asm!("wfi", options(nomem, nostack, preserves_flags)) };
            match crate::pal::soc::cortex_m::hal::soc::board::event_timeout_fired() {
                Ok(true) => break Ok(()),
                Ok(false) => {}
                Err(error) => break Err(map_hardware_error(error)),
            }
        };

        let cancel_result = crate::pal::soc::cortex_m::hal::soc::board::cancel_event_timeout()
            .map_err(map_hardware_error);

        match (sleep_result, cancel_result) {
            (Err(error), _) | (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    fn monotonic_now(&self) -> Result<Duration, ThreadError> {
        crate::pal::soc::cortex_m::hal::soc::board::monotonic_now().map_err(map_hardware_error)
    }
}

impl ThreadPlacementControlContract for CortexMThread {
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

impl ThreadObservationControlContract for CortexMThread {
    fn observe_current(&self) -> Result<ThreadObservation, ThreadError> {
        let id = crate::pal::soc::cortex_m::hal::soc::board::current_thread_id()?;
        let observation = crate::pal::soc::cortex_m::hal::soc::board::current_execution_location()?;

        Ok(ThreadObservation {
            id,
            run_state: ThreadRunState::Running,
            location: observation.location,
            scheduler: ThreadSchedulerObservation::unknown(),
            placement: ThreadPlacementOutcome {
                guarantee: ThreadGuarantee::Verified,
                phase: crate::contract::pal::runtime::thread::ThreadPlacementPhase::Inherit,
                location: observation.location,
            },
        })
    }

    fn observe(&self, _handle: &Self::Handle) -> Result<ThreadObservation, ThreadError> {
        Err(ThreadError::unsupported())
    }
}

impl ThreadStackObservationControlContract for CortexMThread {
    fn observe_current_stack(&self) -> Result<ThreadStackObservation, ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn observe_stack(&self, _handle: &Self::Handle) -> Result<ThreadStackObservation, ThreadError> {
        Err(ThreadError::unsupported())
    }
}

fn scheduler_support() -> ThreadSchedulerSupport {
    let mut caps = ThreadSchedulerCaps::YIELD;
    let mut authorities = ThreadAuthoritySet::ISA;

    if crate::pal::soc::cortex_m::hal::soc::board::event_timeout_supported() {
        caps |= ThreadSchedulerCaps::SLEEP_FOR;
        authorities |= ThreadAuthoritySet::FIRMWARE;
    }
    if crate::pal::soc::cortex_m::hal::soc::board::monotonic_now_supported() {
        caps |= ThreadSchedulerCaps::MONOTONIC_NOW;
        authorities |= ThreadAuthoritySet::FIRMWARE;
    }

    ThreadSchedulerSupport {
        caps,
        model: ThreadSchedulerModel::Unknown,
        priority: ThreadGuarantee::Unsupported,
        realtime: ThreadGuarantee::Unsupported,
        deadline: ThreadGuarantee::Unsupported,
        observation: ThreadGuarantee::Unsupported,
        default_priority_range: None,
        authorities,
        implementation: crate::contract::pal::runtime::thread::ThreadImplementationKind::Emulated,
    }
}

const fn map_hardware_error(error: crate::contract::pal::HardwareError) -> ThreadError {
    match error.kind() {
        crate::contract::pal::HardwareErrorKind::Unsupported => ThreadError::unsupported(),
        crate::contract::pal::HardwareErrorKind::Invalid => ThreadError::invalid(),
        crate::contract::pal::HardwareErrorKind::ResourceExhausted => {
            ThreadError::resource_exhausted()
        }
        crate::contract::pal::HardwareErrorKind::StateConflict => ThreadError::state_conflict(),
        crate::contract::pal::HardwareErrorKind::Busy => ThreadError::busy(),
        crate::contract::pal::HardwareErrorKind::Platform(code) => ThreadError::platform(code),
    }
}

const fn map_vector_error(error: crate::contract::pal::vector::VectorError) -> ThreadError {
    match error.kind() {
        crate::contract::pal::vector::VectorErrorKind::Unsupported => ThreadError::unsupported(),
        crate::contract::pal::vector::VectorErrorKind::Invalid
        | crate::contract::pal::vector::VectorErrorKind::Reserved
        | crate::contract::pal::vector::VectorErrorKind::CoreMismatch
        | crate::contract::pal::vector::VectorErrorKind::WorldMismatch
        | crate::contract::pal::vector::VectorErrorKind::SealViolation => ThreadError::invalid(),
        crate::contract::pal::vector::VectorErrorKind::ResourceExhausted => {
            ThreadError::resource_exhausted()
        }
        crate::contract::pal::vector::VectorErrorKind::AlreadyBound
        | crate::contract::pal::vector::VectorErrorKind::NotBound
        | crate::contract::pal::vector::VectorErrorKind::StateConflict
        | crate::contract::pal::vector::VectorErrorKind::Sealed
        | crate::contract::pal::vector::VectorErrorKind::Platform(_) => {
            ThreadError::state_conflict()
        }
    }
}
