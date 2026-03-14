//! Adapter helpers for translating normalized fusion-pal catalog truth into provider descriptors.
//!
//! These helpers are intentionally explicit. Provider and fusion-pal keep distinct type systems so
//! the orchestration layer can evolve without smuggling backend assumptions through shared
//! structs wearing fake mustaches.

use fusion_pal::sys::mem::{
    MemCatalogResource, MemCatalogStrategy, MemCatalogStrategyOutput, MemDomain, MemGeometry,
    MemOvercommitPolicy, MemPoolResourceReadiness, MemResourceBackingKind, MemResourceContract,
    MemResourceEnvelope, MemResourceStateSummary, MemResourceSupport, MemSharingPolicy,
    MemStateValue, MemTopologyLink, MemTopologyNode,
};

use super::{
    CriticalSafetyRequirements, MemoryCompatibilityEnvelope, MemoryObjectDescriptor,
    MemoryObjectEnvelope, MemoryObjectId, MemoryObjectOrigin, MemoryPoolClassId,
    MemoryResourceDescriptor, MemoryResourceId, MemoryResourceReadiness, MemoryStrategyCapacity,
    MemoryStrategyDescriptor, MemoryStrategyId, MemoryStrategyKind, MemoryStrategyOutputDescriptor,
    MemoryTopologyLinkId, MemoryTopologyLinkKind, MemoryTopologyNode, MemoryTopologyNodeId,
    MemoryTopologyNodeKind,
};
use crate::mem::resource::{
    IntegrityConstraints, MemoryDomain, MemoryGeometry, OvercommitPolicy, ResourceAcquireSupport,
    ResourceAttrs, ResourceBackingKind, ResourceFeatureSupport, ResourceHazardSet, ResourceInfo,
    ResourceOpSet, ResourcePreferenceSet, ResourceResidencySupport, ResourceState, ResourceSupport,
    SharingPolicy, StateValue,
};

/// Converts a fusion-pal catalog resource into a broad provider memory-object descriptor.
#[must_use]
pub fn memory_object_from_catalog_resource(resource: MemCatalogResource) -> MemoryObjectDescriptor {
    MemoryObjectDescriptor {
        id: MemoryObjectId(resource.id.0),
        envelope: object_envelope_from_catalog(resource.envelope),
        cpu_range: resource.cpu_range,
        origin: object_origin_from_catalog(resource.origin),
        usable_len: resource.usable_max_len,
        topology_node: resource.topology_node.map(|id| MemoryTopologyNodeId(id.0)),
    }
}

/// Converts a fusion-pal catalog resource into a pool-capable provider resource descriptor.
///
/// Returns `None` when the catalog resource is not CPU-addressable in the current execution
/// context.
#[must_use]
pub fn memory_resource_from_catalog_resource(
    resource: MemCatalogResource,
    pool_class: Option<MemoryPoolClassId>,
) -> Option<MemoryResourceDescriptor> {
    let range = resource.cpu_range?;
    let info = ResourceInfo::new(
        range,
        domain_from_catalog(resource.envelope.domain),
        backing_from_catalog(resource.envelope.backing),
        ResourceAttrs::from_bits_retain(resource.envelope.attrs.bits()),
        geometry_from_catalog(resource.envelope.geometry),
        contract_from_catalog(resource.envelope.contract),
        support_from_catalog(resource.envelope.support),
        ResourceHazardSet::from_bits_retain(resource.envelope.hazards.bits()),
    );

    Some(MemoryResourceDescriptor {
        id: MemoryResourceId(resource.id.0),
        object_id: Some(MemoryObjectId(resource.id.0)),
        info,
        state: state_from_catalog(resource.state),
        origin: object_origin_from_catalog(resource.origin),
        usable_now_len: resource.usable_now_len,
        usable_max_len: resource.usable_max_len,
        readiness: readiness_from_catalog(resource.readiness),
        topology_node: resource.topology_node.map(|id| MemoryTopologyNodeId(id.0)),
        pool_class,
    })
}

