use super::groups::{
    CandidateGroupRecord,
    preferred_candidate_group,
};
use super::support::{
    CandidateStageCounts,
    MemoryPoolAssessmentIssues,
    candidate_stage_counts,
};
use super::{
    MemoryPoolClassId,
    MemoryPoolRequest,
    MemoryProviderCaps,
    MemoryProviderInventory,
    MemoryProviderSupport,
};

/// Coarse result of assessing a pool request against provider inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryPoolAssessmentVerdict {
    /// Present resources already satisfy the request.
    Ready,
    /// Present resources do not satisfy the request now, but viable preparation or
    /// acquisition paths exist.
    Provisionable,
    /// The provider cannot satisfy the request from current truth.
    Rejected,
}

/// Result of assessing a pool request against known resources and strategies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryPoolAssessment {
    /// Coarse outcome for the request.
    pub verdict: MemoryPoolAssessmentVerdict,
    /// Issues that prevented a direct ready result.
    pub issues: MemoryPoolAssessmentIssues,
    /// Ready-now bytes across the preferred compatible resource group.
    pub matching_present_bytes: usize,
    /// Present bytes that could become ready after legal preparation.
    pub matching_transitionable_bytes: usize,
    /// Number of compatible resources in the preferred compatible group.
    pub matching_resource_count: usize,
    /// Number of ready-now compatible resources in the preferred group.
    pub matching_ready_resource_count: usize,
    /// Number of matching pool classes across the provider inventory.
    pub matching_pool_class_count: usize,
    /// Number of matching acquisition strategies in the preferred group.
    pub matching_strategy_count: usize,
    /// Preferred matching pool class when one is available.
    pub preferred_pool_class: Option<MemoryPoolClassId>,
}

/// Internal summary reused by both assessment and planning so provider analysis does not
/// redundantly enumerate candidate groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PoolAssessmentAnalysis {
    pub matching_pool_class_count: usize,
    pub preferred_group: Option<CandidateGroupRecord>,
    pub issues: MemoryPoolAssessmentIssues,
}

impl MemoryPoolAssessment {
    /// Returns `true` when the request is immediately satisfiable from present resources.
    #[must_use]
    pub const fn is_ready(self) -> bool {
        matches!(self.verdict, MemoryPoolAssessmentVerdict::Ready)
    }

    /// Returns `true` when the request is not ready now but could be provisioned later.
    #[must_use]
    pub const fn is_provisionable(self) -> bool {
        matches!(self.verdict, MemoryPoolAssessmentVerdict::Provisionable)
    }
}

/// Default pool-assessment routine for provider inventories.
///
/// Providers may override this logic when they have richer knowledge, but this function
/// captures the intended contract shape: classify compatible groups, aggregate ready and
/// preparation-required capacity across them, and only report "provisionable" when a
/// present preparation path or explicit acquisition strategy still exists.
#[must_use]
pub fn assess_pool_request(
    inventory: MemoryProviderInventory<'_>,
    request: &MemoryPoolRequest<'_>,
    support: MemoryProviderSupport,
) -> MemoryPoolAssessment {
    let analysis = analyze_pool_request(inventory, request, support);
    assessment_from_analysis(&analysis)
}

#[must_use]
pub(super) fn analyze_pool_request(
    inventory: MemoryProviderInventory<'_>,
    request: &MemoryPoolRequest<'_>,
    support: MemoryProviderSupport,
) -> PoolAssessmentAnalysis {
    let stage_counts = candidate_stage_counts(inventory, request);
    let matching_pool_class_count = inventory
        .pool_classes
        .iter()
        .filter(|class| request.matches_pool_class(class))
        .count();
    let preferred_group = preferred_candidate_group(inventory, request);
    let issues = assess_pool_issues(
        inventory,
        request,
        support,
        stage_counts,
        preferred_group.as_ref(),
    );

    PoolAssessmentAnalysis {
        matching_pool_class_count,
        preferred_group,
        issues,
    }
}

