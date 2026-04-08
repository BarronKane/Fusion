//! Cortex-M bare-metal thread backend.
//!
//! Bare-metal Cortex-M does not grow OS threads out of the floorboards on its own. What this
//! backend can surface honestly today is the current execution context and, for SoCs that can
//! identify the active core, the current logical CPU location.

use core::arch::asm;
use core::num::NonZeroUsize;
use core::ptr::NonNull;
use core::sync::atomic::{
    AtomicBool,
    AtomicU8,
    AtomicU32,
    AtomicUsize,
    Ordering,
};
use core::time::Duration;

use crate::contract::pal::runtime::thread::{
    RawThreadEntry,
    ThreadAuthoritySet,
    ThreadBaseContract,
    ThreadConfig,
    ThreadError,
    ThreadExecutionLocation,
    ThreadGuarantee,
    ThreadId,
    ThreadIdentityStability,
    ThreadJoinPolicy,
    ThreadLifecycle,
    ThreadLifecycleCaps,
    ThreadLifecycleSupport,
    ThreadLocalitySupport,
    ThreadObservation,
    ThreadObservationControlContract,
    ThreadPlacementCaps,
    ThreadPlacementControlContract,
    ThreadPlacementOutcome,
    ThreadPlacementPhase,
    ThreadPlacementRequest,
    ThreadPlacementSupport,
    ThreadPlacementTarget,
    ThreadProcessorGroupId,
    ThreadPriorityRange,
    ThreadRunState,
    ThreadSchedulerCaps,
    ThreadSchedulerClass,
    ThreadSchedulerControlContract,
    ThreadSchedulerModel,
    ThreadSchedulerObservation,
    ThreadSchedulerRequest,
    ThreadSchedulerSupport,
    ThreadCoreClassId,
    ThreadCoreId,
    ThreadClusterId,
    ThreadLogicalCpuId,
    ThreadStackBacking,
    ThreadStackCaps,
    ThreadExplicitBackingPlan,
    ThreadStackObservation,
    ThreadStackObservationControlContract,
    ThreadStackPrefaultPolicy,
    ThreadStackSupport,
    ThreadSupport,
    ThreadSuspendControlContract,
    ThreadTermination,
    ThreadExitCode,
    ThreadTerminationKind,
};