/// Converts a fusion-pal catalog strategy into a provider strategy descriptor.
#[must_use]
pub fn memory_strategy_from_catalog_strategy(
    strategy: MemCatalogStrategy,
    assurance: CriticalSafetyRequirements,
    pool_class: Option<MemoryPoolClassId>,
) -> MemoryStrategyDescriptor {
    MemoryStrategyDescriptor {
        id: MemoryStrategyId(strategy.id.0),
        kind: strategy_kind_from_catalog(strategy.kind),
        acquire: ResourceAcquireSupport {
            domains: crate::mem::resource::MemoryDomainSet::from_bits_retain(
                strategy.domains.bits(),
            ),
            backings: strategy.backings,
            placements: strategy.placements,
            instance: strategy.output.map_or(empty_resource_support(), |output| {
                support_from_catalog(output.envelope.support)
            }),
            features: ResourceFeatureSupport::from_bits_retain(strategy.features.bits()),
            preferences: ResourcePreferenceSet::from_bits_retain(strategy.preferences.bits()),
        },
        capacity: MemoryStrategyCapacity {
            min_len: strategy.capacity.min_len,
            max_len: strategy.capacity.max_len,
            granule: strategy.capacity.granule,
        },
        output: strategy
            .output
            .map(|output| output_descriptor_from_catalog(output, assurance, pool_class)),
    }
}

/// Converts a fusion-pal topology node into the provider topology vocabulary.
#[must_use]
pub fn topology_node_from_catalog(node: MemTopologyNode) -> MemoryTopologyNode {
    MemoryTopologyNode {
        id: MemoryTopologyNodeId(node.id.0),
        kind: match node.kind {
            fusion_pal::sys::mem::MemTopologyNodeKind::Machine => MemoryTopologyNodeKind::Machine,
            fusion_pal::sys::mem::MemTopologyNodeKind::Package => MemoryTopologyNodeKind::Package,
            fusion_pal::sys::mem::MemTopologyNodeKind::NumaNode => MemoryTopologyNodeKind::NumaNode,
            fusion_pal::sys::mem::MemTopologyNodeKind::MemoryController => {
                MemoryTopologyNodeKind::MemoryController
            }
            fusion_pal::sys::mem::MemTopologyNodeKind::Device => MemoryTopologyNodeKind::Device,
            fusion_pal::sys::mem::MemTopologyNodeKind::BoardRegion => {
                MemoryTopologyNodeKind::BoardRegion
            }
        },
        parent: node.parent.map(|id| MemoryTopologyNodeId(id.0)),
        domains: crate::mem::resource::MemoryDomainSet::from_bits_retain(node.domains.bits()),
    }
}

/// Converts a fusion-pal topology link into the provider topology vocabulary.
#[must_use]
pub const fn topology_link_from_catalog(link: MemTopologyLink) -> super::MemoryTopologyLink {
    super::MemoryTopologyLink {
        id: MemoryTopologyLinkId(link.id.0),
        from: MemoryTopologyNodeId(link.from.0),
        to: MemoryTopologyNodeId(link.to.0),
        kind: match link.kind {
            fusion_pal::sys::mem::MemTopologyLinkKind::ParentChild => {
                MemoryTopologyLinkKind::ParentChild
            }
            fusion_pal::sys::mem::MemTopologyLinkKind::AccessPath => {
                MemoryTopologyLinkKind::AccessPath
            }
            fusion_pal::sys::mem::MemTopologyLinkKind::Coherency => {
                MemoryTopologyLinkKind::Coherency
            }
        },
        distance: link.distance,
        bandwidth_bytes_per_sec: link.bandwidth_bytes_per_sec,
    }
}

fn output_descriptor_from_catalog(
    output: MemCatalogStrategyOutput,
    assurance: CriticalSafetyRequirements,
    pool_class: Option<MemoryPoolClassId>,
) -> MemoryStrategyOutputDescriptor {
    MemoryStrategyOutputDescriptor {
        envelope: compatibility_envelope_from_catalog(output.envelope),
        readiness: readiness_from_catalog(output.readiness),
        assurance,
        topology_node: output.topology_node.map(|id| MemoryTopologyNodeId(id.0)),
        pool_class,
    }
}

