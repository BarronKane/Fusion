use super::{
    MemoryCompatibilityEnvelope, MemoryPoolAssessmentVerdict, MemoryPoolClassId, MemoryPoolRequest,
    MemoryProviderInventory, MemoryTopologyNodeId,
};

/// Ephemeral identifier for a provider-authored compatibility group within one inventory
/// snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryGroupId(pub usize);

/// Provider-authored compatibility group over pool-capable resources and strategies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryGroupDescriptor {
    /// Ephemeral group identifier valid only for the current inventory snapshot.
    pub id: MemoryGroupId,
    /// Precomputed pool class backing this group when one exists.
    pub class_id: Option<MemoryPoolClassId>,
    /// Shared pool-visible compatibility envelope for the group.
    pub envelope: MemoryCompatibilityEnvelope,
    /// Optional topology node characterizing the group.
    pub topology_node: Option<MemoryTopologyNodeId>,
    /// Number of present resources in the group.
    pub resource_count: usize,
    /// Number of strategies that can produce members of the group.
    pub strategy_count: usize,
}

/// Request-scoped view of one provider-authored compatibility group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryPoolCandidateGroup {
    /// Underlying provider-authored compatibility group.
    pub group: MemoryGroupDescriptor,
    /// Number of present resources in the group that match the request.
    pub matching_resource_count: usize,
    /// Number of immediately ready present resources in the group that match the request.
    pub matching_ready_resource_count: usize,
    /// Number of strategies in the group that match the request.
    pub matching_strategy_count: usize,
    /// Immediately usable bytes across the matching ready resources.
    pub ready_bytes: usize,
    /// Bytes that could become usable after legal preparation or acquisition.
    pub transitionable_bytes: usize,
    /// Coarse request verdict for this one group.
    pub verdict: MemoryPoolAssessmentVerdict,
}

