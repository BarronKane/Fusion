use bitflags::bitflags;

use super::{
    ThreadIdentityStability,
    scheduler::ThreadPriorityRange,
    scheduler::ThreadSchedulerModel,
};

/// Shared authority bitset specialized for thread support.
pub use crate::contract::pal::caps::AuthoritySet as ThreadAuthoritySet;

/// Shared guarantee ladder specialized for thread support.
pub use crate::contract::pal::caps::Guarantee as ThreadGuarantee;

/// Shared implementation-category vocabulary specialized for thread support.
pub use crate::contract::pal::caps::ImplementationKind as ThreadImplementationKind;

bitflags! {
    /// Lifecycle capabilities a backend may expose for threads.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ThreadLifecycleCaps: u32 {
        /// Supports creating new threads.
        const SPAWN               = 1 << 0;
        /// Supports joining a thread to collect termination.
        const JOIN                = 1 << 1;
        /// Supports detaching a thread.
        const DETACH              = 1 << 2;
        /// Supports naming threads.
        const NAME                = 1 << 3;
        /// Supports querying the current thread identifier.
        const CURRENT_THREAD_ID   = 1 << 4;
        /// Supports observing the current thread.
        const CURRENT_OBSERVE     = 1 << 5;
        /// Supports observing another thread handle.
        const HANDLE_OBSERVE      = 1 << 6;
        /// Supports surfacing a thread-defined termination code.
        const EXIT_CODE           = 1 << 7;
        /// Supports suspending another thread.
        const SUSPEND            = 1 << 8;
        /// Supports resuming a suspended thread.
        const RESUME             = 1 << 9;
    }
}

bitflags! {
    /// Placement and migration control capabilities for threads.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ThreadPlacementCaps: u32 {
        /// Supports affinity requests against logical CPU identifiers.
        const LOGICAL_CPU_AFFINITY    = 1 << 0;
        /// Supports affinity requests against package or socket topology.
        const PACKAGE_AFFINITY        = 1 << 1;
        /// Supports affinity requests against NUMA topology.
        const NUMA_AFFINITY           = 1 << 2;
        /// Supports affinity requests against core classes or heterogeneity buckets.
        const CORE_CLASS_AFFINITY     = 1 << 3;
        /// Supports placement before user entry begins.
        const PRESTART_APPLICATION    = 1 << 4;
        /// Supports changing placement after thread start.
        const POSTSTART_APPLICATION   = 1 << 5;
        /// Supports observing the current logical CPU.
        const CURRENT_CPU_OBSERVE     = 1 << 6;
        /// Supports observing the effective applied placement.
        const EFFECTIVE_OBSERVE       = 1 << 7;
        /// Supports observing migration or last-CPU history.
        const MIGRATION_OBSERVE       = 1 << 8;
    }
}

bitflags! {
    /// Scheduler capabilities a backend may expose for threads.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ThreadSchedulerCaps: u32 {
        /// Supports voluntary scheduler yield.
        const YIELD                  = 1 << 0;
        /// Supports relative sleep.
        const SLEEP_FOR              = 1 << 1;
        /// Supports observing the current monotonic scheduler-visible timebase.
        const MONOTONIC_NOW          = 1 << 2;
        /// Supports explicit numeric or classed priority settings.
        const PRIORITY               = 1 << 3;
        /// Supports querying effective priority.
        const QUERY_PRIORITY         = 1 << 4;
        /// Supports explicit scheduler classes.
        const CLASS                  = 1 << 5;
        /// Supports querying effective scheduler class.
        const QUERY_CLASS            = 1 << 6;
        /// Supports fixed-priority realtime scheduling.
        const REALTIME_FIXED         = 1 << 7;
        /// Supports round-robin realtime scheduling.
        const REALTIME_ROUND_ROBIN   = 1 << 8;
        /// Supports deadline-style scheduling.
        const DEADLINE               = 1 << 9;
    }
}

bitflags! {
    /// Stack and startup-memory capabilities a backend may expose for threads.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ThreadStackCaps: u32 {
        /// Supports explicit stack sizing.
        const EXPLICIT_SIZE          = 1 << 0;
        /// Supports explicit guard sizing.
        const GUARD_SIZE             = 1 << 1;
        /// Supports caller-provided stack backing.
        const CALLER_PROVIDED        = 1 << 2;
        /// Supports creator-side prefaulting of thread stack backing.
        const PREFAULT_CREATOR       = 1 << 3;
        /// Supports target-thread prefaulting before user entry.
        const PREFAULT_TARGET        = 1 << 4;
        /// Supports locking or pinning thread stack backing.
        const LOCK                   = 1 << 5;
        /// Supports stack-locality policy beyond inherited process defaults.
        const LOCALITY_POLICY        = 1 << 6;
        /// Supports observing effective stack-locality outcome.
        const LOCALITY_OBSERVE       = 1 << 7;
        /// Supports observing stack-usage or high-water-mark information.
        const USAGE_OBSERVE          = 1 << 8;
    }
}

