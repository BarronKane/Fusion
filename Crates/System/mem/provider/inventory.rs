use core::num::NonZeroUsize;

use crate::mem::provider::{CriticalSafetyRequirements, MemoryTopologyNodeId};
use crate::mem::resource::{
    MemoryDomain, MemoryGeometry, ResourceAcquireSupport, ResourceAttrs, ResourceBackingKind,
    ResourceContract, ResourceHazardSet, ResourceInfo, ResourceState, ResourceSupport,
};

/// Stable identifier for a provider-known resource record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryResourceId(pub u32);

/// Stable identifier for a provider-known acquisition strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryStrategyId(pub u32);

/// Stable identifier for a provider-defined pool compatibility class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryPoolClassId(pub u32);

/// Borrowed provider inventory view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryProviderInventory<'a> {
    /// Concrete resources already known to the provider.
    pub resources: &'a [MemoryResourceDescriptor],
    /// Acquisition or binding strategies the provider may use later.
    pub strategies: &'a [MemoryStrategyDescriptor],
    /// Provider-defined classes of resources that are interchangeable for a pool.
    pub pool_classes: &'a [MemoryPoolClass],
}

/// Provenance class for a provider-known concrete resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryResourceOrigin {
    /// Resource already existed as a discovered or bound range.
    Discovered,
    /// Resource was actively created by the provider or platform.
    Created,
    /// Resource is borrowed from an external owner with provider-level bookkeeping.
    Borrowed,
    /// Resource was materialized from a reservation or similar placeholder.
    Materialized,
}

/// Coarse acquisition story for a provider strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryStrategyKind {
    /// Hosted virtual-memory creation path such as anonymous or file-backed mappings.
    VirtualCreate,
    /// Bind an externally governed static, physical, or board-defined range.
    BindExisting,
    /// Materialize from a reserved address window.
    ReservationMaterialize,
    /// Map or bind direct physical memory.
    PhysicalMap,
    /// Map or bind device-local or MMIO-style memory.
    DeviceMap,
    /// Use a backend-native pool or partition source.
    NativePool,
}

/// Capacity description for a provider acquisition strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryStrategyCapacity {
    /// Smallest request size the strategy is willing to serve.
    pub min_len: usize,
    /// Largest request size the strategy can serve when known.
    pub max_len: Option<usize>,
    /// Allocation or acquisition granule used by the strategy.
    pub granule: NonZeroUsize,
}

/// Pool-visible envelope of properties that must align across compatible resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryCompatibilityEnvelope {
    /// Memory domain shared by compatible resources.
    pub domain: MemoryDomain,
    /// Backing kind shared by compatible resources.
    pub backing: ResourceBackingKind,
    /// Intrinsic attributes shared by compatible resources.
    pub attrs: ResourceAttrs,
    /// Operation granularity shared by compatible resources.
    pub geometry: MemoryGeometry,
    /// Immutable contract shared by compatible resources.
    pub contract: ResourceContract,
    /// Runtime support surface shared by compatible resources.
    pub support: ResourceSupport,
    /// Hazards shared by compatible resources.
    pub hazards: ResourceHazardSet,
}

impl MemoryCompatibilityEnvelope {
    /// Extracts the pool-visible compatibility envelope from a resource info record.
    #[must_use]
    pub const fn from_resource_info(info: ResourceInfo) -> Self {
        Self {
            domain: info.domain,
            backing: info.backing,
            attrs: info.attrs,
            geometry: info.geometry,
            contract: info.contract,
            support: info.support,
            hazards: info.hazards,
        }
    }

    /// Returns `true` when `resource` carries the same pool-visible semantics.
    #[must_use]
    pub fn matches_resource(self, resource: &MemoryResourceDescriptor) -> bool {
        self == resource.compatibility()
    }
}

/// Provider-known descriptor for a concrete resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryResourceDescriptor {
    /// Stable provider-local resource identifier.
    pub id: MemoryResourceId,
    /// Immutable resource description.
    pub info: ResourceInfo,
    /// Current provider-visible summary state.
    pub state: ResourceState,
    /// Source story for the resource.
    pub origin: MemoryResourceOrigin,
    /// Bytes the provider considers pool-usable from this resource.
    pub usable_len: usize,
    /// Optional topology node associated with the resource.
    pub topology_node: Option<MemoryTopologyNodeId>,
    /// Optional precomputed pool class for this resource.
    pub pool_class: Option<MemoryPoolClassId>,
}

impl MemoryResourceDescriptor {
    /// Returns the pool-visible compatibility envelope for this resource.
    #[must_use]
    pub const fn compatibility(self) -> MemoryCompatibilityEnvelope {
        MemoryCompatibilityEnvelope::from_resource_info(self.info)
    }
}

/// Provider-known descriptor for a way to create or materialize more resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryStrategyDescriptor {
    /// Stable provider-local strategy identifier.
    pub id: MemoryStrategyId,
    /// Coarse acquisition story represented by the strategy.
    pub kind: MemoryStrategyKind,
    /// Domain and runtime support surface exposed by the strategy.
    pub acquire: ResourceAcquireSupport,
    /// Capacity limits or granularity for the strategy.
    pub capacity: MemoryStrategyCapacity,
    /// Safety properties the provider expects the strategy to satisfy when acquisition
    /// succeeds without degrading the request.
    pub assurance: CriticalSafetyRequirements,
    /// Optional topology node naturally associated with the strategy.
    pub topology_node: Option<MemoryTopologyNodeId>,
    /// Optional default pool class this strategy is intended to satisfy.
    pub pool_class: Option<MemoryPoolClassId>,
}

/// Provider-defined class of resources that are safe to pool together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryPoolClass {
    /// Stable provider-local class identifier.
    pub id: MemoryPoolClassId,
    /// Shared pool-visible envelope for members of the class.
    pub envelope: MemoryCompatibilityEnvelope,
    /// Safety properties the provider claims every member of the class satisfies.
    pub assurance: CriticalSafetyRequirements,
    /// Optional topology node that characterizes the class.
    pub topology_node: Option<MemoryTopologyNodeId>,
}

impl MemoryPoolClass {
    /// Returns `true` when `resource` belongs in this compatibility class.
    #[must_use]
    pub fn accepts(self, resource: &MemoryResourceDescriptor) -> bool {
        resource.pool_class == Some(self.id) && self.envelope.matches_resource(resource)
    }
}
