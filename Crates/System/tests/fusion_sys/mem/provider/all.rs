//! Public-contract tests for `fusion_sys::mem::provider`.
//!
//! These tests use tiny mock inventories to pin down the intended orchestration semantics
//! before a real provider implementation exists.

use core::num::NonZeroUsize;
use core::ptr::NonNull;

use fusion_pal::sys::mem::{
    CachePolicy, MemAdviceCaps, MemBackingCaps, MemPlacementCaps, Protect, Region,
};
use fusion_sys::mem::provider::{
    CriticalSafetyRequirements, MemoryCompatibilityEnvelope, MemoryPoolAssessmentVerdict,
    MemoryPoolClass, MemoryPoolClassId, MemoryPoolRequest, MemoryProvider, MemoryProviderBuildSpec,
    MemoryProviderCaps, MemoryProviderConflictPolicy, MemoryProviderDiscoveryPolicy,
    MemoryProviderInventory, MemoryProviderSupport, MemoryResourceDescriptor, MemoryResourceId,
    MemoryResourceOrigin, MemoryStrategyCapacity, MemoryStrategyDescriptor, MemoryStrategyId,
    MemoryStrategyKind, MemoryTopology, MemoryTopologyNode, MemoryTopologyNodeId,
    MemoryTopologyNodeKind,
};
use fusion_sys::mem::resource::{
    MemoryDomain, MemoryDomainSet, MemoryGeometry, OvercommitPolicy, ResourceAcquireSupport,
    ResourceAttrs, ResourceBackingKind, ResourceContract, ResourceFeatureSupport,
    ResourceHazardSet, ResourceInfo, ResourceOpSet, ResourceResidencySupport, ResourceState,
    ResourceSupport, SharingPolicy,
};

struct MockProvider {
    support: MemoryProviderSupport,
    topology_nodes: [MemoryTopologyNode; 1],
    resources: [MemoryResourceDescriptor; 1],
    strategies: [MemoryStrategyDescriptor; 1],
    pool_classes: [MemoryPoolClass; 1],
}

impl MemoryProvider for MockProvider {
    fn support(&self) -> MemoryProviderSupport {
        self.support
    }

    fn topology(&self) -> MemoryTopology<'_> {
        MemoryTopology {
            nodes: &self.topology_nodes,
            links: &[],
        }
    }

    fn inventory(&self) -> MemoryProviderInventory<'_> {
        MemoryProviderInventory {
            resources: &self.resources,
            strategies: &self.strategies,
            pool_classes: &self.pool_classes,
        }
    }
}

const fn mock_region(len: usize) -> Region {
    let base = NonNull::dangling();
    Region { base, len }
}

fn general_resource_info(len: usize) -> ResourceInfo {
    ResourceInfo::new(
        mock_region(len),
        MemoryDomain::VirtualAddressSpace,
        ResourceBackingKind::AnonymousPrivate,
        ResourceAttrs::ALLOCATABLE | ResourceAttrs::CACHEABLE | ResourceAttrs::COHERENT,
        MemoryGeometry {
            base_granule: NonZeroUsize::new(4096).unwrap(),
            alloc_granule: NonZeroUsize::new(4096).unwrap(),
            protect_granule: Some(NonZeroUsize::new(4096).unwrap()),
            commit_granule: None,
            lock_granule: Some(NonZeroUsize::new(4096).unwrap()),
            large_granule: None,
        },
        ResourceContract {
            allowed_protect: Protect::READ | Protect::WRITE,
            write_xor_execute: true,
            sharing: SharingPolicy::Private,
            overcommit: OvercommitPolicy::Disallow,
            cache_policy: CachePolicy::Default,
            integrity: None,
        },
        ResourceSupport {
            protect: Protect::READ | Protect::WRITE,
            ops: ResourceOpSet::QUERY | ResourceOpSet::LOCK,
            advice: MemAdviceCaps::empty(),
            residency: ResourceResidencySupport::BEST_EFFORT | ResourceResidencySupport::LOCKED,
        },
        ResourceHazardSet::empty(),
    )
}

