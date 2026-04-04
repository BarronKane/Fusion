use fusion_pal::sys::mem::Region;

use crate::mem::resource::{
    AllocatorLayoutPolicy,
    MemoryDomain,
    ResourceAttrs,
    ResourceBackingKind,
    ResourceContract,
    ResourceHazardSet,
    ResourceInfo,
    ResourceSupport,
};
use super::MemoryTopologyNodeId;

/// Stable identifier for a provider-known memory object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryObjectId(pub u32);

/// Provenance class for a provider-known memory object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryObjectOrigin {
    /// Object already existed as a discovered or bound range.
    Discovered,
    /// Object was actively created by the provider or surrounding platform.
    Created,
    /// Object is borrowed from an external owner with provider-level bookkeeping.
    Borrowed,
    /// Object was materialized from a reservation or similar placeholder.
    Materialized,
}

/// Pool-visible semantics of a provider-known memory object independent of CPU addressability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryObjectEnvelope {
    /// Memory domain classification for the object.
    pub domain: MemoryDomain,
    /// Concrete backing kind for the object.
    pub backing: ResourceBackingKind,
    /// Intrinsic attributes of the object.
    pub attrs: ResourceAttrs,
    /// Allocator-facing metadata and extent layout policy.
    pub layout: AllocatorLayoutPolicy,
    /// Immutable lifetime contract of the object.
    pub contract: ResourceContract,
    /// Runtime support surface of the object.
    pub support: ResourceSupport,
    /// Inherent hazards of the object.
    pub hazards: ResourceHazardSet,
}

impl MemoryObjectEnvelope {
    /// Extracts the object envelope from a live resource info record.
    #[must_use]
    pub const fn from_resource_info(info: ResourceInfo) -> Self {
        Self {
            domain: info.domain,
            backing: info.backing,
            attrs: info.attrs,
            layout: info.layout,
            contract: info.contract,
            support: info.support,
            hazards: info.hazards,
        }
    }
}

/// Provider-known descriptor for any memory object, including ones that are not currently
/// CPU-addressable or directly pool-eligible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryObjectDescriptor {
    /// Stable provider-local object identifier.
    pub id: MemoryObjectId,
    /// Pool-visible semantics of the object.
    pub envelope: MemoryObjectEnvelope,
    /// CPU-addressable range when one exists in the current execution context.
    pub cpu_range: Option<Region>,
    /// Object provenance for diagnostics and policy.
    pub origin: MemoryObjectOrigin,
    /// Bytes the provider considers meaningful or usable for this object.
    pub usable_len: usize,
    /// Optional topology node associated with the object.
    pub topology_node: Option<MemoryTopologyNodeId>,
}

impl MemoryObjectDescriptor {
    /// Returns `true` when the object has a CPU-visible range in the current execution
    /// context.
    #[must_use]
    pub const fn is_cpu_addressable(self) -> bool {
        self.cpu_range.is_some()
    }
}

/// Current readiness of a provider-known pool resource descriptor.
///
/// Readiness is per descriptor, not per backing object and not per arbitrary subrange. If
/// one larger object has mixed readiness across disjoint regions, the provider is expected
/// to surface those regions as multiple descriptors rather than collapsing them into one
/// partially ready record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryResourceReadiness {
    /// The resource is immediately usable for pooling in its current state.
    ReadyNow,
    /// The resource exists but requires commit-like backing activation first.
    RequiresCommit,
    /// The resource exists descriptively but must be materialized into usable backing.
    RequiresMaterialization,
    /// The resource exists but requires some other legal state transition first.
    RequiresStateTransition,
    /// The provider cannot presently make this resource pool-usable.
    Unavailable,
}

impl MemoryResourceReadiness {
    /// Returns `true` when the resource is immediately usable for pooling.
    #[must_use]
    pub const fn is_ready_now(self) -> bool {
        matches!(self, Self::ReadyNow)
    }

    /// Returns `true` when the resource can become pool-usable without discovering a new
    /// backing object.
    #[must_use]
    pub const fn is_present_transitionable(self) -> bool {
        matches!(
            self,
            Self::ReadyNow
                | Self::RequiresCommit
                | Self::RequiresMaterialization
                | Self::RequiresStateTransition
        )
    }
}