/// Cortex-M owned thread handle placeholder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct CortexMThreadHandle {
    #[cfg(feature = "soc-rp2350")]
    generation: u32,
}

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

        #[cfg(feature = "soc-rp2350")]
        if rp2350_threading_available() {
            return crate::pal::soc::cortex_m::hal::soc::board::current_execution_location()
                .map_or_else(
                    |_| rp2350_thread_support(scheduler),
                    |observation| ThreadSupport {
                        lifecycle: ThreadLifecycleSupport {
                            caps: ThreadLifecycleCaps::SPAWN
                                .union(ThreadLifecycleCaps::JOIN)
                                .union(ThreadLifecycleCaps::DETACH)
                                .union(ThreadLifecycleCaps::CURRENT_THREAD_ID)
                                .union(ThreadLifecycleCaps::CURRENT_OBSERVE)
                                .union(ThreadLifecycleCaps::HANDLE_OBSERVE)
                                .union(ThreadLifecycleCaps::EXIT_CODE),
                            identity_stability: ThreadIdentityStability::SystemLifetime,
                            authorities: observation.authorities | ThreadAuthoritySet::FIRMWARE,
                            implementation:
                                crate::contract::pal::runtime::thread::ThreadImplementationKind::Native,
                        },
                        placement: ThreadPlacementSupport {
                            caps: ThreadPlacementCaps::LOGICAL_CPU_AFFINITY
                                .union(ThreadPlacementCaps::PRESTART_APPLICATION)
                                .union(ThreadPlacementCaps::CURRENT_CPU_OBSERVE)
                                .union(ThreadPlacementCaps::EFFECTIVE_OBSERVE),
                            logical_cpu_affinity: ThreadGuarantee::Enforced,
                            package_affinity: ThreadGuarantee::Unsupported,
                            numa_affinity: ThreadGuarantee::Unsupported,
                            core_class_affinity: ThreadGuarantee::Unsupported,
                            observation: ThreadGuarantee::Verified,
                            authorities: observation.authorities | ThreadAuthoritySet::FIRMWARE,
                            implementation:
                                crate::contract::pal::runtime::thread::ThreadImplementationKind::Native,
                        },
                        scheduler,
                        stack: ThreadStackSupport {
                            caps: ThreadStackCaps::CALLER_PROVIDED
                                .union(ThreadStackCaps::EXPLICIT_SIZE),
                            explicit_size: ThreadGuarantee::Verified,
                            caller_provided: ThreadGuarantee::Verified,
                            prefault: ThreadGuarantee::Unsupported,
                            lock: ThreadGuarantee::Unsupported,
                            locality: ThreadGuarantee::Unsupported,
                            usage_observation: ThreadGuarantee::Unsupported,
                            default_explicit_backing: Some(ThreadExplicitBackingPlan {
                                size_bytes: NonZeroUsize::new(16 * 1024)
                                    .expect("non-zero RP2350 worker stack"),
                                align_bytes: NonZeroUsize::new(16)
                                    .expect("non-zero RP2350 stack alignment"),
                            }),
                            authorities: observation.authorities | ThreadAuthoritySet::FIRMWARE,
                            implementation:
                                crate::contract::pal::runtime::thread::ThreadImplementationKind::Native,
                        },
                        locality: ThreadLocalitySupport::unsupported(),
                    },
                );
        }

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
        config: &ThreadConfig<'_>,
        entry: RawThreadEntry,
        context: *mut (),
    ) -> Result<Self::Handle, ThreadError> {
        #[cfg(feature = "soc-rp2350")]
        if rp2350_threading_available() {
            return unsafe { rp2350_spawn_thread(config, entry, context) };
        }

        Err(ThreadError::unsupported())
    }

    fn current_thread_id(&self) -> Result<ThreadId, ThreadError> {
        crate::pal::soc::cortex_m::hal::soc::board::current_thread_id()
    }

    fn join(&self, handle: Self::Handle) -> Result<ThreadTermination, ThreadError> {
        #[cfg(feature = "soc-rp2350")]
        if rp2350_threading_available() {
            return rp2350_join_thread(handle);
        }

        Err(ThreadError::unsupported())
    }

    fn detach(&self, handle: Self::Handle) -> Result<(), ThreadError> {
        #[cfg(feature = "soc-rp2350")]
        if rp2350_threading_available() {
            return rp2350_detach_thread(handle);
        }

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
        #[cfg(feature = "soc-rp2350")]
        if rp2350_threading_available() {
            return rp2350_thread_placement(*_handle);
        }

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

    fn observe(&self, handle: &Self::Handle) -> Result<ThreadObservation, ThreadError> {
        #[cfg(feature = "soc-rp2350")]
        if rp2350_threading_available() {
            return rp2350_observe_thread(*handle);
        }

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

#[cfg(feature = "soc-rp2350")]
const RP2350_THREAD_IDLE: u8 = 0;
#[cfg(feature = "soc-rp2350")]
const RP2350_THREAD_LAUNCHING: u8 = 1;
#[cfg(feature = "soc-rp2350")]
const RP2350_THREAD_RUNNING_JOINABLE: u8 = 2;
#[cfg(feature = "soc-rp2350")]
const RP2350_THREAD_EXITED_JOINABLE: u8 = 3;
#[cfg(feature = "soc-rp2350")]
const RP2350_THREAD_RUNNING_DETACHED: u8 = 4;
#[cfg(feature = "soc-rp2350")]
const RP2350_THREAD_EXITED_DETACHED: u8 = 5;
#[cfg(feature = "soc-rp2350")]
const RP2350_SIO_FIFO_IRQN: u16 = 25;
#[cfg(all(feature = "soc-rp2350", target_abi = "eabihf"))]
const CORTEX_M33_CPACR: *mut u32 = 0xE000_ED88 as *mut u32;
#[cfg(all(feature = "soc-rp2350", target_abi = "eabihf"))]
const CORTEX_M33_CPACR_CP10_CP11_FULL_ACCESS: u32 = 0x00F0_0000;
#[cfg(feature = "soc-rp2350")]
const RP2350_SIO_FIFO_ST_OFFSET: usize = 0x50;
#[cfg(feature = "soc-rp2350")]
const RP2350_SIO_FIFO_WR_OFFSET: usize = 0x54;
#[cfg(feature = "soc-rp2350")]
const RP2350_SIO_FIFO_RD_OFFSET: usize = 0x58;
#[cfg(feature = "soc-rp2350")]
const RP2350_SIO_FIFO_ST_VLD_BITS: u32 = 0x1;
#[cfg(feature = "soc-rp2350")]
const RP2350_SIO_FIFO_ST_RDY_BITS: u32 = 0x2;
#[cfg(feature = "soc-rp2350")]
const RP2350_PSM_BASE: usize = 0x4001_8000;
#[cfg(feature = "soc-rp2350")]
const RP2350_PSM_FRCE_ON_OFFSET: usize = 0x0;
#[cfg(feature = "soc-rp2350")]
const RP2350_PSM_FRCE_OFF_OFFSET: usize = 0x4;
#[cfg(feature = "soc-rp2350")]
const RP2350_PSM_FRCE_PROC1_BITS: u32 = 0x0100_0000;
#[cfg(feature = "soc-rp2350")]
const CORTEX_M33_VTOR: *const u32 = 0xE000_ED08 as *const u32;

#[cfg(feature = "soc-rp2350")]
static RP2350_THREAD_STATE: AtomicU8 = AtomicU8::new(RP2350_THREAD_IDLE);
#[cfg(feature = "soc-rp2350")]
static RP2350_THREAD_GENERATION: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "soc-rp2350")]
static RP2350_THREAD_ENTRY: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "soc-rp2350")]
static RP2350_THREAD_CONTEXT: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "soc-rp2350")]
static RP2350_THREAD_EXIT_CODE: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "soc-rp2350")]
static RP2350_THREAD_ACTIVE: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "soc-rp2350")]
fn rp2350_threading_available() -> bool {
    crate::pal::soc::cortex_m::hal::soc::board::topology_summary().is_ok_and(|summary| {
        summary.logical_cpu_count.unwrap_or(0) >= 2 && summary.core_count.unwrap_or(0) >= 2
    })
}

