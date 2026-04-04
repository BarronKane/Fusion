use crate::mem::resource::{
    MemoryDomainSet,
    ResourceAttrs,
    ResourceHazardSet,
};
use super::inventory::MemoryProviderInventory;
use super::request::MemoryPoolRequest;
use super::{
    MemoryObjectEnvelope,
    MemoryTopologyNodeId,
};

bitflags::bitflags! {
    /// Coarse capabilities of a provider implementation.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MemoryProviderCaps: u32 {
        /// Exposes an inventory of provider-known memory objects.
        const OBJECT_INVENTORY      = 1 << 0;
        /// Exposes an inventory of concrete pool-capable resources.
        const RESOURCE_INVENTORY    = 1 << 1;
        /// Exposes an inventory of acquisition strategies.
        const STRATEGY_INVENTORY    = 1 << 2;
        /// Exposes a normalized topology view.
        const TOPOLOGY              = 1 << 3;
        /// Exposes provider-defined pool-compatibility classes.
        const POOL_CLASSES          = 1 << 4;
        /// Supports pool-request assessment.
        const POOL_ASSESSMENT       = 1 << 5;
        /// Supports pool provisioning-plan generation.
        const POOL_PLANNING         = 1 << 6;
        /// Can bind or inventory externally governed ranges.
        const BIND_EXISTING         = 1 << 7;
        /// Can actively acquire new resources.
        const ACQUIRE_NEW           = 1 << 8;
        /// Inventory is expected to cover the full reachable memory picture.
        const EXHAUSTIVE_INVENTORY  = 1 << 9;
        /// Topology view is expected to cover the full locality picture.
        const EXHAUSTIVE_TOPOLOGY   = 1 << 10;
    }
}

bitflags::bitflags! {
    /// Safety-oriented requirements that a pool request may insist on.
    ///
    /// These flags are phrased as requirements on the provider-visible semantics rather than
    /// any one backend mechanism. That keeps the request portable while still allowing the
    /// provider to reject resources that are too hazardous for critical code.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct CriticalSafetyRequirements: u32 {
        /// Resource must be allocator-usable rather than a side-effecting or descriptive range.
        const POOLABLE               = 1 << 0;
        /// Resource must not rely on overcommit or later backing failure.
        const DETERMINISTIC_CAPACITY = 1 << 1;
        /// Resource must remain private rather than shared by contract.
        const PRIVATE_ONLY           = 1 << 2;
        /// Resource must not expose shared-aliasing hazards.
        const NO_SHARED_ALIASING     = 1 << 3;
        /// Resource state must not mutate outside the pool's control.
        const NO_EXTERNAL_MUTATION   = 1 << 4;
        /// Provider must not rely on emulated semantics for the resource contract.
        const NO_EMULATION           = 1 << 5;
        /// Resource must participate in the expected coherency domain.
        const REQUIRE_COHERENT       = 1 << 6;
        /// Resource must not have MMIO-style or hazardous I/O side effects.
        const NO_HAZARDOUS_IO        = 1 << 7;
        /// Resource must never admit executable use through its contract.
        const EXECUTE_NEVER          = 1 << 8;
        /// Resource must participate in an integrity-management regime.
        const INTEGRITY_MANAGED      = 1 << 9;
    }
}

bitflags::bitflags! {
    /// Coarse reasons why a pool request could not be satisfied as requested.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MemoryPoolAssessmentIssues: u32 {
        /// No compatible allocatable resource class or resource group matched the request.
        const RESOURCE_COMPATIBILITY = 1 << 0;
        /// Compatible present resources existed, but not enough immediately usable capacity was available.
        const CAPACITY               = 1 << 1;
        /// Available resources or strategies did not satisfy topology constraints.
        const TOPOLOGY               = 1 << 2;
        /// Available resources or strategies violated the required safety envelope.
        const SAFETY                 = 1 << 3;
        /// Available resources or strategies violated explicit contract requirements.
        const CONTRACT               = 1 << 4;
        /// Required operations or residency controls were unavailable.
        const SUPPORT                = 1 << 5;
        /// Present resources matched statically but required preparation before pool use.
        const STATE                  = 1 << 6;
        /// No acquisition or preparation path could plausibly satisfy the request later.
        const STRATEGY               = 1 << 7;
        /// Provider cannot prove the full answer from its current inventory surface.
        const INCOMPLETE_INVENTORY   = 1 << 8;
    }
}

/// Provider-wide support summary for inventory and orchestration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryProviderSupport {
    /// Coarse orchestration capabilities of the provider.
    pub caps: MemoryProviderCaps,
    /// Domains for which the provider knows about existing resources.
    pub discovered_domains: MemoryDomainSet,
    /// Domains the provider may be able to create or materialize later.
    pub acquirable_domains: MemoryDomainSet,
}

/// Candidate counts at successive request-filtering stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct CandidateStageCounts {
    pub topology_agnostic_objects: usize,
    pub compatible_objects: usize,
    pub compatible_resources: usize,
    pub contract_resources: usize,
    pub safety_resources: usize,
    pub support_resources: usize,
    pub compatible_strategies: usize,
    pub contract_strategies: usize,
    pub safety_strategies: usize,
    pub support_strategies: usize,
    pub compatible_classes: usize,
    pub contract_classes: usize,
    pub safety_classes: usize,
}

