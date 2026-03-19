//! Public-contract tests for `fusion_sys::mem::provider`.
//!
//! These tests use tiny mock inventories to pin down the intended orchestration semantics
//! before a real provider implementation exists.

use core::mem::align_of;
use core::num::NonZeroUsize;
use fusion_pal::sys::mem::{
    Address, CachePolicy, MemAdviceCaps, MemBackingCaps, MemCatalogResource, MemCatalogResourceId,
    MemCatalogResourceOrigin, MemCatalogStrategy, MemCatalogStrategyCapacity, MemCatalogStrategyId,
    MemCatalogStrategyKind, MemCatalogStrategyOutput, MemDomain, MemGeometry, MemOvercommitPolicy,
    MemPlacementCaps, MemPoolResourceReadiness, MemResourceAttrs, MemResourceBackingKind,
    MemResourceContract, MemResourceEnvelope, MemResourceHazardSet, MemResourceOpSet,
    MemResourceResidencySupport, MemResourceStateSummary, MemResourceSupport, MemSharingPolicy,
    MemStateValue, Protect, Region,
};
use fusion_sys::mem::provider::{
    CriticalSafetyRequirements, MemoryCompatibilityEnvelope, MemoryGroupDescriptor,
    MemoryObjectDescriptor, MemoryObjectEnvelope, MemoryObjectId, MemoryObjectOrigin,
    MemoryPoolAssessmentIssues, MemoryPoolAssessmentVerdict, MemoryPoolCandidateGroup,
    MemoryPoolClass, MemoryPoolClassId, MemoryPoolPlanStep, MemoryPoolPreparationKind,
    MemoryPoolRequest, MemoryProvider, MemoryProviderBuildSpec, MemoryProviderCaps,
    MemoryProviderConflictPolicy, MemoryProviderDiscoveryPolicy, MemoryProviderInventory,
    MemoryProviderSupport, MemoryResourceDescriptor, MemoryResourceId, MemoryResourceReadiness,
    MemoryStrategyCapacity, MemoryStrategyDescriptor, MemoryStrategyId, MemoryStrategyKind,
    MemoryStrategyOutputDescriptor, MemoryTopology, MemoryTopologyNode, MemoryTopologyNodeId,
    MemoryTopologyNodeKind, memory_object_from_catalog_resource,
    memory_resource_from_catalog_resource, memory_strategy_from_catalog_strategy,
};
use fusion_sys::mem::resource::{
    MemoryDomain, MemoryDomainSet, MemoryGeometry, OvercommitPolicy, ResourceAcquireSupport,
    ResourceAttrs, ResourceBackingKind, ResourceContract, ResourceFeatureSupport,
    ResourceHazardSet, ResourceInfo, ResourceOpSet, ResourceRange, ResourceResidencySupport,
    ResourceState, ResourceSupport, SharingPolicy, StateValue,
};

struct MockProvider<'a> {
    support: MemoryProviderSupport,
    topology_nodes: &'a [MemoryTopologyNode],
    objects: &'a [MemoryObjectDescriptor],
    resources: &'a [MemoryResourceDescriptor],
    strategies: &'a [MemoryStrategyDescriptor],
    pool_classes: &'a [MemoryPoolClass],
}

impl MemoryProvider for MockProvider<'_> {
    fn support(&self) -> MemoryProviderSupport {
        self.support
    }

    fn topology(&self) -> MemoryTopology<'_> {
        MemoryTopology {
            nodes: self.topology_nodes,
            links: &[],
        }
    }

    fn inventory(&self) -> MemoryProviderInventory<'_> {
        MemoryProviderInventory {
            objects: self.objects,
            resources: self.resources,
            strategies: self.strategies,
            pool_classes: self.pool_classes,
        }
    }
}

const fn mock_region(len: usize) -> Region {
    Region {
        base: Address::new(align_of::<usize>()),
        len,
    }
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
            commit_granule: Some(NonZeroUsize::new(4096).unwrap()),
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
            ops: ResourceOpSet::QUERY | ResourceOpSet::LOCK | ResourceOpSet::COMMIT,
            advice: MemAdviceCaps::empty(),
            residency: ResourceResidencySupport::BEST_EFFORT | ResourceResidencySupport::LOCKED,
        },
        ResourceHazardSet::empty(),
    )
}

fn pool_assurance() -> CriticalSafetyRequirements {
    CriticalSafetyRequirements::POOLABLE
        | CriticalSafetyRequirements::DETERMINISTIC_CAPACITY
        | CriticalSafetyRequirements::PRIVATE_ONLY
        | CriticalSafetyRequirements::NO_SHARED_ALIASING
        | CriticalSafetyRequirements::NO_EXTERNAL_MUTATION
        | CriticalSafetyRequirements::NO_EMULATION
        | CriticalSafetyRequirements::REQUIRE_COHERENT
        | CriticalSafetyRequirements::NO_HAZARDOUS_IO
        | CriticalSafetyRequirements::EXECUTE_NEVER
}