#[cfg(feature = "soc-rp2350")]
fn rp2350_thread_support(scheduler: ThreadSchedulerSupport) -> ThreadSupport {
    ThreadSupport {
        lifecycle: ThreadLifecycleSupport {
            caps: ThreadLifecycleCaps::SPAWN
                .union(ThreadLifecycleCaps::JOIN)
                .union(ThreadLifecycleCaps::DETACH)
                .union(ThreadLifecycleCaps::CURRENT_THREAD_ID)
                .union(ThreadLifecycleCaps::CURRENT_OBSERVE)
                .union(ThreadLifecycleCaps::HANDLE_OBSERVE)
                .union(ThreadLifecycleCaps::EXIT_CODE),
            identity_stability: ThreadIdentityStability::SystemLifetime,
            authorities: ThreadAuthoritySet::FIRMWARE | ThreadAuthoritySet::TOPOLOGY,
            implementation: crate::contract::pal::runtime::thread::ThreadImplementationKind::Native,
        },
        placement: ThreadPlacementSupport {
            caps: ThreadPlacementCaps::LOGICAL_CPU_AFFINITY
                .union(ThreadPlacementCaps::PRESTART_APPLICATION)
                .union(ThreadPlacementCaps::CURRENT_CPU_OBSERVE)
                .union(ThreadPlacementCaps::EFFECTIVE_OBSERVE),
            logical_cpu_affinity: ThreadGuarantee::Enforced,
            package_affinity: ThreadGuarantee::Unsupported,
            numa_affinity: ThreadGuarantee::Unsupported,
            core_class_affinity: ThreadGuarantee::Unsupported,
            observation: ThreadGuarantee::Verified,
            authorities: ThreadAuthoritySet::FIRMWARE | ThreadAuthoritySet::TOPOLOGY,
            implementation: crate::contract::pal::runtime::thread::ThreadImplementationKind::Native,
        },
        scheduler,
        stack: ThreadStackSupport {
            caps: ThreadStackCaps::CALLER_PROVIDED.union(ThreadStackCaps::EXPLICIT_SIZE),
            explicit_size: ThreadGuarantee::Verified,
            caller_provided: ThreadGuarantee::Verified,
            prefault: ThreadGuarantee::Unsupported,
            lock: ThreadGuarantee::Unsupported,
            locality: ThreadGuarantee::Unsupported,
            usage_observation: ThreadGuarantee::Unsupported,
            default_explicit_backing: Some(ThreadExplicitBackingPlan {
                size_bytes: NonZeroUsize::new(16 * 1024).expect("non-zero RP2350 worker stack"),
                align_bytes: NonZeroUsize::new(16).expect("non-zero RP2350 stack alignment"),
            }),
            authorities: ThreadAuthoritySet::FIRMWARE,
            implementation: crate::contract::pal::runtime::thread::ThreadImplementationKind::Native,
        },
        locality: ThreadLocalitySupport::unsupported(),
    }
}

