use super::support::{MemoryPoolAssessmentIssues, preferred_class_id};
use super::{
    CriticalSafetyRequirements, MemoryPoolClassId, MemoryPoolRequest, MemoryProviderCaps,
    MemoryProviderInventory, MemoryProviderSupport,
};
use crate::mem::provider::support::{has_contract_candidate, has_required_support};

/// Coarse result of assessing a pool request against provider inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryPoolAssessmentVerdict {
    /// Present resources already satisfy the request.
    Ready,
    /// Present resources do not satisfy the request now, but viable strategies exist.
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
    /// Total present bytes across matching resources.
    pub matching_present_bytes: usize,
    /// Number of matching present resources.
    pub matching_resource_count: usize,
    /// Number of matching pool classes.
    pub matching_pool_class_count: usize,
    /// Number of matching acquisition strategies.
    pub matching_strategy_count: usize,
    /// First matching pool class when one is available.
    pub preferred_pool_class: Option<MemoryPoolClassId>,
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
/// captures the intended contract shape: count what is ready, classify what is compatible,
/// and fall back to "provisionable" only when a strategy can plausibly satisfy the request.
#[must_use]
pub fn assess_pool_request(
    inventory: MemoryProviderInventory<'_>,
    request: &MemoryPoolRequest<'_>,
    support: MemoryProviderSupport,
) -> MemoryPoolAssessment {
    let matching_pool_class_count = inventory
        .pool_classes
        .iter()
        .filter(|class| request.matches_pool_class(class))
        .count();
    let preferred_pool_class = preferred_class_id(inventory.pool_classes, request);
    let (matching_resource_count, matching_present_bytes) =
        matching_resource_totals(inventory.resources, request);
    let matching_strategy_count = inventory
        .strategies
        .iter()
        .filter(|strategy| request.matches_strategy(strategy))
        .count();
    let issues = assess_pool_issues(
        inventory,
        request,
        support,
        matching_resource_count,
        matching_present_bytes,
        matching_strategy_count,
    );

    let verdict =
        if matching_present_bytes >= request.minimum_capacity && matching_resource_count > 0 {
            MemoryPoolAssessmentVerdict::Ready
        } else if matching_strategy_count > 0 {
            MemoryPoolAssessmentVerdict::Provisionable
        } else {
            MemoryPoolAssessmentVerdict::Rejected
        };

    MemoryPoolAssessment {
        verdict,
        issues,
        matching_present_bytes,
        matching_resource_count,
        matching_pool_class_count,
        matching_strategy_count,
        preferred_pool_class,
    }
}

fn matching_resource_totals(
    resources: &[super::MemoryResourceDescriptor],
    request: &MemoryPoolRequest<'_>,
) -> (usize, usize) {
    let mut matching_resource_count = 0usize;
    let mut matching_present_bytes = 0usize;

    for resource in resources {
        if request.matches_resource(resource) {
            matching_resource_count += 1;
            matching_present_bytes = matching_present_bytes.saturating_add(resource.usable_len);
        }
    }

    (matching_resource_count, matching_present_bytes)
}

fn assess_pool_issues(
    inventory: MemoryProviderInventory<'_>,
    request: &MemoryPoolRequest<'_>,
    support: MemoryProviderSupport,
    matching_resource_count: usize,
    matching_present_bytes: usize,
    matching_strategy_count: usize,
) -> MemoryPoolAssessmentIssues {
    let mut issues = MemoryPoolAssessmentIssues::empty();

    if matching_resource_count == 0 {
        issues |= MemoryPoolAssessmentIssues::RESOURCE_COMPATIBILITY;
    }

    if matching_present_bytes < request.minimum_capacity {
        issues |= MemoryPoolAssessmentIssues::CAPACITY;
    }

    if matches!(
        request.topology,
        super::MemoryTopologyPreference::RequireNode(_)
    ) && matching_resource_count == 0
        && matching_strategy_count == 0
    {
        issues |= MemoryPoolAssessmentIssues::TOPOLOGY;
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
        || matching_strategy_count == 0
    {
        issues |= MemoryPoolAssessmentIssues::STRATEGY;
    }

    if request.required_safety != CriticalSafetyRequirements::empty()
        && matching_resource_count == 0
    {
        issues |= MemoryPoolAssessmentIssues::SAFETY;
    }

    if !has_required_support(inventory.resources, inventory.strategies, request) {
        issues |= MemoryPoolAssessmentIssues::SUPPORT;
    }

    if !has_contract_candidate(inventory.resources, inventory.pool_classes, request) {
        issues |= MemoryPoolAssessmentIssues::CONTRACT;
    }

    issues
}