const fn mock_node() -> MemoryTopologyNode {
    MemoryTopologyNode {
        id: MemoryTopologyNodeId(1),
        kind: MemoryTopologyNodeKind::NumaNode,
        parent: None,
        domains: MemoryDomainSet::VIRTUAL_ADDRESS_SPACE,
    }
}

const fn object_from_info(
    id: u32,
    info: ResourceInfo,
    len: usize,
    node: MemoryTopologyNodeId,
) -> MemoryObjectDescriptor {
    MemoryObjectDescriptor {
        id: MemoryObjectId(id),
        envelope: MemoryObjectEnvelope::from_resource_info(info),
        cpu_range: Some(mock_region(len)),
        origin: MemoryObjectOrigin::Created,
        usable_len: len,
        topology_node: Some(node),
    }
}

fn resource_from_info(
    id: u32,
    object_id: MemoryObjectId,
    info: ResourceInfo,
    node: MemoryTopologyNodeId,
    class_id: MemoryPoolClassId,
    readiness: MemoryResourceReadiness,
    usable: (usize, usize),
) -> MemoryResourceDescriptor {
    resource_from_info_with_class(id, object_id, info, node, Some(class_id), readiness, usable)
}

fn resource_from_info_with_class(
    id: u32,
    object_id: MemoryObjectId,
    info: ResourceInfo,
    node: MemoryTopologyNodeId,
    class_id: Option<MemoryPoolClassId>,
    readiness: MemoryResourceReadiness,
    usable: (usize, usize),
) -> MemoryResourceDescriptor {
    MemoryResourceDescriptor {
        id: MemoryResourceId(id),
        object_id: Some(object_id),
        info,
        state: ResourceState::tracked(Protect::READ | Protect::WRITE, false, true),
        origin: MemoryObjectOrigin::Created,
        usable_now_len: usable.0,
        usable_max_len: usable.1,
        readiness,
        topology_node: Some(node),
        pool_class: class_id,
    }
}

fn class_for_info(id: u32, info: ResourceInfo, node: MemoryTopologyNodeId) -> MemoryPoolClass {
    MemoryPoolClass {
        id: MemoryPoolClassId(id),
        envelope: MemoryCompatibilityEnvelope::from_resource_info(info),
        assurance: pool_assurance(),
        topology_node: Some(node),
    }
}

fn strategy_for_info(
    id: u32,
    info: ResourceInfo,
    class_id: MemoryPoolClassId,
    node: MemoryTopologyNodeId,
) -> MemoryStrategyDescriptor {
    strategy_for_info_with_class(id, info, Some(class_id), node)
}

fn strategy_for_info_with_class(
    id: u32,
    info: ResourceInfo,
    class_id: Option<MemoryPoolClassId>,
    node: MemoryTopologyNodeId,
) -> MemoryStrategyDescriptor {
    MemoryStrategyDescriptor {
        id: MemoryStrategyId(id),
        kind: MemoryStrategyKind::VirtualCreate,
        acquire: ResourceAcquireSupport {
            domains: MemoryDomainSet::VIRTUAL_ADDRESS_SPACE,
            backings: MemBackingCaps::ANON_PRIVATE,
            placements: MemPlacementCaps::ANYWHERE,
            instance: info.support,
            features: ResourceFeatureSupport::OVERCOMMIT_DISALLOW,
            preferences: fusion_sys::mem::resource::ResourcePreferenceSet::empty(),
        },
        capacity: MemoryStrategyCapacity {
            min_len: 4096,
            max_len: None,
            granule: NonZeroUsize::new(4096).unwrap(),
        },
        output: Some(MemoryStrategyOutputDescriptor {
            envelope: MemoryCompatibilityEnvelope::from_resource_info(info),
            readiness: MemoryResourceReadiness::ReadyNow,
            assurance: pool_assurance(),
            topology_node: Some(node),
            pool_class: class_id,
        }),
    }
}

fn provider_support() -> MemoryProviderSupport {
    MemoryProviderSupport {
        caps: MemoryProviderCaps::OBJECT_INVENTORY
            | MemoryProviderCaps::RESOURCE_INVENTORY
            | MemoryProviderCaps::STRATEGY_INVENTORY
            | MemoryProviderCaps::TOPOLOGY
            | MemoryProviderCaps::POOL_CLASSES
            | MemoryProviderCaps::POOL_ASSESSMENT
            | MemoryProviderCaps::POOL_PLANNING
            | MemoryProviderCaps::ACQUIRE_NEW
            | MemoryProviderCaps::EXHAUSTIVE_INVENTORY
            | MemoryProviderCaps::EXHAUSTIVE_TOPOLOGY,
        discovered_domains: MemoryDomainSet::VIRTUAL_ADDRESS_SPACE,
        acquirable_domains: MemoryDomainSet::VIRTUAL_ADDRESS_SPACE,
    }
}