#[cfg(feature = "soc-rp2350")]
const fn rp2350_secondary_thread_id() -> ThreadId {
    ThreadId(1)
}

#[cfg(feature = "soc-rp2350")]
const fn rp2350_secondary_thread_location() -> ThreadExecutionLocation {
    ThreadExecutionLocation {
        logical_cpu: Some(ThreadLogicalCpuId {
            group: ThreadProcessorGroupId(0),
            index: 1,
        }),
        core: Some(ThreadCoreId(1)),
        cluster: Some(ThreadClusterId(0)),
        package: Some(crate::contract::pal::mem::MemTopologyNodeId(0)),
        numa_node: None,
        core_class: Some(ThreadCoreClassId(0)),
    }
}

#[cfg(feature = "soc-rp2350")]
fn rp2350_current_core() -> Result<u32, ThreadError> {
    crate::pal::soc::cortex_m::hal::soc::board::current_execution_location()?
        .location
        .core
        .map(|core| core.0)
        .ok_or_else(ThreadError::unsupported)
}

#[cfg(feature = "soc-rp2350")]
fn rp2350_validate_spawn_config(
    config: &ThreadConfig<'_>,
) -> Result<(NonNull<u8>, NonZeroUsize), ThreadError> {
    if !matches!(
        config.scheduler,
        ThreadSchedulerRequest {
            class: ThreadSchedulerClass::Inherit,
            priority: None,
            deadline: None,
        }
    ) {
        return Err(ThreadError::scheduler_denied());
    }

    if !matches!(config.stack.prefault, ThreadStackPrefaultPolicy::Inherit)
        || !matches!(config.stack.lock, crate::contract::pal::runtime::thread::ThreadStackLockPolicy::Inherit)
        || !matches!(
            config.stack.locality,
            crate::contract::pal::runtime::thread::ThreadStackLocalityPolicy::InheritProcessPolicy
                | crate::contract::pal::runtime::thread::ThreadStackLocalityPolicy::FollowThreadPlacement
        )
    {
        return Err(ThreadError::stack_denied());
    }

    let (base, len) = match config.stack.backing {
        ThreadStackBacking::CallerProvided { base, len } => (base, len),
        ThreadStackBacking::Default => return Err(ThreadError::stack_denied()),
    };
    if let Some(size_bytes) = config.stack.size_bytes
        && size_bytes != len
    {
        return Err(ThreadError::invalid());
    }
    if config.stack.guard_bytes.is_some() {
        return Err(ThreadError::stack_denied());
    }

    if !config.placement.targets.is_empty() {
        for target in config.placement.targets {
            match target {
                ThreadPlacementTarget::LogicalCpus(cpus)
                    if cpus.len() == 1 && cpus[0].group.0 == 0 && cpus[0].index == 1 => {}
                ThreadPlacementTarget::LogicalCpus(_) => {
                    return Err(ThreadError::placement_denied());
                }
                ThreadPlacementTarget::Packages(_)
                | ThreadPlacementTarget::NumaNodes(_)
                | ThreadPlacementTarget::CoreClasses(_) => {
                    return Err(ThreadError::placement_denied());
                }
            }
        }
    }

    Ok((base, len))
}

