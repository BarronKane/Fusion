use super::assessment::{
    MemoryPoolAssessmentVerdict, analyze_pool_request, assessment_from_analysis,
};
use super::groups::CandidateGroupKey;
use super::{
    MemoryPoolAssessmentIssues, MemoryPoolClassId, MemoryPoolRequest, MemoryProviderInventory,
    MemoryProviderSupport,
};
use crate::mem::resource::ResourceRange;

/// Coarse result of building a pool provisioning plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryPoolPlanVerdict {
    /// Present resources already satisfy the request.
    Ready,
    /// The request is not ready now, but the provider identified a preparation or
    /// acquisition plan.
    Provisionable,
    /// No honest provisioning plan exists from current provider truth.
    Rejected,
}

impl From<MemoryPoolAssessmentVerdict> for MemoryPoolPlanVerdict {
    fn from(value: MemoryPoolAssessmentVerdict) -> Self {
        match value {
            MemoryPoolAssessmentVerdict::Ready => Self::Ready,
            MemoryPoolAssessmentVerdict::Provisionable => Self::Provisionable,
            MemoryPoolAssessmentVerdict::Rejected => Self::Rejected,
        }
    }
}

/// Preparation path needed before a present resource becomes pool-usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryPoolPreparationKind {
    /// Commit or activate backing first.
    Commit,
    /// Materialize a previously descriptive or reserved range first.
    Materialize,
    /// Apply some other legal state transition first.
    StateTransition,
}

/// Single provider-authored step in a pool provisioning plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryPoolPlanStep {
    /// Consume a present resource immediately.
    UsePresentResource {
        resource_id: super::MemoryResourceId,
        range: ResourceRange,
    },
    /// Prepare a present resource, then consume the prepared range.
    PreparePresentResource {
        resource_id: super::MemoryResourceId,
        range: ResourceRange,
        preparation: MemoryPoolPreparationKind,
    },
    /// Acquire a new compatible resource through a strategy.
    CreateResource {
        strategy_id: super::MemoryStrategyId,
        range: ResourceRange,
    },
}

/// Summary of a provisioning plan written into caller-owned step storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryPoolPlan {
    /// Coarse outcome for the planning attempt.
    pub verdict: MemoryPoolPlanVerdict,
    /// Issues that prevented a direct ready result.
    pub issues: MemoryPoolAssessmentIssues,
    /// Target capacity the plan attempted to satisfy.
    pub target_capacity: usize,
    /// Total bytes the emitted steps account for.
    pub planned_bytes: usize,
    /// Total number of steps required by the plan, even when the supplied output buffer was
    /// too small to hold them all.
    pub step_count: usize,
    /// Whether the caller-supplied step buffer was too small to capture the whole plan.
    pub truncated: bool,
    /// Preferred pool class chosen for the plan when one exists.
    pub preferred_pool_class: Option<MemoryPoolClassId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct PlanAccumulator {
    remaining: usize,
    planned_bytes: usize,
    step_count: usize,
}

/// Writes a default provisioning plan for `request` into `out_steps`.
///
/// The returned summary is still meaningful when `out_steps` is too small; in that case
/// `truncated` is set and `step_count` reports the full number of required steps.
#[must_use]
pub fn plan_pool_request(
    inventory: MemoryProviderInventory<'_>,
    request: &MemoryPoolRequest<'_>,
    support: MemoryProviderSupport,
    out_steps: &mut [MemoryPoolPlanStep],
) -> MemoryPoolPlan {
    let analysis = analyze_pool_request(inventory, request, support);
    let assessment = assessment_from_analysis(&analysis);
    let Some(group) = analysis.preferred_group else {
        return MemoryPoolPlan {
            verdict: MemoryPoolPlanVerdict::Rejected,
            issues: assessment.issues,
            target_capacity: request.minimum_capacity,
            planned_bytes: 0,
            step_count: 0,
            truncated: false,
            preferred_pool_class: assessment.preferred_pool_class,
        };
    };

    let target_capacity = choose_target_capacity(group.group, request);
    let mut accumulator = PlanAccumulator {
        remaining: target_capacity,
        planned_bytes: 0,
        step_count: 0,
    };
    append_ready_resource_steps(&mut accumulator, inventory, group.key, request, out_steps);
    append_preparation_steps(&mut accumulator, inventory, group.key, request, out_steps);
    append_strategy_steps(&mut accumulator, inventory, group.key, request, out_steps);

    MemoryPoolPlan {
        verdict: MemoryPoolPlanVerdict::from(group.group.verdict),
        issues: assessment.issues,
        target_capacity,
        planned_bytes: accumulator.planned_bytes,
        step_count: accumulator.step_count,
        truncated: accumulator.step_count > out_steps.len(),
        preferred_pool_class: group
            .group
            .group
            .class_id
            .or(assessment.preferred_pool_class),
    }
}

const fn choose_target_capacity(
    group: super::MemoryPoolCandidateGroup,
    request: &MemoryPoolRequest<'_>,
) -> usize {
    if group.ready_bytes >= request.preferred_capacity
        || group.transitionable_bytes >= request.preferred_capacity
    {
        request.preferred_capacity
    } else {
        request.minimum_capacity
    }
}