#[test]
fn provider_inventory_tracks_broad_objects_and_pool_resources_separately() {
    let node = mock_node();
    let len = 1024 * 1024;
    let info = general_resource_info(len);
    let class = class_for_info(7, info, node.id);
    let object = object_from_info(11, info, len, node.id);
    let resource = resource_from_info(
        21,
        object.id,
        info,
        node.id,
        class.id,
        MemoryResourceReadiness::ReadyNow,
        (1024 * 1024, 1024 * 1024),
    );
    let opaque_object = MemoryObjectDescriptor {
        id: MemoryObjectId(12),
        envelope: object.envelope,
        cpu_range: None,
        origin: MemoryObjectOrigin::Discovered,
        usable_len: 2 * 1024 * 1024,
        topology_node: Some(node.id),
    };
    let provider = MockProvider {
        support: provider_support(),
        topology_nodes: &[node],
        objects: &[object, opaque_object],
        resources: &[resource],
        strategies: &[strategy_for_info(31, info, class.id, node.id)],
        pool_classes: &[class],
    };

    let inventory = provider.inventory();
    assert_eq!(inventory.objects.len(), 2);
    assert_eq!(inventory.resources.len(), 1);
    assert!(inventory.objects[0].is_cpu_addressable());
    assert!(!inventory.objects[1].is_cpu_addressable());
}

#[test]
fn ready_assessment_aggregates_multiple_compatible_resources() {
    let node = mock_node();
    let len = 128 * 1024;
    let info = general_resource_info(len);
    let class = class_for_info(7, info, node.id);
    let object_a = object_from_info(11, info, len, node.id);
    let object_b = object_from_info(12, info, len, node.id);
    let resource_a = resource_from_info(
        21,
        object_a.id,
        info,
        node.id,
        class.id,
        MemoryResourceReadiness::ReadyNow,
        (128 * 1024, 128 * 1024),
    );
    let resource_b = resource_from_info(
        22,
        object_b.id,
        info,
        node.id,
        class.id,
        MemoryResourceReadiness::ReadyNow,
        (128 * 1024, 128 * 1024),
    );
    let provider = MockProvider {
        support: provider_support(),
        topology_nodes: &[node],
        objects: &[object_a, object_b],
        resources: &[resource_a, resource_b],
        strategies: &[],
        pool_classes: &[class],
    };

    let request = MemoryPoolRequest::general_purpose(256 * 1024);
    let assessment = provider.assess_pool(&request);

    assert_eq!(assessment.verdict, MemoryPoolAssessmentVerdict::Ready);
    assert_eq!(assessment.matching_resource_count, 2);
    assert_eq!(assessment.matching_ready_resource_count, 2);
    assert_eq!(assessment.matching_present_bytes, 256 * 1024);
}

#[test]
fn provisionable_assessment_uses_present_state_transitions_before_new_strategies() {
    let node = mock_node();
    let len = 512 * 1024;
    let info = general_resource_info(len);
    let class = class_for_info(7, info, node.id);
    let object = object_from_info(11, info, len, node.id);
    let resource = MemoryResourceDescriptor {
        id: MemoryResourceId(21),
        object_id: Some(object.id),
        info,
        state: ResourceState::snapshot(
            StateValue::Uniform(Protect::READ | Protect::WRITE),
            StateValue::Uniform(false),
            StateValue::Uniform(false),
        ),
        origin: MemoryObjectOrigin::Created,
        usable_now_len: 0,
        usable_max_len: 512 * 1024,
        readiness: MemoryResourceReadiness::RequiresCommit,
        topology_node: Some(node.id),
        pool_class: Some(class.id),
    };
    let provider = MockProvider {
        support: provider_support(),
        topology_nodes: &[node],
        objects: &[object],
        resources: &[resource],
        strategies: &[],
        pool_classes: &[class],
    };

    let request = MemoryPoolRequest::general_purpose(256 * 1024);
    let assessment = provider.assess_pool(&request);

    assert_eq!(
        assessment.verdict,
        MemoryPoolAssessmentVerdict::Provisionable
    );
    assert_eq!(assessment.matching_present_bytes, 0);
    assert_eq!(assessment.matching_transitionable_bytes, 512 * 1024);
    assert!(
        assessment
            .issues
            .contains(MemoryPoolAssessmentIssues::STATE)
    );
    assert!(
        assessment
            .issues
            .contains(MemoryPoolAssessmentIssues::CAPACITY)
    );
}

#[test]
fn strategy_matching_uses_output_envelope_not_just_acquire_surface() {
    let node = mock_node();
    let len = 64 * 1024;
    let info = general_resource_info(len);
    let class = class_for_info(7, info, node.id);
    let mut strategy = strategy_for_info(31, info, class.id, node.id);
    let output = strategy.output.as_mut().unwrap();
    output.envelope.hazards = ResourceHazardSet::SHARED_ALIASING;
    output.envelope.contract.sharing = SharingPolicy::Shared;
    output.assurance.remove(
        CriticalSafetyRequirements::PRIVATE_ONLY | CriticalSafetyRequirements::NO_SHARED_ALIASING,
    );

    let provider = MockProvider {
        support: provider_support(),
        topology_nodes: &[node],
        objects: &[],
        resources: &[],
        strategies: &[strategy],
        pool_classes: &[class],
    };

    let mut request = MemoryPoolRequest::general_purpose(64 * 1024);
    request.required_safety |=
        CriticalSafetyRequirements::PRIVATE_ONLY | CriticalSafetyRequirements::NO_SHARED_ALIASING;
    let assessment = provider.assess_pool(&request);

    assert_eq!(assessment.verdict, MemoryPoolAssessmentVerdict::Rejected);
    assert_eq!(assessment.matching_strategy_count, 0);
}