#[cfg(feature = "soc-rp2350")]
const fn rp2350_running_state(join_policy: ThreadJoinPolicy) -> u8 {
    match join_policy {
        ThreadJoinPolicy::Joinable => RP2350_THREAD_RUNNING_JOINABLE,
        ThreadJoinPolicy::Detached => RP2350_THREAD_RUNNING_DETACHED,
    }
}

#[cfg(feature = "soc-rp2350")]
unsafe fn rp2350_spawn_thread(
    config: &ThreadConfig<'_>,
    entry: RawThreadEntry,
    context: *mut (),
) -> Result<CortexMThreadHandle, ThreadError> {
    if rp2350_current_core()? != 0 {
        return Err(ThreadError::permission_denied());
    }
    let (stack_base, stack_len) = rp2350_validate_spawn_config(config)?;
    match RP2350_THREAD_STATE.load(Ordering::Acquire) {
        RP2350_THREAD_IDLE | RP2350_THREAD_EXITED_DETACHED => {}
        _ => return Err(ThreadError::busy()),
    }

    if RP2350_THREAD_STATE.load(Ordering::Acquire) == RP2350_THREAD_EXITED_DETACHED {
        rp2350_reset_secondary_core();
        RP2350_THREAD_ACTIVE.store(false, Ordering::Release);
        RP2350_THREAD_STATE.store(RP2350_THREAD_IDLE, Ordering::Release);
    }

    let generation = RP2350_THREAD_GENERATION
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1);
    if generation == 0 {
        return Err(ThreadError::resource_exhausted());
    }

    RP2350_THREAD_ENTRY.store(entry as usize, Ordering::Release);
    RP2350_THREAD_CONTEXT.store(context.addr(), Ordering::Release);
    RP2350_THREAD_EXIT_CODE.store(0, Ordering::Release);
    RP2350_THREAD_ACTIVE.store(true, Ordering::Release);
    RP2350_THREAD_STATE.store(RP2350_THREAD_LAUNCHING, Ordering::Release);

    let stack_top = stack_base
        .as_ptr()
        .wrapping_add(stack_len.get())
        .map_addr(|addr| addr & !0x7)
        .cast::<u32>();
    rp2350_launch_secondary_core(rp2350_secondary_thread_entry, stack_top)?;
    RP2350_THREAD_STATE.store(rp2350_running_state(config.join_policy), Ordering::Release);

    Ok(CortexMThreadHandle { generation })
}

