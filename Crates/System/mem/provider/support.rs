use super::MemoryTopologyNodeId;
use super::inventory::{MemoryPoolClassId, MemoryResourceDescriptor, MemoryStrategyDescriptor};
use super::request::MemoryPoolRequest;
use crate::mem::resource::{
    MemoryDomainSet, ResourceHazardSet, ResourceOpSet, ResourceResidencySupport,
};

bitflags::bitflags! {
    /// Coarse capabilities of a provider implementation.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MemoryProviderCaps: u32 {
        /// Exposes an inventory of concrete resource descriptors.
        const RESOURCE_INVENTORY   = 1 << 0;
        /// Exposes an inventory of acquisition strategies.
        const STRATEGY_INVENTORY   = 1 << 1;
        /// Exposes a normalized topology view.
        const TOPOLOGY             = 1 << 2;
        /// Exposes provider-defined pool-compatibility classes.
        const POOL_CLASSES         = 1 << 3;
        /// Supports pool-request assessment.
        const POOL_ASSESSMENT      = 1 << 4;
        /// Can bind or inventory externally governed ranges.
        const BIND_EXISTING        = 1 << 5;
        /// Can actively acquire new resources.
        const ACQUIRE_NEW          = 1 << 6;
        /// Inventory is expected to cover the full reachable memory picture.
        const EXHAUSTIVE_INVENTORY = 1 << 7;
        /// Topology view is expected to cover the full locality picture.
        const EXHAUSTIVE_TOPOLOGY  = 1 << 8;
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
        /// No compatible allocatable resource matched the request.
        const RESOURCE_COMPATIBILITY = 1 << 0;
        /// Compatible present resources existed, but not enough capacity was available.
        const CAPACITY               = 1 << 1;
        /// Available resources or strategies did not satisfy topology constraints.
        const TOPOLOGY               = 1 << 2;
        /// Present resources violated the required safety envelope.
        const SAFETY                 = 1 << 3;
        /// Present resources violated explicit contract requirements.
        const CONTRACT               = 1 << 4;
        /// Required operations or residency controls were unavailable.
        const SUPPORT                = 1 << 5;
        /// No acquisition strategy could plausibly satisfy the request later.
        const STRATEGY               = 1 << 6;
        /// Provider cannot prove the full answer from its current inventory surface.
        const INCOMPLETE_INVENTORY   = 1 << 7;
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

pub(super) fn has_required_support(
    resources: &[MemoryResourceDescriptor],
    strategies: &[MemoryStrategyDescriptor],
    request: &MemoryPoolRequest<'_>,
) -> bool {
    if request.required_ops == ResourceOpSet::empty()
        && request.required_residency == ResourceResidencySupport::BEST_EFFORT
    {
        return true;
    }

    let resources_support = resources.iter().any(|resource| {
        resource.info.support.ops.contains(request.required_ops)
            && resource
                .info
                .support
                .residency
                .contains(request.required_residency)
    });
    let strategies_support = strategies.iter().any(|strategy| {
        strategy.acquire.instance.ops.contains(request.required_ops)
            && strategy
                .acquire
                .instance
                .residency
                .contains(request.required_residency)
    });

    resources_support || strategies_support
}

pub(super) fn has_contract_candidate(
    resources: &[MemoryResourceDescriptor],
    pool_classes: &[super::MemoryPoolClass],
    request: &MemoryPoolRequest<'_>,
) -> bool {
    let resources_match = resources
        .iter()
        .any(|resource| request.contract.matches(resource.info.contract));
    let classes_match = pool_classes
        .iter()
        .any(|class| request.contract.matches(class.envelope.contract));

    resources_match || classes_match
}

pub(super) fn matches_safety(
    resource: &MemoryResourceDescriptor,
    required_safety: CriticalSafetyRequirements,
) -> bool {
    let info = resource.info;
    let attrs = info.attrs;
    let hazards = info.hazards;
    let contract = info.contract;

    if required_safety.contains(CriticalSafetyRequirements::POOLABLE)
        && !attrs.contains(crate::mem::resource::ResourceAttrs::ALLOCATABLE)
    {
        return false;
    }

    if required_safety.contains(CriticalSafetyRequirements::DETERMINISTIC_CAPACITY)
        && hazards.contains(ResourceHazardSet::OVERCOMMIT)
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
        && (!attrs.contains(crate::mem::resource::ResourceAttrs::COHERENT)
            || hazards.contains(ResourceHazardSet::NON_COHERENT))
    {
        return false;
    }

    if required_safety.contains(CriticalSafetyRequirements::NO_HAZARDOUS_IO)
        && (attrs.contains(crate::mem::resource::ResourceAttrs::HAZARDOUS_IO)
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
        && (!attrs.contains(crate::mem::resource::ResourceAttrs::INTEGRITY_MANAGED)
            && contract.integrity.is_none())
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

pub(super) fn preferred_class_id(
    classes: &[super::MemoryPoolClass],
    request: &MemoryPoolRequest<'_>,
) -> Option<MemoryPoolClassId> {
    classes
        .iter()
        .find(|class| request.matches_pool_class(class))
        .map(|class| class.id)
}