#[test]
fn pool_plan_prefers_ready_resources_then_preparation_steps() {
    let node = mock_node();
    let len = 256 * 1024;
    let info = general_resource_info(len);
    let class = class_for_info(7, info, node.id);
    let object_a = object_from_info(11, info, len, node.id);
    let object_b = object_from_info(12, info, len, node.id);
    let ready = resource_from_info(
        21,
        object_a.id,
        info,
        node.id,
        class.id,
        MemoryResourceReadiness::ReadyNow,
        (128 * 1024, 128 * 1024),
    );
    let staged = resource_from_info(
        22,
        object_b.id,
        info,
        node.id,
        class.id,
        MemoryResourceReadiness::RequiresStateTransition,
        (0, 256 * 1024),
    );
    let provider = MockProvider {
        support: provider_support(),
        topology_nodes: &[node],
        objects: &[object_a, object_b],
        resources: &[ready, staged],
        strategies: &[strategy_for_info(31, info, class.id, node.id)],
        pool_classes: &[class],
    };

    let request = MemoryPoolRequest::general_purpose(256 * 1024);
    let mut steps = [MemoryPoolPlanStep::CreateResource {
        strategy_id: MemoryStrategyId(0),
        range: ResourceRange::whole(0),
    }; 4];
    let plan = provider.plan_pool(&request, &mut steps);

    assert_eq!(
        plan.verdict,
        fusion_sys::mem::provider::MemoryPoolPlanVerdict::Provisionable
    );
    assert_eq!(plan.target_capacity, 256 * 1024);
    assert_eq!(plan.step_count, 2);
    assert_eq!(
        steps[0],
        MemoryPoolPlanStep::UsePresentResource {
            resource_id: ready.id,
            range: ResourceRange::whole(128 * 1024),
        }
    );
    assert_eq!(
        steps[1],
        MemoryPoolPlanStep::PreparePresentResource {
            resource_id: staged.id,
            range: ResourceRange::whole(128 * 1024),
            preparation: MemoryPoolPreparationKind::StateTransition,
        }
    );
}

#[test]
fn provider_build_spec_defaults_to_pal_discovery_with_explicit_overlays() {
    let spec = MemoryProviderBuildSpec::system();

    assert_eq!(
        spec.discovery,
        MemoryProviderDiscoveryPolicy::MergePalWithExplicit
    );
    assert_eq!(spec.conflict_policy, MemoryProviderConflictPolicy::Reject);
    assert!(spec.explicit_objects.is_empty());
    assert!(spec.explicit_resources.is_empty());
    assert!(spec.explicit_topology_nodes.is_empty());
}

#[test]
fn provider_group_enumeration_exposes_canonical_compatible_groups() {
    let node = mock_node();
    let len = 128 * 1024;
    let info = general_resource_info(len);
    let class = class_for_info(7, info, node.id);
    let object_a = object_from_info(11, info, len, node.id);
    let object_b = object_from_info(12, info, len, node.id);
    let resource_a = resource_from_info(
        21,
        object_a.id,
        info,
        node.id,
        class.id,
        MemoryResourceReadiness::ReadyNow,
        (128 * 1024, 128 * 1024),
    );
    let resource_b = resource_from_info(
        22,
        object_b.id,
        info,
        node.id,
        class.id,
        MemoryResourceReadiness::ReadyNow,
        (128 * 1024, 128 * 1024),
    );
    let strategy = strategy_for_info(31, info, class.id, node.id);
    let provider = MockProvider {
        support: provider_support(),
        topology_nodes: &[node],
        objects: &[object_a, object_b],
        resources: &[resource_a, resource_b],
        strategies: &[strategy],
        pool_classes: &[class],
    };

    let mut groups = [MemoryGroupDescriptor {
        id: fusion_sys::mem::provider::MemoryGroupId(usize::MAX),
        class_id: None,
        envelope: class.envelope,
        topology_node: None,
        resource_count: 0,
        strategy_count: 0,
    }; 2];
    let summary = provider.write_groups(&mut groups);

    assert_eq!(summary.inventory_groups, 1);
    assert_eq!(summary.matching_groups, 1);
    assert_eq!(summary.written_groups, 1);
    assert!(!summary.truncated);
    assert_eq!(groups[0].class_id, Some(class.id));
    assert_eq!(groups[0].resource_count, 2);
    assert_eq!(groups[0].strategy_count, 1);
}

