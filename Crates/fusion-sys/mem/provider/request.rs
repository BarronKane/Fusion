use fusion_pal::sys::mem::{CachePolicy, Protect};

use super::{
    CriticalSafetyRequirements,
    MemoryObjectDescriptor,
    MemoryObjectEnvelope,
    MemoryPoolClass,
    MemoryResourceDescriptor,
    MemoryStrategyDescriptor,
    MemoryTopologyPreference,
};
use crate::mem::provider::support::{matches_node_requirement, matches_safety_envelope};
use crate::mem::resource::{
    IntegrityConstraints,
    MemoryDomain,
    MemoryDomainSet,
    OvercommitPolicy,
    ResourceAttrs,
    ResourceFeatureSupport,
    ResourceHazardSet,
    ResourceOpSet,
    ResourceResidencySupport,
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
    ///
    /// The default domain set intentionally excludes MMIO and opaque device-local memory so
    /// "general purpose" does not quietly widen into side-effecting or non-CPU-addressable
    /// space if other safety filters are relaxed later.
    #[must_use]
    pub fn general_purpose(minimum_capacity: usize) -> Self {
        Self {
            name: None,
            minimum_capacity,
            preferred_capacity: minimum_capacity,
            required_domains: MemoryDomainSet::VIRTUAL_ADDRESS_SPACE
                | MemoryDomainSet::PHYSICAL
                | MemoryDomainSet::STATIC_REGION,
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

    /// Returns `true` when the object falls within the request's broad compatibility
    /// envelope, independent of whether it is currently CPU-addressable or pool-ready.
    #[must_use]
    pub fn matches_object(self, object: &MemoryObjectDescriptor) -> bool {
        self.matches_object_base(object)
            && self.contract.matches(object.envelope.contract)
            && matches_safety_envelope(object.envelope, self.required_safety)
    }

    /// Returns `true` when `object` satisfies the broad request shape before contract and
    /// safety filtering.
    #[must_use]
    pub(super) fn matches_object_base(self, object: &MemoryObjectDescriptor) -> bool {
        self.matches_object_envelope(object.envelope, object.topology_node)
    }

    /// Returns `true` when `resource` satisfies the broad compatibility envelope in this
    /// request, ignoring current readiness.
    #[must_use]
    pub fn matches_resource(self, resource: &MemoryResourceDescriptor) -> bool {
        self.matches_resource_envelope(resource) && self.matches_resource_support(resource)
    }

    /// Returns `true` when `resource` satisfies the request's static compatibility
    /// envelope, ignoring current readiness and runtime operation support.
    #[must_use]
    pub fn matches_resource_envelope(self, resource: &MemoryResourceDescriptor) -> bool {
        self.matches_resource_base(resource)
            && self.contract.matches(resource.info.contract)
            && matches_safety_envelope(resource.object_envelope(), self.required_safety)
    }

    /// Returns `true` when `resource` satisfies the broad request shape before contract,
    /// safety, and support filtering.
    #[must_use]
    pub(super) fn matches_resource_base(self, resource: &MemoryResourceDescriptor) -> bool {
        self.matches_object_envelope(resource.object_envelope(), resource.topology_node)
    }

    /// Returns `true` when `resource` is immediately usable for pooling in this request.
    #[must_use]
    pub fn matches_resource_ready_now(self, resource: &MemoryResourceDescriptor) -> bool {
        self.matches_resource_envelope(resource)
            && self.matches_resource_support(resource)
            && resource.is_ready_now()
    }

    /// Returns `true` when `resource` can become pool-usable without discovering a new
    /// backing object.
    #[must_use]
    pub fn matches_resource_transitionable(self, resource: &MemoryResourceDescriptor) -> bool {
        self.matches_resource_envelope(resource)
            && self.matches_resource_support(resource)
            && resource.is_present_transitionable()
    }

    /// Returns `true` when `class` describes a resource envelope compatible with this
    /// request.
    #[must_use]
    pub fn matches_pool_class(self, class: &MemoryPoolClass) -> bool {
        self.matches_pool_class_base(class)
            && self.contract.matches(class.envelope.contract)
            && matches_safety_envelope(class.envelope.object_envelope(), self.required_safety)
            && self.matches_pool_class_support(class)
            && class.assurance.contains(self.required_safety)
    }

    /// Returns `true` when `class` satisfies the broad request shape before contract,
    /// safety, and support filtering.
    #[must_use]
    pub(super) fn matches_pool_class_base(self, class: &MemoryPoolClass) -> bool {
        self.matches_object_envelope(class.envelope.object_envelope(), class.topology_node)
    }

    /// Returns `true` when `strategy` can honestly produce a compatible pool resource.
    #[must_use]
    pub fn matches_strategy(self, strategy: &MemoryStrategyDescriptor) -> bool {
        if !self.matches_strategy_base(strategy) {
            return false;
        }

        let Some(output) = strategy.output else {
            return false;
        };

        self.matches_strategy_support(strategy)
            && self.contract.matches(output.envelope.contract)
            && matches_safety_envelope(output.envelope.object_envelope(), self.required_safety)
            && output.assurance.contains(self.required_safety)
    }

    /// Returns `true` when `strategy` satisfies the broad request shape before contract and
    /// safety filtering.
    #[must_use]
    pub(super) fn matches_strategy_base(self, strategy: &MemoryStrategyDescriptor) -> bool {
        if self.minimum_capacity < strategy.capacity.min_len {
            return false;
        }

        if let Some(max_len) = strategy.capacity.max_len
            && self.minimum_capacity > max_len
        {
            return false;
        }

        let Some(output) = strategy.output else {
            return false;
        };

        self.matches_object_envelope(output.envelope.object_envelope(), output.topology_node)
    }

    /// Returns `true` when `resource` satisfies the request's immutable contract filters.
    #[must_use]
    pub fn matches_resource_contract(self, resource: &MemoryResourceDescriptor) -> bool {
        self.contract.matches(resource.info.contract)
    }

    /// Returns `true` when `class` satisfies the request's immutable contract filters.
    #[must_use]
    pub fn matches_pool_class_contract(self, class: &MemoryPoolClass) -> bool {
        self.contract.matches(class.envelope.contract)
    }

    /// Returns `true` when `strategy` can produce a resource satisfying the immutable
    /// contract filters.
    #[must_use]
    pub fn matches_strategy_contract(self, strategy: &MemoryStrategyDescriptor) -> bool {
        strategy
            .output
            .is_some_and(|output| self.contract.matches(output.envelope.contract))
    }

    /// Returns `true` when `resource` satisfies the explicit safety requirements.
    #[must_use]
    pub fn matches_resource_safety(self, resource: &MemoryResourceDescriptor) -> bool {
        matches_safety_envelope(resource.object_envelope(), self.required_safety)
    }

    /// Returns `true` when `resource` satisfies the explicit runtime support requirements.
    #[must_use]
    pub const fn matches_resource_support(self, resource: &MemoryResourceDescriptor) -> bool {
        resource.info.support.ops.contains(self.required_ops)
            && resource
                .info
                .support
                .residency
                .contains(self.required_residency)
    }

    /// Returns `true` when `class` satisfies the explicit safety requirements.
    #[must_use]
    pub fn matches_pool_class_safety(self, class: &MemoryPoolClass) -> bool {
        matches_safety_envelope(class.envelope.object_envelope(), self.required_safety)
            && class.assurance.contains(self.required_safety)
    }

    /// Returns `true` when `class` satisfies the explicit runtime support requirements.
    #[must_use]
    pub const fn matches_pool_class_support(self, class: &MemoryPoolClass) -> bool {
        class.envelope.support.ops.contains(self.required_ops)
            && class
                .envelope
                .support
                .residency
                .contains(self.required_residency)
    }

    /// Returns `true` when `strategy` can produce a resource satisfying the explicit safety
    /// requirements.
    #[must_use]
    pub fn matches_strategy_safety(self, strategy: &MemoryStrategyDescriptor) -> bool {
        strategy.output.is_some_and(|output| {
            matches_safety_envelope(output.envelope.object_envelope(), self.required_safety)
                && output.assurance.contains(self.required_safety)
        })
    }

    /// Returns `true` when `strategy` satisfies the explicit support requirements.
    #[must_use]
    pub const fn matches_strategy_support(self, strategy: &MemoryStrategyDescriptor) -> bool {
        let Some(output) = strategy.output else {
            return false;
        };

        strategy.acquire.features.contains(self.required_features)
            && output.envelope.support.ops.contains(self.required_ops)
            && output
                .envelope
                .support
                .residency
                .contains(self.required_residency)
    }

    fn matches_object_envelope(
        self,
        envelope: MemoryObjectEnvelope,
        topology_node: Option<super::MemoryTopologyNodeId>,
    ) -> bool {
        if !self
            .required_domains
            .contains(domain_to_set(envelope.domain))
        {
            return false;
        }

        if !envelope.attrs.contains(self.required_attrs)
            || envelope.attrs.intersects(self.forbidden_attrs)
        {
            return false;
        }

        if envelope.hazards.intersects(self.forbidden_hazards) {
            return false;
        }

        matches_node_requirement(self.topology, topology_node)
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
