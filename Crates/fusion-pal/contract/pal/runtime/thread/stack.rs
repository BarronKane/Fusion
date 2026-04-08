use core::num::NonZeroUsize;
use core::ptr::NonNull;

use crate::contract::pal::HardwareTopologyNodeId;

/// Backing strategy for a thread stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadStackBacking {
    /// Use backend-default stack allocation.
    Default,
    /// Use caller-provided stack memory.
    CallerProvided {
        /// Base of the caller-provided stack region.
        base: NonNull<u8>,
        /// Length of the caller-provided stack region.
        len: NonZeroUsize,
    },
}

/// Prefault policy for startup stack instantiation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadStackPrefaultPolicy {
    /// Inherit the backend or process default.
    Inherit,
    /// Do not prefault stack pages proactively.
    Disabled,
    /// Allow the creating thread to prefault stack backing before entry.
    Creator,
    /// Require the target thread to prefault stack backing before user entry.
    Target,
}

/// Lock or pin policy for thread stack backing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadStackLockPolicy {
    /// Inherit the backend or process default.
    Inherit,
    /// Do not request locked or pinned stack backing.
    Disabled,
    /// Prefer locked or pinned stack backing when available.
    Preferred,
    /// Require locked or pinned stack backing or fail.
    Required,
}

/// Locality policy for thread stack backing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadStackLocalityPolicy {
    /// Follow the backend or process default.
    InheritProcessPolicy,
    /// Follow the thread placement policy when the backend can couple them honestly.
    FollowThreadPlacement,
    /// Prefer stack backing local to the given NUMA topology node.
    PreferredNumaNode(HardwareTopologyNodeId),
    /// Require stack backing local to the given NUMA topology node.
    RequiredNumaNode(HardwareTopologyNodeId),
}

/// Canonical caller-provided stack-backing plan for explicit-bound thread backends.
///
/// This is the honest bridge for platforms that cannot materialize a thread stack on demand and
/// instead require higher layers to provide one explicit backing region. When surfaced, this plan
/// describes the backend's default requested region shape rather than inviting runtimes to invent
/// their own folklore bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadExplicitBackingPlan {
    /// Default usable stack size in bytes.
    pub size_bytes: NonZeroUsize,
    /// Required base-alignment for the backing region.
    pub align_bytes: NonZeroUsize,
}

/// Requested stack and startup-memory policy for a thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadStackRequest {
    /// Requested usable stack size in bytes, if explicit sizing is desired.
    pub size_bytes: Option<NonZeroUsize>,
    /// Requested guard size in bytes, if explicit guard sizing is desired.
    pub guard_bytes: Option<usize>,
    /// Backing strategy for the stack.
    pub backing: ThreadStackBacking,
    /// Prefault policy for startup stack instantiation.
    pub prefault: ThreadStackPrefaultPolicy,
    /// Lock or pin policy for stack backing.
    pub lock: ThreadStackLockPolicy,
    /// Locality policy for stack backing.
    pub locality: ThreadStackLocalityPolicy,
}

impl ThreadStackRequest {
    /// Returns a stack request that inherits backend defaults.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            size_bytes: None,
            guard_bytes: None,
            backing: ThreadStackBacking::Default,
            prefault: ThreadStackPrefaultPolicy::Inherit,
            lock: ThreadStackLockPolicy::Inherit,
            locality: ThreadStackLocalityPolicy::InheritProcessPolicy,
        }
    }
}

impl Default for ThreadStackRequest {
    fn default() -> Self {
        Self::new()
    }
}

/// Observable stack-usage information for a thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadStackObservation {
    /// Configured usable stack size, when known.
    pub configured_bytes: Option<NonZeroUsize>,
    /// Effective guard size, when known.
    pub guard_bytes: Option<usize>,
    /// Greatest observed stack usage or touched depth, when the backend can surface it.
    pub high_water_bytes: Option<usize>,
    /// Current observed stack usage, when the backend can surface it honestly.
    pub current_bytes: Option<usize>,
    /// Whether the backend detected a stack overflow condition.
    pub overflow_detected: Option<bool>,
}

impl ThreadStackObservation {
    /// Returns an empty stack observation.
    #[must_use]
    pub const fn unknown() -> Self {
        Self {
            configured_bytes: None,
            guard_bytes: None,
            high_water_bytes: None,
            current_bytes: None,
            overflow_detected: None,
        }
    }
}