#[test]
fn provider_candidate_group_enumeration_aggregates_request_scoped_capacity() {
    let node = mock_node();
    let len = 128 * 1024;
    let info = general_resource_info(len);
    let class = class_for_info(7, info, node.id);
    let object_a = object_from_info(11, info, len, node.id);
    let object_b = object_from_info(12, info, len, node.id);
    let ready = resource_from_info(
        21,
        object_a.id,
        info,
        node.id,
        class.id,
        MemoryResourceReadiness::ReadyNow,
        (128 * 1024, 128 * 1024),
    );
    let staged = resource_from_info(
        22,
        object_b.id,
        info,
        node.id,
        class.id,
        MemoryResourceReadiness::RequiresCommit,
        (0, 128 * 1024),
    );
    let provider = MockProvider {
        support: provider_support(),
        topology_nodes: &[node],
        objects: &[object_a, object_b],
        resources: &[ready, staged],
        strategies: &[],
        pool_classes: &[class],
    };

    let request = MemoryPoolRequest::general_purpose(256 * 1024);
    let mut groups = [MemoryPoolCandidateGroup {
        group: MemoryGroupDescriptor {
            id: fusion_sys::mem::provider::MemoryGroupId(usize::MAX),
            class_id: None,
            envelope: class.envelope,
            topology_node: None,
            resource_count: 0,
            strategy_count: 0,
        },
        matching_resource_count: 0,
        matching_ready_resource_count: 0,
        matching_strategy_count: 0,
        ready_bytes: 0,
        transitionable_bytes: 0,
        verdict: MemoryPoolAssessmentVerdict::Rejected,
    }; 2];
    let summary = provider.write_candidate_groups(&request, &mut groups);

    assert_eq!(summary.inventory_groups, 1);
    assert_eq!(summary.matching_groups, 1);
    assert_eq!(summary.written_groups, 1);
    assert_eq!(groups[0].matching_resource_count, 2);
    assert_eq!(groups[0].matching_ready_resource_count, 1);
    assert_eq!(groups[0].ready_bytes, 128 * 1024);
    assert_eq!(groups[0].transitionable_bytes, 256 * 1024);
    assert_eq!(
        groups[0].verdict,
        MemoryPoolAssessmentVerdict::Provisionable
    );
}

#[test]
fn provider_group_enumeration_deduplicates_unclassed_resources_and_strategies() {
    let node = mock_node();
    let len = 64 * 1024;
    let info = general_resource_info(len);
    let object_a = object_from_info(11, info, len, node.id);
    let object_b = object_from_info(12, info, len, node.id);
    let resource_a = resource_from_info_with_class(
        21,
        object_a.id,
        info,
        node.id,
        None,
        MemoryResourceReadiness::ReadyNow,
        (len, len),
    );
    let resource_b = resource_from_info_with_class(
        22,
        object_b.id,
        info,
        node.id,
        None,
        MemoryResourceReadiness::ReadyNow,
        (len, len),
    );
    let strategy = strategy_for_info_with_class(31, info, None, node.id);
    let provider = MockProvider {
        support: provider_support(),
        topology_nodes: &[node],
        objects: &[object_a, object_b],
        resources: &[resource_a, resource_b],
        strategies: &[strategy],
        pool_classes: &[],
    };

    let mut groups = [MemoryGroupDescriptor {
        id: fusion_sys::mem::provider::MemoryGroupId(usize::MAX),
        class_id: Some(MemoryPoolClassId(u32::MAX)),
        envelope: MemoryCompatibilityEnvelope::from_resource_info(info),
        topology_node: None,
        resource_count: 0,
        strategy_count: 0,
    }; 2];
    let summary = provider.write_groups(&mut groups);

    assert_eq!(summary.inventory_groups, 1);
    assert_eq!(summary.matching_groups, 1);
    assert_eq!(summary.written_groups, 1);
    assert_eq!(groups[0].class_id, None);
    assert_eq!(groups[0].resource_count, 2);
    assert_eq!(groups[0].strategy_count, 1);
}

#[test]
fn provider_group_enumeration_emits_unclassed_strategy_only_group() {
    let node = mock_node();
    let info = general_resource_info(64 * 1024);
    let mut strategy = strategy_for_info_with_class(31, info, None, node.id);
    let output = strategy.output.as_mut().unwrap();
    output.envelope.backing = ResourceBackingKind::StaticRegion;
    output.envelope.attrs |= ResourceAttrs::STATIC_REGION;
    let envelope = output.envelope;
    let provider = MockProvider {
        support: provider_support(),
        topology_nodes: &[node],
        objects: &[],
        resources: &[],
        strategies: &[strategy],
        pool_classes: &[],
    };

    let mut groups = [MemoryGroupDescriptor {
        id: fusion_sys::mem::provider::MemoryGroupId(usize::MAX),
        class_id: Some(MemoryPoolClassId(u32::MAX)),
        envelope,
        topology_node: None,
        resource_count: 0,
        strategy_count: 0,
    }; 1];
    let summary = provider.write_groups(&mut groups);

    assert_eq!(summary.inventory_groups, 1);
    assert_eq!(summary.matching_groups, 1);
    assert_eq!(summary.written_groups, 1);
    assert_eq!(groups[0].class_id, None);
    assert_eq!(groups[0].resource_count, 0);
    assert_eq!(groups[0].strategy_count, 1);
}

