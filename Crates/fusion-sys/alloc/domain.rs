use crate::mem::resource::{
    AllocatorLayoutPolicy,
    MemoryDomain,
    MemoryDomainSet,
    ResourceAttrs,
    ResourceHazardSet,
};
use super::{
    AllocPolicy,
    MemoryPoolStats,
};

/// Stable identifier for one allocator-owned domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AllocatorDomainId(pub u16);

/// Origin of one allocator-owned domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AllocatorDomainKind {
    /// Implicit default domain formed by the allocator builder.
    Default,
    /// Explicit domain added by the caller.
    Explicit,
}

/// Observable summary of one allocator-owned domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AllocatorDomainInfo {
    /// Stable domain identifier.
    pub id: AllocatorDomainId,
    /// Whether the domain was implicit or explicit.
    pub kind: AllocatorDomainKind,
    /// Policy enforced for this domain.
    pub policy: AllocPolicy,
    /// Number of owned resources assigned to the domain.
    pub resource_count: usize,
    /// Domains represented by the assigned resources.
    pub memory_domains: MemoryDomainSet,
    /// Aggregate intrinsic resource attributes.
    pub attrs: ResourceAttrs,
    /// Aggregate inherent hazards across assigned resources.
    pub hazards: ResourceHazardSet,
}

/// Operationally auditable snapshot of one allocator-owned domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AllocatorDomainAudit {
    /// Stable descriptive domain info.
    pub info: AllocatorDomainInfo,
    /// Primary allocator-facing layout policy derived from the domain's owned resources when one
    /// exists.
    pub primary_layout_policy: Option<AllocatorLayoutPolicy>,
    /// Current pool stats when the domain owns a realized pool.
    pub pool_stats: Option<MemoryPoolStats>,
}

impl AllocatorDomainInfo {
    pub(super) const fn new(
        id: AllocatorDomainId,
        kind: AllocatorDomainKind,
        policy: AllocPolicy,
    ) -> Self {
        Self {
            id,
            kind,
            policy,
            resource_count: 0,
            memory_domains: MemoryDomainSet::empty(),
            attrs: ResourceAttrs::empty(),
            hazards: ResourceHazardSet::empty(),
        }
    }

    pub(super) fn note_resource(
        &mut self,
        domain: MemoryDomain,
        attrs: ResourceAttrs,
        hazards: ResourceHazardSet,
    ) {
        self.resource_count += 1;
        self.memory_domains |= memory_domain_set(domain);
        self.attrs |= attrs;
        self.hazards |= hazards;
    }
}

pub(super) const fn memory_domain_set(domain: MemoryDomain) -> MemoryDomainSet {
    match domain {
        MemoryDomain::VirtualAddressSpace => MemoryDomainSet::VIRTUAL_ADDRESS_SPACE,
        MemoryDomain::DeviceLocal => MemoryDomainSet::DEVICE_LOCAL,
        MemoryDomain::Physical => MemoryDomainSet::PHYSICAL,
        MemoryDomain::StaticRegion => MemoryDomainSet::STATIC_REGION,
        MemoryDomain::Mmio => MemoryDomainSet::MMIO,
    }
}