fn object_envelope_from_catalog(envelope: MemResourceEnvelope) -> MemoryObjectEnvelope {
    MemoryObjectEnvelope {
        domain: domain_from_catalog(envelope.domain),
        backing: backing_from_catalog(envelope.backing),
        attrs: ResourceAttrs::from_bits_retain(envelope.attrs.bits()),
        contract: contract_from_catalog(envelope.contract),
        support: support_from_catalog(envelope.support),
        hazards: ResourceHazardSet::from_bits_retain(envelope.hazards.bits()),
    }
}

fn compatibility_envelope_from_catalog(
    envelope: MemResourceEnvelope,
) -> MemoryCompatibilityEnvelope {
    MemoryCompatibilityEnvelope {
        domain: domain_from_catalog(envelope.domain),
        backing: backing_from_catalog(envelope.backing),
        attrs: ResourceAttrs::from_bits_retain(envelope.attrs.bits()),
        geometry: geometry_from_catalog(envelope.geometry),
        contract: contract_from_catalog(envelope.contract),
        support: support_from_catalog(envelope.support),
        hazards: ResourceHazardSet::from_bits_retain(envelope.hazards.bits()),
    }
}

const fn domain_from_catalog(domain: MemDomain) -> MemoryDomain {
    match domain {
        MemDomain::VirtualAddressSpace => MemoryDomain::VirtualAddressSpace,
        MemDomain::DeviceLocal => MemoryDomain::DeviceLocal,
        MemDomain::Physical => MemoryDomain::Physical,
        MemDomain::StaticRegion => MemoryDomain::StaticRegion,
        MemDomain::Mmio => MemoryDomain::Mmio,
    }
}

const fn backing_from_catalog(backing: MemResourceBackingKind) -> ResourceBackingKind {
    match backing {
        MemResourceBackingKind::AnonymousPrivate => ResourceBackingKind::AnonymousPrivate,
        MemResourceBackingKind::AnonymousShared => ResourceBackingKind::AnonymousShared,
        MemResourceBackingKind::FilePrivate => ResourceBackingKind::FilePrivate,
        MemResourceBackingKind::FileShared => ResourceBackingKind::FileShared,
        MemResourceBackingKind::Borrowed => ResourceBackingKind::Borrowed,
        MemResourceBackingKind::StaticRegion => ResourceBackingKind::StaticRegion,
        MemResourceBackingKind::Partition => ResourceBackingKind::Partition,
        MemResourceBackingKind::DeviceLocal => ResourceBackingKind::DeviceLocal,
        MemResourceBackingKind::Physical => ResourceBackingKind::Physical,
        MemResourceBackingKind::Mmio => ResourceBackingKind::Mmio,
    }
}

const fn geometry_from_catalog(geometry: MemGeometry) -> MemoryGeometry {
    MemoryGeometry {
        base_granule: geometry.base_granule,
        alloc_granule: geometry.alloc_granule,
        protect_granule: geometry.protect_granule,
        commit_granule: geometry.commit_granule,
        lock_granule: geometry.lock_granule,
        large_granule: geometry.large_granule,
    }
}

fn contract_from_catalog(contract: MemResourceContract) -> crate::mem::resource::ResourceContract {
    crate::mem::resource::ResourceContract {
        allowed_protect: contract.allowed_protect,
        write_xor_execute: contract.write_xor_execute,
        sharing: match contract.sharing {
            MemSharingPolicy::Private => SharingPolicy::Private,
            MemSharingPolicy::Shared => SharingPolicy::Shared,
        },
        overcommit: match contract.overcommit {
            MemOvercommitPolicy::Allow => OvercommitPolicy::Allow,
            MemOvercommitPolicy::Disallow => OvercommitPolicy::Disallow,
        },
        cache_policy: contract.cache_policy,
        integrity: contract.integrity.map(|integrity| IntegrityConstraints {
            mode: integrity.mode,
            tag: integrity.tag,
        }),
    }
}