#[test]
fn provider_candidate_group_summary_reports_inventory_and_matching_counts() {
    let node = mock_node();
    let len = 64 * 1024;
    let info = general_resource_info(len);
    let object = object_from_info(11, info, len, node.id);
    let resource = resource_from_info_with_class(
        21,
        object.id,
        info,
        node.id,
        None,
        MemoryResourceReadiness::ReadyNow,
        (len, len),
    );
    let provider = MockProvider {
        support: provider_support(),
        topology_nodes: &[node],
        objects: &[object],
        resources: &[resource],
        strategies: &[],
        pool_classes: &[],
    };

    let mut request = MemoryPoolRequest::general_purpose(len);
    request.required_domains = MemoryDomainSet::DEVICE_LOCAL;

    let mut groups = [MemoryPoolCandidateGroup {
        group: MemoryGroupDescriptor {
            id: fusion_sys::mem::provider::MemoryGroupId(usize::MAX),
            class_id: None,
            envelope: MemoryCompatibilityEnvelope::from_resource_info(info),
            topology_node: None,
            resource_count: 0,
            strategy_count: 0,
        },
        matching_resource_count: 0,
        matching_ready_resource_count: 0,
        matching_strategy_count: 0,
        ready_bytes: 0,
        transitionable_bytes: 0,
        verdict: MemoryPoolAssessmentVerdict::Rejected,
    }; 1];
    let summary = provider.write_candidate_groups(&request, &mut groups);

    assert_eq!(summary.inventory_groups, 1);
    assert_eq!(summary.matching_groups, 0);
    assert_eq!(summary.written_groups, 0);
    assert!(!summary.truncated);
}

#[test]
fn provider_plan_uses_split_descriptors_for_partially_ready_object() {
    let node = mock_node();
    let half = 32 * 1024;
    let info = general_resource_info(half);
    let class = class_for_info(7, info, node.id);
    let object = object_from_info(11, info, half * 2, node.id);
    let ready = resource_from_info(
        21,
        object.id,
        info,
        node.id,
        class.id,
        MemoryResourceReadiness::ReadyNow,
        (half, half),
    );
    let staged = resource_from_info(
        22,
        object.id,
        info,
        node.id,
        class.id,
        MemoryResourceReadiness::RequiresCommit,
        (0, half),
    );
    let provider = MockProvider {
        support: provider_support(),
        topology_nodes: &[node],
        objects: &[object],
        resources: &[ready, staged],
        strategies: &[],
        pool_classes: &[class],
    };

    let request = MemoryPoolRequest::general_purpose(half * 2);
    let mut steps = [MemoryPoolPlanStep::UsePresentResource {
        resource_id: MemoryResourceId(0),
        range: ResourceRange::whole(0),
    }; 4];
    let plan = provider.plan_pool(&request, &mut steps);

    assert_eq!(
        plan.verdict,
        fusion_sys::mem::provider::MemoryPoolPlanVerdict::Provisionable
    );
    assert_eq!(plan.planned_bytes, half * 2);
    assert_eq!(plan.step_count, 2);
    assert_eq!(
        steps[0],
        MemoryPoolPlanStep::UsePresentResource {
            resource_id: ready.id,
            range: ResourceRange::whole(half),
        }
    );
    assert_eq!(
        steps[1],
        MemoryPoolPlanStep::PreparePresentResource {
            resource_id: staged.id,
            range: ResourceRange::whole(half),
            preparation: MemoryPoolPreparationKind::Commit,
        }
    );
}

#[test]
fn general_purpose_request_excludes_mmio_and_device_local_domains() {
    let request = MemoryPoolRequest::general_purpose(4096);

    assert!(
        request
            .required_domains
            .contains(MemoryDomainSet::VIRTUAL_ADDRESS_SPACE)
    );
    assert!(request.required_domains.contains(MemoryDomainSet::PHYSICAL));
    assert!(
        request
            .required_domains
            .contains(MemoryDomainSet::STATIC_REGION)
    );
    assert!(
        !request
            .required_domains
            .contains(MemoryDomainSet::DEVICE_LOCAL)
    );
    assert!(!request.required_domains.contains(MemoryDomainSet::MMIO));
}

macro_rules! assert_flag_eq {
    ($pal:path, $sys:path) => {
        assert_eq!($pal.bits(), $sys.bits());
    };
}

#[test]
fn pal_and_sys_domain_flag_layouts_remain_compatible() {
    assert_flag_eq!(
        fusion_pal::sys::mem::MemDomainSet::VIRTUAL_ADDRESS_SPACE,
        fusion_sys::mem::resource::MemoryDomainSet::VIRTUAL_ADDRESS_SPACE
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemDomainSet::DEVICE_LOCAL,
        fusion_sys::mem::resource::MemoryDomainSet::DEVICE_LOCAL
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemDomainSet::PHYSICAL,
        fusion_sys::mem::resource::MemoryDomainSet::PHYSICAL
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemDomainSet::STATIC_REGION,
        fusion_sys::mem::resource::MemoryDomainSet::STATIC_REGION
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemDomainSet::MMIO,
        fusion_sys::mem::resource::MemoryDomainSet::MMIO
    );
}