fn append_ready_resource_steps(
    accumulator: &mut PlanAccumulator,
    inventory: MemoryProviderInventory<'_>,
    key: CandidateGroupKey,
    request: &MemoryPoolRequest<'_>,
    out_steps: &mut [MemoryPoolPlanStep],
) {
    if accumulator.remaining == 0 {
        return;
    }

    for resource in inventory.resources {
        if !resource_in_group(resource, key, request)
            || !request.matches_resource_ready_now(resource)
        {
            continue;
        }

        let len = core::cmp::min(accumulator.remaining, resource.usable_now_len);
        if len != 0 {
            append_step(
                accumulator,
                out_steps,
                MemoryPoolPlanStep::UsePresentResource {
                    resource_id: resource.id,
                    range: ResourceRange::whole(len),
                },
                len,
            );
        }

        if accumulator.remaining == 0 {
            break;
        }
    }
}

fn append_preparation_steps(
    accumulator: &mut PlanAccumulator,
    inventory: MemoryProviderInventory<'_>,
    key: CandidateGroupKey,
    request: &MemoryPoolRequest<'_>,
    out_steps: &mut [MemoryPoolPlanStep],
) {
    if accumulator.remaining == 0 {
        return;
    }

    for resource in inventory.resources {
        if !resource_in_group(resource, key, request)
            || request.matches_resource_ready_now(resource)
            || !request.matches_resource_transitionable(resource)
        {
            continue;
        }

        let len = core::cmp::min(accumulator.remaining, resource.usable_max_len);
        if len != 0 {
            append_step(
                accumulator,
                out_steps,
                MemoryPoolPlanStep::PreparePresentResource {
                    resource_id: resource.id,
                    range: ResourceRange::whole(len),
                    preparation: preparation_kind(resource.readiness),
                },
                len,
            );
        }

        if accumulator.remaining == 0 {
            break;
        }
    }
}

fn append_strategy_steps(
    accumulator: &mut PlanAccumulator,
    inventory: MemoryProviderInventory<'_>,
    key: CandidateGroupKey,
    request: &MemoryPoolRequest<'_>,
    out_steps: &mut [MemoryPoolPlanStep],
) {
    if accumulator.remaining == 0 {
        return;
    }

    for strategy in inventory.strategies {
        if !strategy_in_group(strategy, key, request) {
            continue;
        }

        let len = core::cmp::min(
            accumulator.remaining,
            strategy.capacity.max_len.unwrap_or(accumulator.remaining),
        );
        if len != 0 {
            append_step(
                accumulator,
                out_steps,
                MemoryPoolPlanStep::CreateResource {
                    strategy_id: strategy.id,
                    range: ResourceRange::whole(len),
                },
                len,
            );
        }

        if accumulator.remaining == 0 {
            break;
        }
    }
}

fn resource_in_group(
    resource: &super::MemoryResourceDescriptor,
    key: CandidateGroupKey,
    request: &MemoryPoolRequest<'_>,
) -> bool {
    match key {
        CandidateGroupKey::PoolClass(class_id) => {
            resource.pool_class == Some(class_id) && request.matches_resource(resource)
        }
        CandidateGroupKey::Derived(envelope, topology_node) => {
            resource.pool_class.is_none()
                && request.matches_resource(resource)
                && resource.compatibility() == envelope
                && resource.topology_node == topology_node
        }
    }
}

fn strategy_in_group(
    strategy: &super::MemoryStrategyDescriptor,
    key: CandidateGroupKey,
    request: &MemoryPoolRequest<'_>,
) -> bool {
    let Some(output) = strategy.output else {
        return false;
    };

    match key {
        CandidateGroupKey::PoolClass(class_id) => {
            output.pool_class == Some(class_id) && request.matches_strategy(strategy)
        }
        CandidateGroupKey::Derived(envelope, topology_node) => {
            output.pool_class.is_none()
                && request.matches_strategy(strategy)
                && output.envelope == envelope
                && output.topology_node == topology_node
        }
    }
}

fn push_step(out_steps: &mut [MemoryPoolPlanStep], index: usize, step: MemoryPoolPlanStep) {
    if let Some(slot) = out_steps.get_mut(index) {
        *slot = step;
    }
}

fn append_step(
    accumulator: &mut PlanAccumulator,
    out_steps: &mut [MemoryPoolPlanStep],
    step: MemoryPoolPlanStep,
    len: usize,
) {
    push_step(out_steps, accumulator.step_count, step);
    accumulator.step_count += 1;
    accumulator.planned_bytes = accumulator.planned_bytes.saturating_add(len);
    accumulator.remaining = accumulator.remaining.saturating_sub(len);
}

const fn preparation_kind(readiness: super::MemoryResourceReadiness) -> MemoryPoolPreparationKind {
    match readiness {
        super::MemoryResourceReadiness::ReadyNow
        | super::MemoryResourceReadiness::RequiresStateTransition
        | super::MemoryResourceReadiness::Unavailable => MemoryPoolPreparationKind::StateTransition,
        super::MemoryResourceReadiness::RequiresCommit => MemoryPoolPreparationKind::Commit,
        super::MemoryResourceReadiness::RequiresMaterialization => {
            MemoryPoolPreparationKind::Materialize
        }
    }
}