bitflags! {
    /// Locality-observation and first-touch related capabilities for threads.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ThreadLocalityCaps: u32 {
        /// Supports controlling stack first-touch behavior.
        const STACK_FIRST_TOUCH_CONTROL   = 1 << 0;
        /// Supports observing current package or socket placement.
        const PACKAGE_OBSERVE             = 1 << 1;
        /// Supports observing current NUMA placement.
        const NUMA_OBSERVE                = 1 << 2;
        /// Supports observing current core class or heterogeneity bucket.
        const CORE_CLASS_OBSERVE          = 1 << 3;
        /// Supports observing effective memory-locality policy inherited by the thread.
        const MEMORY_POLICY_OBSERVE       = 1 << 4;
        /// Supports explicit inherited-memory-policy control at thread start.
        const MEMORY_POLICY_CONTROL       = 1 << 5;
    }
}

/// Lifecycle support offered by a thread backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadLifecycleSupport {
    /// Fine-grained lifecycle capability flags.
    pub caps: ThreadLifecycleCaps,
    /// Stability guarantee for thread identifiers surfaced by the backend.
    pub identity_stability: ThreadIdentityStability,
    /// Evidence sources used to justify the effective lifecycle surface.
    pub authorities: ThreadAuthoritySet,
    /// Whether the lifecycle support is native, emulated, or unavailable.
    pub implementation: ThreadImplementationKind,
}

impl ThreadLifecycleSupport {
    /// Returns an explicitly unsupported lifecycle surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: ThreadLifecycleCaps::empty(),
            identity_stability: ThreadIdentityStability::Unknown,
            authorities: ThreadAuthoritySet::empty(),
            implementation: ThreadImplementationKind::Unsupported,
        }
    }
}

/// Placement and migration support offered by a thread backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadPlacementSupport {
    /// Fine-grained placement capability flags.
    pub caps: ThreadPlacementCaps,
    /// Strength of logical-CPU affinity guarantees.
    pub logical_cpu_affinity: ThreadGuarantee,
    /// Strength of package/socket locality guarantees.
    pub package_affinity: ThreadGuarantee,
    /// Strength of NUMA-locality guarantees.
    pub numa_affinity: ThreadGuarantee,
    /// Strength of core-class locality guarantees.
    pub core_class_affinity: ThreadGuarantee,
    /// Strength of effective placement observation.
    pub observation: ThreadGuarantee,
    /// Evidence sources used to justify the effective placement surface.
    pub authorities: ThreadAuthoritySet,
    /// Whether the placement support is native, emulated, or unavailable.
    pub implementation: ThreadImplementationKind,
}

impl ThreadPlacementSupport {
    /// Returns an explicitly unsupported placement surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: ThreadPlacementCaps::empty(),
            logical_cpu_affinity: ThreadGuarantee::Unsupported,
            package_affinity: ThreadGuarantee::Unsupported,
            numa_affinity: ThreadGuarantee::Unsupported,
            core_class_affinity: ThreadGuarantee::Unsupported,
            observation: ThreadGuarantee::Unsupported,
            authorities: ThreadAuthoritySet::empty(),
            implementation: ThreadImplementationKind::Unsupported,
        }
    }
}

/// Scheduler support offered by a thread backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadSchedulerSupport {
    /// Fine-grained scheduler capability flags.
    pub caps: ThreadSchedulerCaps,
    /// Scheduler model governing runnable threads.
    pub model: ThreadSchedulerModel,
    /// Strength of priority control guarantees.
    pub priority: ThreadGuarantee,
    /// Strength of realtime scheduling guarantees.
    pub realtime: ThreadGuarantee,
    /// Strength of deadline scheduling guarantees.
    pub deadline: ThreadGuarantee,
    /// Strength of effective scheduler observation.
    pub observation: ThreadGuarantee,
    /// Priority range for the inherited or default scheduler class, when one is meaningful
    /// to surface without an explicit class-specific query.
    pub default_priority_range: Option<ThreadPriorityRange>,
    /// Evidence sources used to justify the scheduler surface.
    pub authorities: ThreadAuthoritySet,
    /// Whether the scheduler support is native, emulated, or unavailable.
    pub implementation: ThreadImplementationKind,
}