#[test]
fn pal_and_sys_attr_flag_layouts_remain_compatible() {
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceAttrs::ALLOCATABLE,
        fusion_sys::mem::resource::ResourceAttrs::ALLOCATABLE
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceAttrs::READ_ONLY_BACKING,
        fusion_sys::mem::resource::ResourceAttrs::READ_ONLY_BACKING
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceAttrs::DMA_VISIBLE,
        fusion_sys::mem::resource::ResourceAttrs::DMA_VISIBLE
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceAttrs::DEVICE_LOCAL,
        fusion_sys::mem::resource::ResourceAttrs::DEVICE_LOCAL
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceAttrs::CACHEABLE,
        fusion_sys::mem::resource::ResourceAttrs::CACHEABLE
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceAttrs::COHERENT,
        fusion_sys::mem::resource::ResourceAttrs::COHERENT
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceAttrs::PHYS_CONTIGUOUS,
        fusion_sys::mem::resource::ResourceAttrs::PHYS_CONTIGUOUS
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceAttrs::TAGGED,
        fusion_sys::mem::resource::ResourceAttrs::TAGGED
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceAttrs::INTEGRITY_MANAGED,
        fusion_sys::mem::resource::ResourceAttrs::INTEGRITY_MANAGED
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceAttrs::STATIC_REGION,
        fusion_sys::mem::resource::ResourceAttrs::STATIC_REGION
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceAttrs::HAZARDOUS_IO,
        fusion_sys::mem::resource::ResourceAttrs::HAZARDOUS_IO
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceAttrs::PERSISTENT,
        fusion_sys::mem::resource::ResourceAttrs::PERSISTENT
    );
}

#[test]
fn pal_and_sys_op_flag_layouts_remain_compatible() {
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceOpSet::PROTECT,
        fusion_sys::mem::resource::ResourceOpSet::PROTECT
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceOpSet::ADVISE,
        fusion_sys::mem::resource::ResourceOpSet::ADVISE
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceOpSet::LOCK,
        fusion_sys::mem::resource::ResourceOpSet::LOCK
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceOpSet::QUERY,
        fusion_sys::mem::resource::ResourceOpSet::QUERY
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceOpSet::COMMIT,
        fusion_sys::mem::resource::ResourceOpSet::COMMIT
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceOpSet::DECOMMIT,
        fusion_sys::mem::resource::ResourceOpSet::DECOMMIT
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceOpSet::DISCARD,
        fusion_sys::mem::resource::ResourceOpSet::DISCARD
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceOpSet::FLUSH,
        fusion_sys::mem::resource::ResourceOpSet::FLUSH
    );
}

#[test]
fn pal_and_sys_residency_flag_layouts_remain_compatible() {
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceResidencySupport::BEST_EFFORT,
        fusion_sys::mem::resource::ResourceResidencySupport::BEST_EFFORT
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceResidencySupport::PREFAULT,
        fusion_sys::mem::resource::ResourceResidencySupport::PREFAULT
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceResidencySupport::LOCKED,
        fusion_sys::mem::resource::ResourceResidencySupport::LOCKED
    );
}

#[test]
fn pal_and_sys_feature_and_preference_flag_layouts_remain_compatible() {
    assert_flag_eq!(
        fusion_pal::sys::mem::MemStrategyFeatureSupport::OVERCOMMIT_DISALLOW,
        fusion_sys::mem::resource::ResourceFeatureSupport::OVERCOMMIT_DISALLOW
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemStrategyFeatureSupport::CACHE_POLICY,
        fusion_sys::mem::resource::ResourceFeatureSupport::CACHE_POLICY
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemStrategyFeatureSupport::INTEGRITY,
        fusion_sys::mem::resource::ResourceFeatureSupport::INTEGRITY
    );

    assert_flag_eq!(
        fusion_pal::sys::mem::MemStrategyPreferenceSet::PLACEMENT,
        fusion_sys::mem::resource::ResourcePreferenceSet::PLACEMENT
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemStrategyPreferenceSet::PREFAULT,
        fusion_sys::mem::resource::ResourcePreferenceSet::PREFAULT
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemStrategyPreferenceSet::LOCK,
        fusion_sys::mem::resource::ResourcePreferenceSet::LOCK
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemStrategyPreferenceSet::HUGE_PAGES,
        fusion_sys::mem::resource::ResourcePreferenceSet::HUGE_PAGES
    );
}