const fn support_from_catalog(support: MemResourceSupport) -> ResourceSupport {
    ResourceSupport {
        protect: support.protect,
        ops: ResourceOpSet::from_bits_retain(support.ops.bits()),
        advice: support.advice,
        residency: ResourceResidencySupport::from_bits_retain(support.residency.bits()),
    }
}

fn state_from_catalog(state: MemResourceStateSummary) -> ResourceState {
    ResourceState::snapshot(
        state_value_from_catalog(state.current_protect),
        state_value_from_catalog(state.locked),
        state_value_from_catalog(state.committed),
    )
}

fn state_value_from_catalog<T>(value: MemStateValue<T>) -> StateValue<T> {
    match value {
        MemStateValue::Uniform(value) => StateValue::Uniform(value),
        MemStateValue::Asymmetric => StateValue::Asymmetric,
        MemStateValue::Unknown => StateValue::Unknown,
    }
}

const fn readiness_from_catalog(readiness: MemPoolResourceReadiness) -> MemoryResourceReadiness {
    match readiness {
        MemPoolResourceReadiness::ReadyNow => MemoryResourceReadiness::ReadyNow,
        MemPoolResourceReadiness::RequiresCommit => MemoryResourceReadiness::RequiresCommit,
        MemPoolResourceReadiness::RequiresMaterialization => {
            MemoryResourceReadiness::RequiresMaterialization
        }
        MemPoolResourceReadiness::RequiresStateTransition => {
            MemoryResourceReadiness::RequiresStateTransition
        }
        MemPoolResourceReadiness::Unavailable => MemoryResourceReadiness::Unavailable,
    }
}

const fn object_origin_from_catalog(
    origin: fusion_pal::sys::mem::MemCatalogResourceOrigin,
) -> MemoryObjectOrigin {
    match origin {
        fusion_pal::sys::mem::MemCatalogResourceOrigin::Discovered => {
            MemoryObjectOrigin::Discovered
        }
        fusion_pal::sys::mem::MemCatalogResourceOrigin::Created => MemoryObjectOrigin::Created,
        fusion_pal::sys::mem::MemCatalogResourceOrigin::Borrowed => MemoryObjectOrigin::Borrowed,
        fusion_pal::sys::mem::MemCatalogResourceOrigin::Materialized => {
            MemoryObjectOrigin::Materialized
        }
    }
}

const fn strategy_kind_from_catalog(
    kind: fusion_pal::sys::mem::MemCatalogStrategyKind,
) -> MemoryStrategyKind {
    match kind {
        fusion_pal::sys::mem::MemCatalogStrategyKind::VirtualCreate => {
            MemoryStrategyKind::VirtualCreate
        }
        fusion_pal::sys::mem::MemCatalogStrategyKind::BindExisting => {
            MemoryStrategyKind::BindExisting
        }
        fusion_pal::sys::mem::MemCatalogStrategyKind::ReservationMaterialize => {
            MemoryStrategyKind::ReservationMaterialize
        }
        fusion_pal::sys::mem::MemCatalogStrategyKind::PhysicalMap => {
            MemoryStrategyKind::PhysicalMap
        }
        fusion_pal::sys::mem::MemCatalogStrategyKind::DeviceMap => MemoryStrategyKind::DeviceMap,
        fusion_pal::sys::mem::MemCatalogStrategyKind::NativePool => MemoryStrategyKind::NativePool,
    }
}

const fn empty_resource_support() -> ResourceSupport {
    ResourceSupport {
        protect: fusion_pal::sys::mem::Protect::NONE,
        ops: ResourceOpSet::empty(),
        advice: fusion_pal::sys::mem::MemAdviceCaps::empty(),
        residency: ResourceResidencySupport::empty(),
    }
}
