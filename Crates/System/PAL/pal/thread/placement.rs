use crate::pal::mem::MemTopologyNodeId;

use super::{
    ThreadClusterId, ThreadCoreClassId, ThreadCoreId, ThreadGuarantee, ThreadLogicalCpuId,
};

/// Constraint strength requested for thread placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadConstraintMode {
    /// The requested placement is preferred and may degrade honestly.
    Prefer,
    /// The requested placement is required and failure should be explicit.
    Require,
}

/// Migration policy requested for a thread after startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadMigrationPolicy {
    /// Inherit the platform or process default.
    Inherit,
    /// Allow migration freely after startup.
    Allow,
    /// Prefer to avoid migration but tolerate honest degradation.
    Avoid,
    /// Require non-migration if the backend can guarantee it honestly.
    Disallow,
}

/// Phase at which placement must be applied relative to thread entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadPlacementPhase {
    /// Use the platform default placement timing.
    Inherit,
    /// Placement may be applied after the thread starts.
    PostStartAllowed,
    /// Placement should be applied before user entry when possible.
    PreStartPreferred,
    /// Placement must be applied before user entry or thread creation should fail.
    PreStartRequired,
}

/// Requested placement policy for a thread.
///
/// The identifiers supplied here are expected to come from a sibling topology authority
/// surface. Callers should not invent logical CPU, package, or NUMA-node identifiers
/// locally and hope the backend finds them charming. Placement requests are only as
/// truthful as the topology model they are built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadPlacementRequest<'a> {
    /// Requested logical CPU targets, if any.
    pub logical_cpus: &'a [ThreadLogicalCpuId],
    /// Requested package or socket topology nodes, if any.
    pub packages: &'a [MemTopologyNodeId],
    /// Requested NUMA topology nodes, if any.
    pub numa_nodes: &'a [MemTopologyNodeId],
    /// Requested heterogeneous core classes, if any.
    pub core_classes: &'a [ThreadCoreClassId],
    /// Strength of the placement request.
    pub mode: ThreadConstraintMode,
    /// When placement must be applied relative to user entry.
    pub phase: ThreadPlacementPhase,
    /// Requested migration policy after startup.
    pub migration: ThreadMigrationPolicy,
}

impl ThreadPlacementRequest<'_> {
    /// Returns an empty placement request that inherits platform defaults.
    ///
    /// Any non-empty request should be populated from topology identifiers discovered via
    /// the sibling hardware or topology authority for the current machine.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            logical_cpus: &[],
            packages: &[],
            numa_nodes: &[],
            core_classes: &[],
            mode: ThreadConstraintMode::Prefer,
            phase: ThreadPlacementPhase::Inherit,
            migration: ThreadMigrationPolicy::Inherit,
        }
    }
}

impl Default for ThreadPlacementRequest<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// Current or effective execution location of a thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadExecutionLocation {
    /// Current logical CPU, when observable.
    pub logical_cpu: Option<ThreadLogicalCpuId>,
    /// Current physical or topology-defined core, when observable.
    pub core: Option<ThreadCoreId>,
    /// Current core cluster or LLC domain, when observable.
    pub cluster: Option<ThreadClusterId>,
    /// Current package or socket topology node, when observable.
    pub package: Option<MemTopologyNodeId>,
    /// Current NUMA topology node, when observable.
    pub numa_node: Option<MemTopologyNodeId>,
    /// Current heterogeneous core class, when observable.
    pub core_class: Option<ThreadCoreClassId>,
}

impl ThreadExecutionLocation {
    /// Returns an empty execution location with no observable placement information.
    #[must_use]
    pub const fn unknown() -> Self {
        Self {
            logical_cpu: None,
            core: None,
            cluster: None,
            package: None,
            numa_node: None,
            core_class: None,
        }
    }
}

/// Effective placement outcome surfaced by a backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadPlacementOutcome {
    /// Strength of the effective placement guarantee.
    pub guarantee: ThreadGuarantee,
    /// Phase at which the placement was or will be applied.
    pub phase: ThreadPlacementPhase,
    /// Effective execution location, when observable.
    pub location: ThreadExecutionLocation,
}

impl ThreadPlacementOutcome {
    /// Returns an explicitly unsupported placement outcome.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            guarantee: ThreadGuarantee::Unsupported,
            phase: ThreadPlacementPhase::Inherit,
            location: ThreadExecutionLocation::unknown(),
        }
    }
}