fn mock_provider(resource_len: usize) -> MockProvider {
    let node = MemoryTopologyNode {
        id: MemoryTopologyNodeId(1),
        kind: MemoryTopologyNodeKind::NumaNode,
        parent: None,
        domains: MemoryDomainSet::VIRTUAL_ADDRESS_SPACE,
    };
    let resource_info = general_resource_info(resource_len);
    let pool_class = MemoryPoolClass {
        id: MemoryPoolClassId(7),
        envelope: MemoryCompatibilityEnvelope::from_resource_info(resource_info),
        assurance: CriticalSafetyRequirements::POOLABLE
            | CriticalSafetyRequirements::DETERMINISTIC_CAPACITY
            | CriticalSafetyRequirements::PRIVATE_ONLY
            | CriticalSafetyRequirements::NO_SHARED_ALIASING
            | CriticalSafetyRequirements::NO_EXTERNAL_MUTATION
            | CriticalSafetyRequirements::NO_EMULATION
            | CriticalSafetyRequirements::REQUIRE_COHERENT
            | CriticalSafetyRequirements::NO_HAZARDOUS_IO
            | CriticalSafetyRequirements::EXECUTE_NEVER,
        topology_node: Some(node.id),
    };

    MockProvider {
        support: MemoryProviderSupport {
            caps: MemoryProviderCaps::RESOURCE_INVENTORY
                | MemoryProviderCaps::STRATEGY_INVENTORY
                | MemoryProviderCaps::TOPOLOGY
                | MemoryProviderCaps::POOL_CLASSES
                | MemoryProviderCaps::POOL_ASSESSMENT
                | MemoryProviderCaps::ACQUIRE_NEW
                | MemoryProviderCaps::EXHAUSTIVE_INVENTORY
                | MemoryProviderCaps::EXHAUSTIVE_TOPOLOGY,
            discovered_domains: MemoryDomainSet::VIRTUAL_ADDRESS_SPACE,
            acquirable_domains: MemoryDomainSet::VIRTUAL_ADDRESS_SPACE,
        },
        topology_nodes: [node],
        resources: [MemoryResourceDescriptor {
            id: MemoryResourceId(11),
            info: resource_info,
            state: ResourceState::tracked(Protect::READ | Protect::WRITE, false, true),
            origin: MemoryResourceOrigin::Created,
            usable_len: resource_len,
            topology_node: Some(node.id),
            pool_class: Some(pool_class.id),
        }],
        strategies: [MemoryStrategyDescriptor {
            id: MemoryStrategyId(13),
            kind: MemoryStrategyKind::VirtualCreate,
            acquire: ResourceAcquireSupport {
                domains: MemoryDomainSet::VIRTUAL_ADDRESS_SPACE,
                backings: MemBackingCaps::ANON_PRIVATE,
                placements: MemPlacementCaps::ANYWHERE,
                instance: resource_info.support,
                features: ResourceFeatureSupport::OVERCOMMIT_DISALLOW,
                preferences: fusion_sys::mem::resource::ResourcePreferenceSet::empty(),
            },
            capacity: MemoryStrategyCapacity {
                min_len: 4096,
                max_len: None,
                granule: NonZeroUsize::new(4096).unwrap(),
            },
            assurance: pool_class.assurance,
            topology_node: Some(node.id),
            pool_class: Some(pool_class.id),
        }],
        pool_classes: [pool_class],
    }
}

#[test]
fn provider_inventory_exposes_pool_compatible_general_purpose_ram() {
    let provider = mock_provider(1024 * 1024);
    let request = MemoryPoolRequest::general_purpose(64 * 1024);

    let topology = provider.topology();
    assert_eq!(topology.nodes.len(), 1);

    let inventory = provider.inventory();
    assert_eq!(inventory.resources.len(), 1);
    assert_eq!(inventory.pool_classes.len(), 1);
    assert!(request.matches_resource(&inventory.resources[0]));
    assert!(request.matches_pool_class(&inventory.pool_classes[0]));
    assert!(inventory.pool_classes[0].accepts(&inventory.resources[0]));
}

#[test]
fn ready_assessment_prefers_present_capacity_when_enough_memory_exists() {
    let provider = mock_provider(1024 * 1024);
    let request = MemoryPoolRequest::general_purpose(128 * 1024);
    let assessment = provider.assess_pool(&request);

    assert_eq!(assessment.verdict, MemoryPoolAssessmentVerdict::Ready);
    assert!(assessment.is_ready());
    assert_eq!(assessment.matching_resource_count, 1);
    assert_eq!(assessment.matching_pool_class_count, 1);
    assert_eq!(assessment.matching_strategy_count, 1);
    assert!(assessment.preferred_pool_class.is_some());
}

#[test]
fn provisionable_assessment_uses_strategy_when_present_capacity_is_too_small() {
    let provider = mock_provider(32 * 1024);
    let request = MemoryPoolRequest::general_purpose(512 * 1024);
    let assessment = provider.assess_pool(&request);

    assert_eq!(
        assessment.verdict,
        MemoryPoolAssessmentVerdict::Provisionable
    );
    assert!(assessment.is_provisionable());
    assert_eq!(assessment.matching_resource_count, 0);
    assert_eq!(assessment.matching_strategy_count, 1);
}

#[test]
fn safety_critical_request_rejects_shared_aliasing_resources() {
    let mut provider = mock_provider(1024 * 1024);
    provider.resources[0].info.contract.sharing = SharingPolicy::Shared;
    provider.resources[0].info.hazards = ResourceHazardSet::SHARED_ALIASING;
    provider.pool_classes[0].envelope = provider.resources[0].compatibility();
    provider.pool_classes[0].assurance.remove(
        CriticalSafetyRequirements::PRIVATE_ONLY | CriticalSafetyRequirements::NO_SHARED_ALIASING,
    );
    provider.strategies[0].assurance.remove(
        CriticalSafetyRequirements::PRIVATE_ONLY | CriticalSafetyRequirements::NO_SHARED_ALIASING,
    );

    let mut request = MemoryPoolRequest::general_purpose(64 * 1024);
    request.required_safety |=
        CriticalSafetyRequirements::PRIVATE_ONLY | CriticalSafetyRequirements::NO_SHARED_ALIASING;
    let assessment = provider.assess_pool(&request);

    assert_eq!(assessment.verdict, MemoryPoolAssessmentVerdict::Rejected);
    assert_eq!(assessment.matching_resource_count, 0);
    assert_eq!(assessment.matching_strategy_count, 0);
}

#[test]
fn provider_build_spec_defaults_to_pal_discovery_with_explicit_overlays() {
    let spec = MemoryProviderBuildSpec::system();

    assert_eq!(
        spec.discovery,
        MemoryProviderDiscoveryPolicy::MergePalWithExplicit
    );
    assert_eq!(spec.conflict_policy, MemoryProviderConflictPolicy::Reject);
    assert!(spec.explicit_resources.is_empty());
    assert!(spec.explicit_topology_nodes.is_empty());
}