#[cfg(feature = "soc-rp2350")]
fn rp2350_join_thread(handle: CortexMThreadHandle) -> Result<ThreadTermination, ThreadError> {
    if rp2350_current_core()? != 0 {
        return Err(ThreadError::permission_denied());
    }
    if !RP2350_THREAD_ACTIVE.load(Ordering::Acquire)
        || RP2350_THREAD_GENERATION.load(Ordering::Acquire) != handle.generation
    {
        return Err(ThreadError::state_conflict());
    }

    loop {
        match RP2350_THREAD_STATE.load(Ordering::Acquire) {
            RP2350_THREAD_EXITED_JOINABLE => {
                let code = RP2350_THREAD_EXIT_CODE.load(Ordering::Acquire);
                rp2350_reset_secondary_core();
                RP2350_THREAD_ACTIVE.store(false, Ordering::Release);
                RP2350_THREAD_STATE.store(RP2350_THREAD_IDLE, Ordering::Release);
                return Ok(ThreadTermination {
                    kind: ThreadTerminationKind::Returned,
                    code: Some(ThreadExitCode(code)),
                });
            }
            RP2350_THREAD_RUNNING_DETACHED | RP2350_THREAD_EXITED_DETACHED => {
                return Err(ThreadError::state_conflict());
            }
            RP2350_THREAD_IDLE => return Err(ThreadError::state_conflict()),
            _ => unsafe { asm!("wfe", options(nomem, nostack, preserves_flags)) },
        }
    }
}

#[cfg(feature = "soc-rp2350")]
fn rp2350_detach_thread(handle: CortexMThreadHandle) -> Result<(), ThreadError> {
    if !RP2350_THREAD_ACTIVE.load(Ordering::Acquire)
        || RP2350_THREAD_GENERATION.load(Ordering::Acquire) != handle.generation
    {
        return Err(ThreadError::state_conflict());
    }

    loop {
        match RP2350_THREAD_STATE.load(Ordering::Acquire) {
            RP2350_THREAD_RUNNING_JOINABLE => {
                if RP2350_THREAD_STATE
                    .compare_exchange(
                        RP2350_THREAD_RUNNING_JOINABLE,
                        RP2350_THREAD_RUNNING_DETACHED,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    return Ok(());
                }
            }
            RP2350_THREAD_EXITED_JOINABLE => {
                rp2350_reset_secondary_core();
                RP2350_THREAD_ACTIVE.store(false, Ordering::Release);
                RP2350_THREAD_STATE.store(RP2350_THREAD_IDLE, Ordering::Release);
                return Ok(());
            }
            RP2350_THREAD_RUNNING_DETACHED | RP2350_THREAD_EXITED_DETACHED => return Ok(()),
            RP2350_THREAD_IDLE => return Err(ThreadError::state_conflict()),
            _ => core::hint::spin_loop(),
        }
    }
}

#[cfg(feature = "soc-rp2350")]
fn rp2350_thread_placement(
    handle: CortexMThreadHandle,
) -> Result<ThreadPlacementOutcome, ThreadError> {
    if !RP2350_THREAD_ACTIVE.load(Ordering::Acquire)
        || RP2350_THREAD_GENERATION.load(Ordering::Acquire) != handle.generation
    {
        return Err(ThreadError::state_conflict());
    }
    Ok(ThreadPlacementOutcome {
        guarantee: ThreadGuarantee::Verified,
        phase: ThreadPlacementPhase::PreStartPreferred,
        location: rp2350_secondary_thread_location(),
    })
}

#[cfg(feature = "soc-rp2350")]
fn rp2350_observe_thread(handle: CortexMThreadHandle) -> Result<ThreadObservation, ThreadError> {
    if !RP2350_THREAD_ACTIVE.load(Ordering::Acquire)
        || RP2350_THREAD_GENERATION.load(Ordering::Acquire) != handle.generation
    {
        return Err(ThreadError::state_conflict());
    }
    let run_state = match RP2350_THREAD_STATE.load(Ordering::Acquire) {
        RP2350_THREAD_LAUNCHING => ThreadRunState::Starting,
        RP2350_THREAD_RUNNING_JOINABLE | RP2350_THREAD_RUNNING_DETACHED => ThreadRunState::Running,
        RP2350_THREAD_EXITED_JOINABLE | RP2350_THREAD_EXITED_DETACHED => ThreadRunState::Exited,
        _ => ThreadRunState::Unknown,
    };
    Ok(ThreadObservation {
        id: rp2350_secondary_thread_id(),
        run_state,
        location: rp2350_secondary_thread_location(),
        scheduler: ThreadSchedulerObservation::unknown(),
        placement: ThreadPlacementOutcome {
            guarantee: ThreadGuarantee::Verified,
            phase: ThreadPlacementPhase::PreStartPreferred,
            location: rp2350_secondary_thread_location(),
        },
    })
}

