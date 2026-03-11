use fusion_pal::sys::mem::{CachePolicy, Protect};

use super::{
    CriticalSafetyRequirements, MemoryPoolClass, MemoryResourceDescriptor,
    MemoryStrategyDescriptor, MemoryTopologyPreference,
};
use crate::mem::provider::support::{matches_node_requirement, matches_safety};
use crate::mem::resource::{
    IntegrityConstraints, MemoryDomain, MemoryDomainSet, OvercommitPolicy, ResourceAttrs,
    ResourceFeatureSupport, ResourceHazardSet, ResourceOpSet, ResourceResidencySupport,
    SharingPolicy,
};

/// Contract filters that a pool request may require from all candidate resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryPoolContractRequirements {
    /// Maximum protection set that must be admitted by the resource contract.
    pub required_allowed_protect: Protect,
    /// Whether the pool requires write-xor-execute discipline.
    pub require_write_xor_execute: bool,
    /// Required sharing policy when one is imposed.
    pub required_sharing: Option<SharingPolicy>,
    /// Required overcommit policy when one is imposed.
    pub required_overcommit: Option<OvercommitPolicy>,
    /// Required cache policy when one is imposed.
    pub required_cache_policy: Option<CachePolicy>,
    /// Required integrity regime when one is imposed.
    pub required_integrity: Option<IntegrityConstraints>,
}

impl MemoryPoolContractRequirements {
    /// Returns `true` when `contract` satisfies every hard filter in this request.
    #[must_use]
    pub fn matches(self, contract: crate::mem::resource::ResourceContract) -> bool {
        if !contract
            .allowed_protect
            .contains(self.required_allowed_protect)
        {
            return false;
        }

        if self.require_write_xor_execute && !contract.write_xor_execute {
            return false;
        }

        if let Some(sharing) = self.required_sharing
            && contract.sharing != sharing
        {
            return false;
        }

        if let Some(overcommit) = self.required_overcommit
            && contract.overcommit != overcommit
        {
            return false;
        }

        if let Some(cache_policy) = self.required_cache_policy
            && contract.cache_policy != cache_policy
        {
            return false;
        }

        if let Some(integrity) = self.required_integrity
            && contract.integrity != Some(integrity)
        {
            return false;
        }

        true
    }
}

/// Pool-facing request evaluated against provider inventory and strategies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryPoolRequest<'a> {
    /// Optional human-readable label for diagnostics or provider bookkeeping.
    pub name: Option<&'a str>,
    /// Minimum pool capacity required immediately.
    pub minimum_capacity: usize,
    /// Preferred target capacity when the provider can do better than the minimum.
    pub preferred_capacity: usize,
    /// Domains acceptable for the pool.
    pub required_domains: MemoryDomainSet,
    /// Intrinsic attributes every accepted resource must have.
    pub required_attrs: ResourceAttrs,
    /// Intrinsic attributes no accepted resource may have.
    pub forbidden_attrs: ResourceAttrs,
    /// Runtime operations every accepted resource must expose.
    pub required_ops: ResourceOpSet,
    /// Residency support every accepted resource or strategy must expose.
    pub required_residency: ResourceResidencySupport,
    /// Acquisition-time features required from any strategy used to grow the pool.
    pub required_features: ResourceFeatureSupport,
    /// Hazards that are forbidden for all accepted resources.
    pub forbidden_hazards: ResourceHazardSet,
    /// Safety-oriented requirements that should be enforced across the pool.
    pub required_safety: CriticalSafetyRequirements,
    /// Immutable contract filters the pool must enforce.
    pub contract: MemoryPoolContractRequirements,
    /// Topology preference or requirement for the pool.
    pub topology: MemoryTopologyPreference,
}