/// Summary of writing provider-authored groups into caller-owned storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryGroupWriteSummary {
    /// Total number of canonical inventory groups visible to the provider.
    pub inventory_groups: usize,
    /// Number of groups relevant to the current write operation.
    ///
    /// For `write_groups`, this equals `inventory_groups`. For
    /// `write_candidate_groups`, this counts only the groups that matched the supplied
    /// request well enough to produce a candidate-group record.
    pub matching_groups: usize,
    /// Number of groups actually written into caller-owned storage.
    pub written_groups: usize,
    /// Whether the supplied output buffer was too small to hold every group.
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum CandidateGroupKey {
    PoolClass(MemoryPoolClassId),
    Derived(MemoryCompatibilityEnvelope, Option<MemoryTopologyNodeId>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct InventoryGroupRecord {
    pub descriptor: MemoryGroupDescriptor,
    pub key: CandidateGroupKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct CandidateGroupRecord {
    pub group: MemoryPoolCandidateGroup,
    pub key: CandidateGroupKey,
}

/// Writes every provider-authored compatibility group into `out_groups`.
#[must_use]
pub fn write_groups(
    inventory: MemoryProviderInventory<'_>,
    out_groups: &mut [MemoryGroupDescriptor],
) -> MemoryGroupWriteSummary {
    let mut inventory_groups = 0usize;
    let mut written_groups = 0usize;

    for_each_inventory_group(inventory, |record| {
        inventory_groups += 1;
        if let Some(slot) = out_groups.get_mut(written_groups) {
            *slot = record.descriptor;
            written_groups += 1;
        }
    });

    MemoryGroupWriteSummary {
        inventory_groups,
        matching_groups: inventory_groups,
        written_groups,
        truncated: inventory_groups > written_groups,
    }
}

/// Writes every request-scoped candidate group into `out_groups`.
#[must_use]
pub fn write_candidate_groups(
    inventory: MemoryProviderInventory<'_>,
    request: &MemoryPoolRequest<'_>,
    out_groups: &mut [MemoryPoolCandidateGroup],
) -> MemoryGroupWriteSummary {
    let mut inventory_groups = 0usize;
    let mut matching_groups = 0usize;
    let mut written_groups = 0usize;

    for_each_inventory_group(inventory, |record| {
        inventory_groups += 1;
        let Some(candidate) = candidate_group_for_record(record, inventory, request) else {
            return;
        };

        matching_groups += 1;
        if let Some(slot) = out_groups.get_mut(written_groups) {
            *slot = candidate.group;
            written_groups += 1;
        }
    });

    MemoryGroupWriteSummary {
        inventory_groups,
        matching_groups,
        written_groups,
        truncated: matching_groups > written_groups,
    }
}

pub(super) fn preferred_candidate_group(
    inventory: MemoryProviderInventory<'_>,
    request: &MemoryPoolRequest<'_>,
) -> Option<CandidateGroupRecord> {
    let mut best = None;

    for_each_inventory_group(inventory, |record| {
        let Some(candidate) = candidate_group_for_record(record, inventory, request) else {
            return;
        };

        choose_better_candidate(&mut best, &candidate, request);
    });

    best
}

fn choose_better_candidate(
    current: &mut Option<CandidateGroupRecord>,
    candidate: &CandidateGroupRecord,
    request: &MemoryPoolRequest<'_>,
) {
    let should_replace = current.as_ref().is_none_or(|existing| {
        group_rank(candidate.group, request) > group_rank(existing.group, request)
    });

    if should_replace {
        *current = Some(*candidate);
    }
}

fn group_rank(
    group: MemoryPoolCandidateGroup,
    request: &MemoryPoolRequest<'_>,
) -> (u8, usize, usize, usize, usize) {
    let verdict_rank = match group.verdict {
        MemoryPoolAssessmentVerdict::Ready => 2,
        MemoryPoolAssessmentVerdict::Provisionable => 1,
        MemoryPoolAssessmentVerdict::Rejected => 0,
    };
    let preferred_ready = core::cmp::min(group.ready_bytes, request.preferred_capacity);
    let preferred_transitionable =
        core::cmp::min(group.transitionable_bytes, request.preferred_capacity);

    (
        verdict_rank,
        preferred_ready,
        preferred_transitionable,
        group.matching_strategy_count,
        usize::MAX - group.matching_resource_count,
    )
}

pub(super) fn for_each_inventory_group(
    inventory: MemoryProviderInventory<'_>,
    mut f: impl FnMut(InventoryGroupRecord),
) {
    let mut next_id = 0usize;
    emit_class_groups(inventory, &mut next_id, &mut f);
    emit_unclassed_resource_groups(inventory, &mut next_id, &mut f);
    emit_unclassed_strategy_groups(inventory, &mut next_id, &mut f);
}

fn emit_class_groups(
    inventory: MemoryProviderInventory<'_>,
    next_id: &mut usize,
    f: &mut impl FnMut(InventoryGroupRecord),
) {
    for class in inventory.pool_classes {
        let descriptor = MemoryGroupDescriptor {
            id: MemoryGroupId(*next_id),
            class_id: Some(class.id),
            envelope: class.envelope,
            topology_node: class.topology_node,
            resource_count: inventory
                .resources
                .iter()
                .filter(|resource| class.accepts(resource))
                .count(),
            strategy_count: inventory
                .strategies
                .iter()
                .filter(|strategy| class.accepts_strategy(strategy))
                .count(),
        };

        *next_id = next_id.saturating_add(1);
        f(InventoryGroupRecord {
            descriptor,
            key: CandidateGroupKey::PoolClass(class.id),
        });
    }
}

fn emit_unclassed_resource_groups(
    inventory: MemoryProviderInventory<'_>,
    next_id: &mut usize,
    f: &mut impl FnMut(InventoryGroupRecord),
) {
    for (index, resource) in inventory.resources.iter().enumerate() {
        if resource.pool_class.is_some() {
            continue;
        }

        let key = CandidateGroupKey::Derived(resource.compatibility(), resource.topology_node);
        if inventory.resources[..index].iter().any(|other| {
            other.pool_class.is_none()
                && CandidateGroupKey::Derived(other.compatibility(), other.topology_node) == key
        }) {
            continue;
        }

        let descriptor = MemoryGroupDescriptor {
            id: MemoryGroupId(*next_id),
            class_id: None,
            envelope: resource.compatibility(),
            topology_node: resource.topology_node,
            resource_count: inventory
                .resources
                .iter()
                .filter(|other| {
                    other.pool_class.is_none()
                        && other.compatibility() == resource.compatibility()
                        && other.topology_node == resource.topology_node
                })
                .count(),
            strategy_count: inventory
                .strategies
                .iter()
                .filter(|strategy| {
                    strategy.output.is_some_and(|output| {
                        output.pool_class.is_none()
                            && output.envelope == resource.compatibility()
                            && output.topology_node == resource.topology_node
                    })
                })
                .count(),
        };

        *next_id = next_id.saturating_add(1);
        f(InventoryGroupRecord { descriptor, key });
    }
}

fn emit_unclassed_strategy_groups(
    inventory: MemoryProviderInventory<'_>,
    next_id: &mut usize,
    f: &mut impl FnMut(InventoryGroupRecord),
) {
    for (index, strategy) in inventory.strategies.iter().enumerate() {
        let Some(output) = strategy.output else {
            continue;
        };

        if output.pool_class.is_some() {
            continue;
        }

        let key = CandidateGroupKey::Derived(output.envelope, output.topology_node);
        let seen_in_resources = inventory.resources.iter().any(|resource| {
            resource.pool_class.is_none()
                && CandidateGroupKey::Derived(resource.compatibility(), resource.topology_node)
                    == key
        });
        let seen_in_prior_strategies = inventory.strategies[..index].iter().any(|other| {
            other.output.is_some_and(|other_output| {
                other_output.pool_class.is_none()
                    && CandidateGroupKey::Derived(other_output.envelope, other_output.topology_node)
                        == key
            })
        });
        if seen_in_resources || seen_in_prior_strategies {
            continue;
        }

        let descriptor = MemoryGroupDescriptor {
            id: MemoryGroupId(*next_id),
            class_id: None,
            envelope: output.envelope,
            topology_node: output.topology_node,
            resource_count: 0,
            strategy_count: inventory
                .strategies
                .iter()
                .filter(|other| {
                    other.output.is_some_and(|other_output| {
                        other_output.pool_class.is_none()
                            && other_output.envelope == output.envelope
                            && other_output.topology_node == output.topology_node
                    })
                })
                .count(),
        };

        *next_id = next_id.saturating_add(1);
        f(InventoryGroupRecord { descriptor, key });
    }
}

fn candidate_group_for_record(
    record: InventoryGroupRecord,
    inventory: MemoryProviderInventory<'_>,
    request: &MemoryPoolRequest<'_>,
) -> Option<CandidateGroupRecord> {
    let mut matching_resource_count = 0usize;
    let mut matching_ready_resource_count = 0usize;
    let mut matching_strategy_count = 0usize;
    let mut ready_bytes = 0usize;
    let mut transitionable_bytes = 0usize;

    for resource in inventory.resources {
        if !resource_belongs_to_group(resource, record.key) || !request.matches_resource(resource) {
            continue;
        }

        matching_resource_count += 1;

        if request.matches_resource_ready_now(resource) {
            matching_ready_resource_count += 1;
            ready_bytes = ready_bytes.saturating_add(resource.usable_now_len);
        }

        if request.matches_resource_transitionable(resource) {
            transitionable_bytes = transitionable_bytes.saturating_add(resource.usable_max_len);
        }
    }

    for strategy in inventory.strategies {
        if strategy_belongs_to_group(strategy, record.key) && request.matches_strategy(strategy) {
            matching_strategy_count += 1;
        }
    }

    if matching_resource_count == 0 && matching_strategy_count == 0 {
        return None;
    }

    let verdict = if ready_bytes >= request.minimum_capacity && matching_ready_resource_count != 0 {
        MemoryPoolAssessmentVerdict::Ready
    } else if transitionable_bytes != 0 || matching_strategy_count != 0 {
        MemoryPoolAssessmentVerdict::Provisionable
    } else {
        MemoryPoolAssessmentVerdict::Rejected
    };

    Some(CandidateGroupRecord {
        key: record.key,
        group: MemoryPoolCandidateGroup {
            group: record.descriptor,
            matching_resource_count,
            matching_ready_resource_count,
            matching_strategy_count,
            ready_bytes,
            transitionable_bytes,
            verdict,
        },
    })
}

fn resource_belongs_to_group(
    resource: &super::MemoryResourceDescriptor,
    key: CandidateGroupKey,
) -> bool {
    match key {
        CandidateGroupKey::PoolClass(class_id) => resource.pool_class == Some(class_id),
        CandidateGroupKey::Derived(envelope, topology_node) => {
            resource.pool_class.is_none()
                && resource.compatibility() == envelope
                && resource.topology_node == topology_node
        }
    }
}

fn strategy_belongs_to_group(
    strategy: &super::MemoryStrategyDescriptor,
    key: CandidateGroupKey,
) -> bool {
    let Some(output) = strategy.output else {
        return false;
    };

    match key {
        CandidateGroupKey::PoolClass(class_id) => output.pool_class == Some(class_id),
        CandidateGroupKey::Derived(envelope, topology_node) => {
            output.pool_class.is_none()
                && output.envelope == envelope
                && output.topology_node == topology_node
        }
    }
}