#[cfg(feature = "soc-rp2350")]
unsafe fn rp2350_secondary_thread_entry() {
    #[cfg(target_abi = "eabihf")]
    rp2350_enable_secondary_core_fpu();

    let entry = RP2350_THREAD_ENTRY.load(Ordering::Acquire);
    let context = RP2350_THREAD_CONTEXT.load(Ordering::Acquire);
    if entry == 0 {
        loop {
            unsafe { asm!("wfe", options(nomem, nostack, preserves_flags)) };
        }
    }

    // SAFETY: the spawn path stores one valid raw entry function for the worker generation.
    let entry: RawThreadEntry = unsafe { core::mem::transmute::<usize, RawThreadEntry>(entry) };
    let result = unsafe { entry(context as *mut ()) };
    RP2350_THREAD_EXIT_CODE.store(result.code.0, Ordering::Release);
    let next_state = match RP2350_THREAD_STATE.load(Ordering::Acquire) {
        RP2350_THREAD_RUNNING_DETACHED => RP2350_THREAD_EXITED_DETACHED,
        _ => RP2350_THREAD_EXITED_JOINABLE,
    };
    RP2350_THREAD_STATE.store(next_state, Ordering::Release);
    unsafe { asm!("sev", options(nomem, nostack, preserves_flags)) };

    loop {
        unsafe { asm!("wfe", options(nomem, nostack, preserves_flags)) };
    }
}

#[cfg(all(feature = "soc-rp2350", target_abi = "eabihf"))]
fn rp2350_enable_secondary_core_fpu() {
    unsafe {
        let current = core::ptr::read_volatile(CORTEX_M33_CPACR);
        core::ptr::write_volatile(
            CORTEX_M33_CPACR,
            current | CORTEX_M33_CPACR_CP10_CP11_FULL_ACCESS,
        );
        asm!("dsb", options(nomem, nostack, preserves_flags));
        asm!("isb", options(nomem, nostack, preserves_flags));
    }
}

#[cfg(feature = "soc-rp2350")]
fn rp2350_reset_secondary_core() {
    let force_on = (RP2350_PSM_BASE + RP2350_PSM_FRCE_ON_OFFSET) as *mut u32;
    let force_off = (RP2350_PSM_BASE + RP2350_PSM_FRCE_OFF_OFFSET) as *mut u32;
    let irq_enabled = rp2350_nvic_line_enabled(RP2350_SIO_FIFO_IRQN);
    if irq_enabled {
        let _ = crate::pal::soc::cortex_m::hal::soc::rp2350::irq_disable(RP2350_SIO_FIFO_IRQN);
    }

    unsafe {
        core::ptr::write_volatile(force_off, RP2350_PSM_FRCE_PROC1_BITS);
        while core::ptr::read_volatile(force_off) & RP2350_PSM_FRCE_PROC1_BITS == 0 {
            core::hint::spin_loop();
        }
        core::ptr::write_volatile(force_on, RP2350_PSM_FRCE_PROC1_BITS);
    }
    rp2350_fifo_wait_for_value(0);
    if irq_enabled {
        let _ = crate::pal::soc::cortex_m::hal::soc::rp2350::irq_enable(RP2350_SIO_FIFO_IRQN);
    }
}