pub(super) fn candidate_stage_counts(
    inventory: MemoryProviderInventory<'_>,
    request: &MemoryPoolRequest<'_>,
) -> CandidateStageCounts {
    let mut counts = CandidateStageCounts::default();

    for object in inventory.objects {
        if matches_object_envelope_without_topology(object.envelope, *request) {
            counts.topology_agnostic_objects += 1;
        }
        if request.matches_object_base(object) {
            counts.compatible_objects += 1;
        }
    }

    for resource in inventory.resources {
        if request.matches_resource_base(resource) {
            counts.compatible_resources += 1;
            if request.matches_resource_contract(resource) {
                counts.contract_resources += 1;
                if request.matches_resource_safety(resource) {
                    counts.safety_resources += 1;
                    if request.matches_resource_support(resource) {
                        counts.support_resources += 1;
                    }
                }
            }
        }
    }

    for class in inventory.pool_classes {
        if request.matches_pool_class_base(class) {
            counts.compatible_classes += 1;
            if request.matches_pool_class_contract(class) {
                counts.contract_classes += 1;
                if request.matches_pool_class_safety(class) {
                    counts.safety_classes += 1;
                }
            }
        }
    }

    for strategy in inventory.strategies {
        if request.matches_strategy_base(strategy) {
            counts.compatible_strategies += 1;
            if request.matches_strategy_contract(strategy) {
                counts.contract_strategies += 1;
                if request.matches_strategy_safety(strategy) {
                    counts.safety_strategies += 1;
                    if request.matches_strategy_support(strategy) {
                        counts.support_strategies += 1;
                    }
                }
            }
        }
    }

    counts
}

pub(super) fn matches_safety_envelope(
    envelope: MemoryObjectEnvelope,
    required_safety: CriticalSafetyRequirements,
) -> bool {
    let attrs = envelope.attrs;
    let hazards = envelope.hazards;
    let contract = envelope.contract;

    if required_safety.contains(CriticalSafetyRequirements::POOLABLE)
        && !attrs.contains(ResourceAttrs::ALLOCATABLE)
    {
        return false;
    }

    if required_safety.contains(CriticalSafetyRequirements::DETERMINISTIC_CAPACITY)
        && (hazards.contains(ResourceHazardSet::OVERCOMMIT)
            || matches!(
                contract.overcommit,
                crate::mem::resource::OvercommitPolicy::Allow
            ))
    {
        return false;
    }

    if required_safety.contains(CriticalSafetyRequirements::PRIVATE_ONLY)
        && contract.sharing != crate::mem::resource::SharingPolicy::Private
    {
        return false;
    }

    if required_safety.contains(CriticalSafetyRequirements::NO_SHARED_ALIASING)
        && hazards.contains(ResourceHazardSet::SHARED_ALIASING)
    {
        return false;
    }

    if required_safety.contains(CriticalSafetyRequirements::NO_EXTERNAL_MUTATION)
        && hazards.contains(ResourceHazardSet::EXTERNAL_MUTATION)
    {
        return false;
    }

    if required_safety.contains(CriticalSafetyRequirements::NO_EMULATION)
        && hazards.contains(ResourceHazardSet::EMULATED)
    {
        return false;
    }

    if required_safety.contains(CriticalSafetyRequirements::REQUIRE_COHERENT)
        && (!attrs.contains(ResourceAttrs::COHERENT)
            || hazards.contains(ResourceHazardSet::NON_COHERENT))
    {
        return false;
    }

    if required_safety.contains(CriticalSafetyRequirements::NO_HAZARDOUS_IO)
        && (attrs.contains(ResourceAttrs::HAZARDOUS_IO)
            || hazards.contains(ResourceHazardSet::MMIO_SIDE_EFFECTS))
    {
        return false;
    }

    if required_safety.contains(CriticalSafetyRequirements::EXECUTE_NEVER)
        && (contract
            .allowed_protect
            .contains(fusion_pal::sys::mem::Protect::EXEC)
            || hazards.contains(ResourceHazardSet::EXECUTABLE))
    {
        return false;
    }

    if required_safety.contains(CriticalSafetyRequirements::INTEGRITY_MANAGED)
        && (!attrs.contains(ResourceAttrs::INTEGRITY_MANAGED) && contract.integrity.is_none())
    {
        return false;
    }

    true
}

pub(super) fn matches_node_requirement(
    preference: super::MemoryTopologyPreference,
    node: Option<MemoryTopologyNodeId>,
) -> bool {
    match preference {
        super::MemoryTopologyPreference::Anywhere
        | super::MemoryTopologyPreference::PreferNode(_) => true,
        super::MemoryTopologyPreference::RequireNode(required) => node == Some(required),
    }
}

const fn matches_object_envelope_without_topology(
    envelope: MemoryObjectEnvelope,
    request: MemoryPoolRequest<'_>,
) -> bool {
    request
        .required_domains
        .contains(domain_to_set(envelope.domain))
        && envelope.attrs.contains(request.required_attrs)
        && !envelope.attrs.intersects(request.forbidden_attrs)
        && !envelope.hazards.intersects(request.forbidden_hazards)
}

const fn domain_to_set(domain: crate::mem::resource::MemoryDomain) -> MemoryDomainSet {
    match domain {
        crate::mem::resource::MemoryDomain::VirtualAddressSpace => {
            MemoryDomainSet::VIRTUAL_ADDRESS_SPACE
        }
        crate::mem::resource::MemoryDomain::DeviceLocal => MemoryDomainSet::DEVICE_LOCAL,
        crate::mem::resource::MemoryDomain::Physical => MemoryDomainSet::PHYSICAL,
        crate::mem::resource::MemoryDomain::StaticRegion => MemoryDomainSet::STATIC_REGION,
        crate::mem::resource::MemoryDomain::Mmio => MemoryDomainSet::MMIO,
    }
}