#[test]
fn pal_and_sys_hazard_flag_layouts_remain_compatible() {
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceHazardSet::EXECUTABLE,
        fusion_sys::mem::resource::ResourceHazardSet::EXECUTABLE
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceHazardSet::SHARED_ALIASING,
        fusion_sys::mem::resource::ResourceHazardSet::SHARED_ALIASING
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceHazardSet::EMULATED,
        fusion_sys::mem::resource::ResourceHazardSet::EMULATED
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceHazardSet::OVERCOMMIT,
        fusion_sys::mem::resource::ResourceHazardSet::OVERCOMMIT
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceHazardSet::NON_COHERENT,
        fusion_sys::mem::resource::ResourceHazardSet::NON_COHERENT
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceHazardSet::EXTERNAL_MUTATION,
        fusion_sys::mem::resource::ResourceHazardSet::EXTERNAL_MUTATION
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceHazardSet::MMIO_SIDE_EFFECTS,
        fusion_sys::mem::resource::ResourceHazardSet::MMIO_SIDE_EFFECTS
    );
    assert_flag_eq!(
        fusion_pal::sys::mem::MemResourceHazardSet::PERSISTENCE_REQUIRES_FLUSH,
        fusion_sys::mem::resource::ResourceHazardSet::PERSISTENCE_REQUIRES_FLUSH
    );
}

#[test]
fn pal_catalog_adapters_preserve_object_resource_and_strategy_truth() {
    let node = mock_node();
    let catalog_resource = MemCatalogResource {
        id: MemCatalogResourceId(91),
        envelope: MemResourceEnvelope {
            domain: MemDomain::VirtualAddressSpace,
            backing: MemResourceBackingKind::AnonymousPrivate,
            attrs: MemResourceAttrs::ALLOCATABLE
                | MemResourceAttrs::CACHEABLE
                | MemResourceAttrs::COHERENT,
            geometry: MemGeometry {
                base_granule: NonZeroUsize::new(4096).unwrap(),
                alloc_granule: NonZeroUsize::new(4096).unwrap(),
                protect_granule: Some(NonZeroUsize::new(4096).unwrap()),
                commit_granule: Some(NonZeroUsize::new(4096).unwrap()),
                lock_granule: Some(NonZeroUsize::new(4096).unwrap()),
                large_granule: None,
            },
            contract: MemResourceContract {
                allowed_protect: Protect::READ | Protect::WRITE,
                write_xor_execute: true,
                sharing: MemSharingPolicy::Private,
                overcommit: MemOvercommitPolicy::Disallow,
                cache_policy: CachePolicy::Default,
                integrity: None,
            },
            support: MemResourceSupport {
                protect: Protect::READ | Protect::WRITE,
                ops: MemResourceOpSet::QUERY | MemResourceOpSet::LOCK | MemResourceOpSet::COMMIT,
                advice: MemAdviceCaps::empty(),
                residency: MemResourceResidencySupport::BEST_EFFORT
                    | MemResourceResidencySupport::LOCKED,
            },
            hazards: MemResourceHazardSet::empty(),
        },
        cpu_range: Some(mock_region(64 * 1024)),
        usable_now_len: 32 * 1024,
        usable_max_len: 64 * 1024,
        state: MemResourceStateSummary {
            current_protect: MemStateValue::Uniform(Protect::READ | Protect::WRITE),
            locked: MemStateValue::Uniform(false),
            committed: MemStateValue::Uniform(true),
        },
        readiness: MemPoolResourceReadiness::RequiresStateTransition,
        origin: MemCatalogResourceOrigin::Discovered,
        topology_node: Some(fusion_pal::sys::mem::MemTopologyNodeId(node.id.0)),
    };
    let object = memory_object_from_catalog_resource(catalog_resource);
    let resource =
        memory_resource_from_catalog_resource(catalog_resource, Some(MemoryPoolClassId(7)))
            .expect("cpu-addressable catalog resource should map to provider resource");
    let strategy = memory_strategy_from_catalog_strategy(
        MemCatalogStrategy {
            id: MemCatalogStrategyId(92),
            kind: MemCatalogStrategyKind::VirtualCreate,
            domains: fusion_pal::sys::mem::MemDomainSet::VIRTUAL_ADDRESS_SPACE,
            backings: MemBackingCaps::ANON_PRIVATE,
            placements: MemPlacementCaps::ANYWHERE,
            features: fusion_pal::sys::mem::MemStrategyFeatureSupport::OVERCOMMIT_DISALLOW,
            preferences: fusion_pal::sys::mem::MemStrategyPreferenceSet::empty(),
            capacity: MemCatalogStrategyCapacity {
                min_len: 4096,
                max_len: None,
                granule: NonZeroUsize::new(4096).unwrap(),
            },
            output: Some(MemCatalogStrategyOutput {
                envelope: catalog_resource.envelope,
                readiness: MemPoolResourceReadiness::ReadyNow,
                topology_node: Some(fusion_pal::sys::mem::MemTopologyNodeId(node.id.0)),
            }),
        },
        pool_assurance(),
        Some(MemoryPoolClassId(7)),
    );

    assert_eq!(object.id, MemoryObjectId(91));
    assert!(object.is_cpu_addressable());
    assert_eq!(resource.id, MemoryResourceId(91));
    assert_eq!(resource.usable_now_len, 32 * 1024);
    assert_eq!(
        resource.readiness,
        MemoryResourceReadiness::RequiresStateTransition
    );
    assert_eq!(strategy.id, MemoryStrategyId(92));
    assert_eq!(strategy.kind, MemoryStrategyKind::VirtualCreate);
    assert_eq!(
        strategy.output.expect("strategy output").pool_class,
        Some(MemoryPoolClassId(7))
    );
}