#[cfg(feature = "soc-rp2350")]
fn rp2350_launch_secondary_core(entry: unsafe fn(), sp: *mut u32) -> Result<(), ThreadError> {
    let irq_enabled = rp2350_nvic_line_enabled(RP2350_SIO_FIFO_IRQN);
    if irq_enabled {
        crate::pal::soc::cortex_m::hal::soc::rp2350::irq_disable(RP2350_SIO_FIFO_IRQN)
            .map_err(map_hardware_error)?;
    }

    let vector_table = unsafe { core::ptr::read_volatile(CORTEX_M33_VTOR) as usize };
    let commands = [0usize, 0, 1, vector_table, sp.addr(), entry as usize];
    let mut seq = 0usize;
    while seq < commands.len() {
        let command = commands[seq];
        if command == 0 {
            rp2350_fifo_drain();
            unsafe { asm!("sev", options(nomem, nostack, preserves_flags)) };
        }
        rp2350_fifo_push_blocking(command as u32);
        let response = rp2350_fifo_pop_blocking() as usize;
        seq = if response == command { seq + 1 } else { 0 };
    }

    if irq_enabled {
        crate::pal::soc::cortex_m::hal::soc::rp2350::irq_enable(RP2350_SIO_FIFO_IRQN)
            .map_err(map_hardware_error)?;
    }
    Ok(())
}

#[cfg(feature = "soc-rp2350")]
fn rp2350_fifo_status() -> u32 {
    let status = (crate::pal::soc::cortex_m::hal::soc::rp2350::RP2350_SIO_BASE
        + RP2350_SIO_FIFO_ST_OFFSET) as *const u32;
    unsafe { core::ptr::read_volatile(status) }
}

#[cfg(feature = "soc-rp2350")]
fn rp2350_fifo_drain() {
    let fifo_rd = (crate::pal::soc::cortex_m::hal::soc::rp2350::RP2350_SIO_BASE
        + RP2350_SIO_FIFO_RD_OFFSET) as *const u32;
    while (rp2350_fifo_status() & RP2350_SIO_FIFO_ST_VLD_BITS) != 0 {
        unsafe {
            let _ = core::ptr::read_volatile(fifo_rd);
        }
    }
}

#[cfg(feature = "soc-rp2350")]
fn rp2350_fifo_push_blocking(value: u32) {
    let fifo_wr = (crate::pal::soc::cortex_m::hal::soc::rp2350::RP2350_SIO_BASE
        + RP2350_SIO_FIFO_WR_OFFSET) as *mut u32;
    while (rp2350_fifo_status() & RP2350_SIO_FIFO_ST_RDY_BITS) == 0 {
        core::hint::spin_loop();
    }
    unsafe {
        core::ptr::write_volatile(fifo_wr, value);
        asm!("sev", options(nomem, nostack, preserves_flags));
    }
}

#[cfg(feature = "soc-rp2350")]
fn rp2350_fifo_pop_blocking() -> u32 {
    let fifo_rd = (crate::pal::soc::cortex_m::hal::soc::rp2350::RP2350_SIO_BASE
        + RP2350_SIO_FIFO_RD_OFFSET) as *const u32;
    while (rp2350_fifo_status() & RP2350_SIO_FIFO_ST_VLD_BITS) == 0 {
        unsafe { asm!("wfe", options(nomem, nostack, preserves_flags)) };
    }
    unsafe { core::ptr::read_volatile(fifo_rd) }
}

#[cfg(feature = "soc-rp2350")]
fn rp2350_fifo_wait_for_value(expected: u32) {
    loop {
        if rp2350_fifo_pop_blocking() == expected {
            return;
        }
    }
}

#[cfg(feature = "soc-rp2350")]
fn rp2350_nvic_line_enabled(irqn: u16) -> bool {
    let register_index = usize::from(irqn / 32);
    let bit = 1u32 << u32::from(irqn % 32);
    let register = unsafe {
        crate::pal::soc::cortex_m::hal::soc::rp2350::CORTEX_M_NVIC_ISER.add(register_index)
    };
    unsafe { core::ptr::read_volatile(register) & bit != 0 }
}
