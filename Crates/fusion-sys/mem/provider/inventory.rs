use core::num::NonZeroUsize;

use crate::mem::provider::{
    CriticalSafetyRequirements,
    MemoryObjectEnvelope,
    MemoryObjectId,
    MemoryResourceReadiness,
    MemoryTopologyNodeId,
};
use crate::mem::resource::{
    AllocatorLayoutPolicy,
    MemoryDomain,
    MemoryGeometry,
    ResourceAcquireSupport,
    ResourceBackingKind,
    ResourceHazardSet,
    ResourceInfo,
    ResourceState,
    ResourceSupport,
};

/// Stable identifier for a provider-known pool-capable resource record.
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
    /// All provider-known memory objects, including ones that are not currently
    /// CPU-addressable or pool-eligible.
    pub objects: &'a [super::MemoryObjectDescriptor],
    /// Concrete CPU-addressable resources the provider considers pool candidates.
    pub resources: &'a [MemoryResourceDescriptor],
    /// Acquisition or binding strategies the provider may use later.
    pub strategies: &'a [MemoryStrategyDescriptor],
    /// Provider-defined classes of resources that are interchangeable for a pool.
    pub pool_classes: &'a [MemoryPoolClass],
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
    pub attrs: crate::mem::resource::ResourceAttrs,
    /// Operation granularity shared by compatible resources.
    pub geometry: MemoryGeometry,
    /// Allocator-facing metadata and extent layout policy shared by compatible resources.
    pub layout: AllocatorLayoutPolicy,
    /// Immutable contract shared by compatible resources.
    pub contract: crate::mem::resource::ResourceContract,
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
            layout: info.layout,
            contract: info.contract,
            support: info.support,
            hazards: info.hazards,
        }
    }

    /// Returns the broader object envelope for this compatibility record.
    #[must_use]
    pub const fn object_envelope(self) -> MemoryObjectEnvelope {
        MemoryObjectEnvelope {
            domain: self.domain,
            backing: self.backing,
            attrs: self.attrs,
            layout: self.layout,
            contract: self.contract,
            support: self.support,
            hazards: self.hazards,
        }
    }

    /// Returns `true` when `resource` carries the same pool-visible semantics.
    #[must_use]
    pub fn matches_resource(self, resource: &MemoryResourceDescriptor) -> bool {
        self == resource.compatibility()
    }
}

/// Provider-known descriptor for a concrete CPU-addressable pool resource.
///
/// This descriptor is intentionally uniform from the planner's point of view. If one
/// discovered or bound object has mixed pool readiness across subranges, the provider must
/// normalize that object into multiple `MemoryResourceDescriptor` records. A descriptor
/// that is partly ready and partly preparation-required will otherwise lose capacity during
/// planning, because pool planning consumes ready descriptors and preparation-required
/// descriptors in separate phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryResourceDescriptor {
    /// Stable provider-local resource identifier.
    pub id: MemoryResourceId,
    /// Related broad memory-object identifier when one exists.
    pub object_id: Option<MemoryObjectId>,
    /// Immutable resource description.
    pub info: ResourceInfo,
    /// Current provider-visible summary state.
    pub state: ResourceState,
    /// Source story for the resource.
    pub origin: super::MemoryObjectOrigin,
    /// Bytes immediately pool-usable from this descriptor without more transitions.
    pub usable_now_len: usize,
    /// Maximum bytes the provider expects could become pool-usable from this descriptor
    /// after legal preparation.
    pub usable_max_len: usize,
    /// Current readiness classification for pool use.
    pub readiness: MemoryResourceReadiness,
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

    /// Returns the broader object envelope for this resource.
    #[must_use]
    pub const fn object_envelope(self) -> MemoryObjectEnvelope {
        self.compatibility().object_envelope()
    }

    /// Returns `true` when the resource is immediately usable for pooling.
    #[must_use]
    pub const fn is_ready_now(self) -> bool {
        self.readiness.is_ready_now() && self.usable_now_len != 0
    }

    /// Returns `true` when the resource can become pool-usable without creating a new
    /// backing object.
    #[must_use]
    pub const fn is_present_transitionable(self) -> bool {
        self.readiness.is_present_transitionable() && self.usable_max_len != 0
    }
}

/// Pool-relevant output envelope of an acquisition strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryStrategyOutputDescriptor {
    /// Pool-visible envelope the strategy can create when used in this mode.
    pub envelope: MemoryCompatibilityEnvelope,
    /// Readiness expected on the created or materialized resource before it is pool-usable.
    pub readiness: MemoryResourceReadiness,
    /// Safety properties the provider expects this output mode to satisfy.
    pub assurance: CriticalSafetyRequirements,
    /// Optional topology node naturally associated with the output.
    pub topology_node: Option<MemoryTopologyNodeId>,
    /// Optional default pool class this output is intended to satisfy.
    pub pool_class: Option<MemoryPoolClassId>,
}

/// Provider-known descriptor for a way to create or materialize more resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryStrategyDescriptor {
    /// Stable provider-local strategy identifier.
    pub id: MemoryStrategyId,
    /// Coarse acquisition story represented by the strategy.
    pub kind: MemoryStrategyKind,
    /// Acquisition controls and support exposed by the strategy.
    pub acquire: ResourceAcquireSupport,
    /// Capacity limits or granularity for the strategy.
    pub capacity: MemoryStrategyCapacity,
    /// Pool-relevant output envelope when the strategy can produce a pool-capable resource.
    pub output: Option<MemoryStrategyOutputDescriptor>,
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

    /// Returns `true` when `strategy` naturally produces this compatibility class.
    #[must_use]
    pub fn accepts_strategy(self, strategy: &MemoryStrategyDescriptor) -> bool {
        strategy.output.is_some_and(|output| {
            output.pool_class == Some(self.id)
                && output.envelope == self.envelope
                && output.topology_node == self.topology_node
        })
    }
}