#[must_use]
pub(super) fn assessment_from_analysis(analysis: &PoolAssessmentAnalysis) -> MemoryPoolAssessment {
    let preferred_pool_class = analysis
        .preferred_group
        .and_then(|record| record.group.group.class_id);
    let verdict = analysis
        .preferred_group
        .map_or(MemoryPoolAssessmentVerdict::Rejected, |record| {
            record.group.verdict
        });

    MemoryPoolAssessment {
        verdict,
        issues: analysis.issues,
        matching_present_bytes: analysis
            .preferred_group
            .map_or(0, |record| record.group.ready_bytes),
        matching_transitionable_bytes: analysis
            .preferred_group
            .map_or(0, |record| record.group.transitionable_bytes),
        matching_resource_count: analysis
            .preferred_group
            .map_or(0, |record| record.group.matching_resource_count),
        matching_ready_resource_count: analysis
            .preferred_group
            .map_or(0, |record| record.group.matching_ready_resource_count),
        matching_pool_class_count: analysis.matching_pool_class_count,
        matching_strategy_count: analysis
            .preferred_group
            .map_or(0, |record| record.group.matching_strategy_count),
        preferred_pool_class,
    }
}

fn assess_pool_issues(
    inventory: MemoryProviderInventory<'_>,
    request: &MemoryPoolRequest<'_>,
    support: MemoryProviderSupport,
    stage_counts: CandidateStageCounts,
    preferred_group: Option<&super::groups::CandidateGroupRecord>,
) -> MemoryPoolAssessmentIssues {
    let mut issues = MemoryPoolAssessmentIssues::empty();

    if matches!(
        request.topology,
        super::MemoryTopologyPreference::RequireNode(_)
    ) && stage_counts.topology_agnostic_objects != 0
        && stage_counts.compatible_objects == 0
    {
        issues |= MemoryPoolAssessmentIssues::TOPOLOGY;
    }

    if stage_counts.compatible_resources == 0
        && stage_counts.compatible_strategies == 0
        && stage_counts.compatible_classes == 0
    {
        issues |= MemoryPoolAssessmentIssues::RESOURCE_COMPATIBILITY;
    }

    if (stage_counts.compatible_resources != 0
        || stage_counts.compatible_strategies != 0
        || stage_counts.compatible_classes != 0)
        && stage_counts.contract_resources == 0
        && stage_counts.contract_strategies == 0
        && stage_counts.contract_classes == 0
    {
        issues |= MemoryPoolAssessmentIssues::CONTRACT;
    }

    if (stage_counts.contract_resources != 0
        || stage_counts.contract_strategies != 0
        || stage_counts.contract_classes != 0)
        && stage_counts.safety_resources == 0
        && stage_counts.safety_strategies == 0
        && stage_counts.safety_classes == 0
    {
        issues |= MemoryPoolAssessmentIssues::SAFETY;
    }

    if (stage_counts.safety_resources != 0 || stage_counts.safety_strategies != 0)
        && stage_counts.support_resources == 0
        && stage_counts.support_strategies == 0
    {
        issues |= MemoryPoolAssessmentIssues::SUPPORT;
    }

    if let Some(record) = preferred_group {
        if record.group.ready_bytes < request.minimum_capacity {
            issues |= MemoryPoolAssessmentIssues::CAPACITY;
        }

        if record.group.ready_bytes < request.minimum_capacity
            && record.group.transitionable_bytes >= request.minimum_capacity
        {
            issues |= MemoryPoolAssessmentIssues::STATE;
        }

        if record.group.verdict != MemoryPoolAssessmentVerdict::Ready
            && record.group.matching_strategy_count == 0
        {
            issues |= MemoryPoolAssessmentIssues::STRATEGY;
        }
    } else {
        issues |= MemoryPoolAssessmentIssues::CAPACITY | MemoryPoolAssessmentIssues::STRATEGY;
    }

    if !support
        .caps
        .contains(MemoryProviderCaps::EXHAUSTIVE_INVENTORY)
    {
        issues |= MemoryPoolAssessmentIssues::INCOMPLETE_INVENTORY;
    }

    if !support
        .caps
        .contains(MemoryProviderCaps::STRATEGY_INVENTORY)
        && !inventory.strategies.is_empty()
    {
        issues |= MemoryPoolAssessmentIssues::INCOMPLETE_INVENTORY;
    }

    issues
}