impl MemoryPoolRequest<'_> {
    /// Creates a conservative general-purpose request for allocator-usable private memory.
    #[must_use]
    pub fn general_purpose(minimum_capacity: usize) -> Self {
        Self {
            name: None,
            minimum_capacity,
            preferred_capacity: minimum_capacity,
            required_domains: MemoryDomainSet::all(),
            required_attrs: ResourceAttrs::ALLOCATABLE,
            forbidden_attrs: ResourceAttrs::HAZARDOUS_IO | ResourceAttrs::READ_ONLY_BACKING,
            required_ops: ResourceOpSet::empty(),
            required_residency: ResourceResidencySupport::BEST_EFFORT,
            required_features: ResourceFeatureSupport::empty(),
            forbidden_hazards: ResourceHazardSet::MMIO_SIDE_EFFECTS,
            required_safety: CriticalSafetyRequirements::POOLABLE,
            contract: MemoryPoolContractRequirements {
                required_allowed_protect: Protect::READ | Protect::WRITE,
                require_write_xor_execute: true,
                required_sharing: Some(SharingPolicy::Private),
                required_overcommit: None,
                required_cache_policy: None,
                required_integrity: None,
            },
            topology: MemoryTopologyPreference::Anywhere,
        }
    }

    /// Returns `true` when `resource` satisfies the hard filters in this request.
    #[must_use]
    pub fn matches_resource(self, resource: &MemoryResourceDescriptor) -> bool {
        let info = resource.info;

        if self.minimum_capacity > resource.usable_len {
            return false;
        }

        if !self.required_domains.contains(domain_to_set(info.domain)) {
            return false;
        }

        if !info.attrs.contains(self.required_attrs) || info.attrs.intersects(self.forbidden_attrs)
        {
            return false;
        }

        if !info.support.ops.contains(self.required_ops) {
            return false;
        }

        if !info.support.residency.contains(self.required_residency) {
            return false;
        }

        if info.hazards.intersects(self.forbidden_hazards) {
            return false;
        }

        if !self.contract.matches(info.contract) {
            return false;
        }

        if !matches_safety(resource, self.required_safety) {
            return false;
        }

        matches_node_requirement(self.topology, resource.topology_node)
    }

    /// Returns `true` when `class` describes a resource envelope compatible with this request.
    #[must_use]
    pub fn matches_pool_class(self, class: &MemoryPoolClass) -> bool {
        if !self
            .required_domains
            .contains(domain_to_set(class.envelope.domain))
        {
            return false;
        }

        if !class.envelope.attrs.contains(self.required_attrs)
            || class.envelope.attrs.intersects(self.forbidden_attrs)
        {
            return false;
        }

        if !class.envelope.support.ops.contains(self.required_ops) {
            return false;
        }

        if !class
            .envelope
            .support
            .residency
            .contains(self.required_residency)
        {
            return false;
        }

        if class.envelope.hazards.intersects(self.forbidden_hazards) {
            return false;
        }

        if !self.contract.matches(class.envelope.contract) {
            return false;
        }

        if !class.assurance.contains(self.required_safety) {
            return false;
        }

        matches_node_requirement(self.topology, class.topology_node)
    }

    /// Returns `true` when `strategy` can plausibly serve the hard filters in this request.
    ///
    /// This is intentionally coarse. Strategies are assessed by what they can produce and what
    /// acquisition controls they support, not by pretending every final resource property is
    /// already fixed before a concrete request is issued.
    #[must_use]
    pub fn matches_strategy(self, strategy: &MemoryStrategyDescriptor) -> bool {
        if self.minimum_capacity < strategy.capacity.min_len {
            return false;
        }

        if let Some(max_len) = strategy.capacity.max_len
            && self.minimum_capacity > max_len
        {
            return false;
        }

        if !strategy.acquire.domains.intersects(self.required_domains) {
            return false;
        }

        if !strategy.acquire.instance.ops.contains(self.required_ops) {
            return false;
        }

        if !strategy
            .acquire
            .instance
            .residency
            .contains(self.required_residency)
        {
            return false;
        }

        if !strategy.acquire.features.contains(self.required_features) {
            return false;
        }

        if !strategy.assurance.contains(self.required_safety) {
            return false;
        }

        matches_node_requirement(self.topology, strategy.topology_node)
    }
}

const fn domain_to_set(domain: MemoryDomain) -> MemoryDomainSet {
    match domain {
        MemoryDomain::VirtualAddressSpace => MemoryDomainSet::VIRTUAL_ADDRESS_SPACE,
        MemoryDomain::DeviceLocal => MemoryDomainSet::DEVICE_LOCAL,
        MemoryDomain::Physical => MemoryDomainSet::PHYSICAL,
        MemoryDomain::StaticRegion => MemoryDomainSet::STATIC_REGION,
        MemoryDomain::Mmio => MemoryDomainSet::MMIO,
    }
}