impl ThreadSchedulerSupport {
    /// Returns an explicitly unsupported scheduler surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: ThreadSchedulerCaps::empty(),
            model: ThreadSchedulerModel::Unknown,
            priority: ThreadGuarantee::Unsupported,
            realtime: ThreadGuarantee::Unsupported,
            deadline: ThreadGuarantee::Unsupported,
            observation: ThreadGuarantee::Unsupported,
            default_priority_range: None,
            authorities: ThreadAuthoritySet::empty(),
            implementation: ThreadImplementationKind::Unsupported,
        }
    }
}

/// Stack and startup-memory support offered by a thread backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadStackSupport {
    /// Fine-grained stack capability flags.
    pub caps: ThreadStackCaps,
    /// Strength of explicit stack sizing guarantees.
    pub explicit_size: ThreadGuarantee,
    /// Strength of caller-provided stack guarantees.
    pub caller_provided: ThreadGuarantee,
    /// Strength of prefault and startup-memory instantiation guarantees.
    pub prefault: ThreadGuarantee,
    /// Strength of stack lock/pin guarantees.
    pub lock: ThreadGuarantee,
    /// Strength of stack-locality guarantees.
    pub locality: ThreadGuarantee,
    /// Strength of stack-usage or high-water-mark observation.
    pub usage_observation: ThreadGuarantee,
    /// Canonical caller-provided backing plan for explicit-bound backends, when one exists.
    pub default_explicit_backing: Option<super::ThreadExplicitBackingPlan>,
    /// Evidence sources used to justify the stack surface.
    pub authorities: ThreadAuthoritySet,
    /// Whether the stack support is native, emulated, or unavailable.
    pub implementation: ThreadImplementationKind,
}

impl ThreadStackSupport {
    /// Returns an explicitly unsupported stack surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: ThreadStackCaps::empty(),
            explicit_size: ThreadGuarantee::Unsupported,
            caller_provided: ThreadGuarantee::Unsupported,
            prefault: ThreadGuarantee::Unsupported,
            lock: ThreadGuarantee::Unsupported,
            locality: ThreadGuarantee::Unsupported,
            usage_observation: ThreadGuarantee::Unsupported,
            default_explicit_backing: None,
            authorities: ThreadAuthoritySet::empty(),
            implementation: ThreadImplementationKind::Unsupported,
        }
    }
}

/// Locality observation and first-touch support offered by a thread backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadLocalitySupport {
    /// Fine-grained locality capability flags.
    pub caps: ThreadLocalityCaps,
    /// Strength of first-touch control guarantees.
    pub first_touch: ThreadGuarantee,
    /// Strength of current execution-location observation.
    pub location_observation: ThreadGuarantee,
    /// Strength of inherited memory-policy control.
    pub memory_policy: ThreadGuarantee,
    /// Evidence sources used to justify the locality surface.
    pub authorities: ThreadAuthoritySet,
    /// Whether the locality support is native, emulated, or unavailable.
    pub implementation: ThreadImplementationKind,
}

impl ThreadLocalitySupport {
    /// Returns an explicitly unsupported locality surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: ThreadLocalityCaps::empty(),
            first_touch: ThreadGuarantee::Unsupported,
            location_observation: ThreadGuarantee::Unsupported,
            memory_policy: ThreadGuarantee::Unsupported,
            authorities: ThreadAuthoritySet::empty(),
            implementation: ThreadImplementationKind::Unsupported,
        }
    }
}

/// Aggregated thread support surface for a backend after authority-aware capability collapse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadSupport {
    /// Lifecycle support.
    pub lifecycle: ThreadLifecycleSupport,
    /// Placement and migration support.
    pub placement: ThreadPlacementSupport,
    /// Scheduler support.
    pub scheduler: ThreadSchedulerSupport,
    /// Stack and startup-memory support.
    pub stack: ThreadStackSupport,
    /// Locality observation and first-touch support.
    pub locality: ThreadLocalitySupport,
}

impl ThreadSupport {
    /// Returns a backend with no supported thread surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            lifecycle: ThreadLifecycleSupport::unsupported(),
            placement: ThreadPlacementSupport::unsupported(),
            scheduler: ThreadSchedulerSupport::unsupported(),
            stack: ThreadStackSupport::unsupported(),
            locality: ThreadLocalitySupport::unsupported(),
        }
    }
}
