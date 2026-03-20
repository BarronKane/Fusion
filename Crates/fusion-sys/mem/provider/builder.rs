use super::{
    MemoryObjectDescriptor,
    MemoryPoolClass,
    MemoryResourceDescriptor,
    MemoryStrategyDescriptor,
    MemoryTopologyLink,
    MemoryTopologyNode,
};

/// Policy for how a provider should obtain its initial inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryProviderDiscoveryPolicy {
    /// Discover inventory only from `fusion_pal::sys::mem`.
    PalOnly,
    /// Use only explicit overlays supplied by the caller.
    ExplicitOnly,
    /// Merge fusion-pal discovery with caller-supplied overlays.
    MergePalWithExplicit,
}

/// Policy for resolving conflicts between discovered and explicit inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryProviderConflictPolicy {
    /// Reject conflicting descriptors rather than silently preferring one.
    Reject,
    /// Prefer explicit overlays supplied by the caller.
    PreferExplicit,
    /// Prefer descriptors discovered through the fusion-pal.
    PreferDiscovered,
}

/// Borrowed composition spec for constructing a provider instance.
///
/// This is the planned builder payload for a future concrete provider implementation. The
/// default shape is "discover from fusion-pal, then merge explicit overlays if the caller supplied
/// them." That lets a hosted target use only normalized fusion-pal discovery, while a DMA-heavy
/// board or firmware environment can inject additional bound resources or topology that the
/// fusion-pal cannot discover automatically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryProviderBuildSpec<'a> {
    /// How the provider obtains discovered inventory.
    pub discovery: MemoryProviderDiscoveryPolicy,
    /// How conflicts between discovered and explicit descriptors should be resolved.
    pub conflict_policy: MemoryProviderConflictPolicy,
    /// Explicit memory objects to merge into the provider inventory.
    pub explicit_objects: &'a [MemoryObjectDescriptor],
    /// Explicit concrete resources to merge into the provider inventory.
    pub explicit_resources: &'a [MemoryResourceDescriptor],
    /// Explicit acquisition strategies to merge into the provider inventory.
    pub explicit_strategies: &'a [MemoryStrategyDescriptor],
    /// Explicit precomputed pool classes to merge into the provider inventory.
    pub explicit_pool_classes: &'a [MemoryPoolClass],
    /// Explicit topology nodes to merge into the provider topology view.
    pub explicit_topology_nodes: &'a [MemoryTopologyNode],
    /// Explicit topology links to merge into the provider topology view.
    pub explicit_topology_links: &'a [MemoryTopologyLink],
}

impl MemoryProviderBuildSpec<'_> {
    /// Returns the default composition policy for a system provider.
    #[must_use]
    pub const fn system() -> Self {
        Self {
            discovery: MemoryProviderDiscoveryPolicy::MergePalWithExplicit,
            conflict_policy: MemoryProviderConflictPolicy::Reject,
            explicit_objects: &[],
            explicit_resources: &[],
            explicit_strategies: &[],
            explicit_pool_classes: &[],
            explicit_topology_nodes: &[],
            explicit_topology_links: &[],
        }
    }

    /// Returns an explicit-only composition policy with no fusion-pal discovery.
    #[must_use]
    pub const fn explicit_only() -> Self {
        Self {
            discovery: MemoryProviderDiscoveryPolicy::ExplicitOnly,
            ..Self::system()
        }
    }
}
