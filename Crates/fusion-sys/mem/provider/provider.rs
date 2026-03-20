//! Provider and topology orchestration for `fusion_sys::mem`.
//!
//! `MemoryResource` answers "what is this contiguous governed range?".
//! `MemoryProvider` answers the next set of questions that allocators and pool builders
//! actually care about:
//!
//! - What memory objects already exist on this machine or board?
//! - Which of those objects are CPU-addressable and honestly pool-capable right now?
//! - Which resources are compatible enough to live in the same `MemoryPool`?
//! - What topology or locality story does the target expose?
//! - What preparation or acquisition strategies can make more compatible pool capacity later?
//! - Which constraints are hard requirements, and which are merely preferences?
//!
//! That makes the provider layer the orchestration seam above concrete resources:
//!
//! - `VirtualMemoryResource` remains the hosted virtual-memory acquisition path.
//! - `BoundMemoryResource` remains the truthful binding path for pre-existing ranges.
//! - `MemoryProvider` inventories broader memory objects, then classifies the pool-capable
//!   subset rather than pretending every memory object is already an owned CPU range.
//!
//! The provider layer should preserve safety-critical truth rather than optimize for
//! convenience. A pool request that needs deterministic capacity, no shared aliasing, and no
//! emulated semantics should be rejected or marked provisionable only when the provider can
//! actually prove those properties. The point of this module is to keep higher layers from
//! guessing.
//!
//! Provider remains intentionally platform-independent. It should orchestrate normalized
//! memory truth surfaced through `fusion_pal::sys::mem`, not branch on Linux, Windows,
//! firmware tables, or board files directly. If the provider needs a fact and the fusion-pal cannot
//! express it, that is a fusion-pal contract gap to fix below this layer.
//!
//! The planned composition model is:
//!
//! - default discovery from the selected fusion-pal memory catalog
//! - optional explicit overlays for bound resources, strategies, and topology
//! - conflict resolution that stays explicit rather than silently picking winners
//! - pool assessment and pool provisioning plans based on the merged inventory
//!
//! What this layer should do:
//!
//! - Inventory memory objects that already exist, even when they are not currently
//!   CPU-addressable pool resources.
//! - Inventory present pool-capable resources separately from broader object inventory.
//! - Normalize mixed-readiness objects into multiple pool-resource descriptors rather than
//!   collapsing partly ready and partly preparation-required subranges into one record.
//! - Inventory acquisition strategies that may produce more resources later.
//! - Normalize topology into a pool-facing locality model.
//! - Classify which resources are compatible enough to share a pool.
//! - Assess whether a future `MemoryPool` request is ready now, provisionable, or dead.
//! - Emit provider-authored provisioning plans into caller-owned step storage.
//! - Keep hard contract violations separate from soft preference misses.
//! - Surface hazards explicitly so critical code can reject them intentionally.
//!
//! What this layer should not do:
//!
//! - It should not perform `malloc`, `free`, binning, slab management, or other allocator
//!   fast-path behavior.
//! - It should not hide unsupported platform semantics behind optimistic defaults.
//! - It should not merge resources into the same pool class when their contract, hazards,
//!   geometry, or support surface differ in a way a pool consumer would care about.
//! - It should not assume every target has useful virtual memory.
//! - It should not scrape platform-native sources directly; that translation belongs in the
//!   fusion-pal or whatever sits below it.
//!
//! This module is still a planning surface rather than a fully dynamic provider runtime. The
//! types here are meant to make the intended seams explicit before `MemoryPool` arrives and
//! calcifies the wrong assumptions.

mod assessment;
mod builder;
mod from_pal;
mod groups;
mod inventory;
mod object;
mod plan;
mod request;
mod support;
mod topology;

pub use assessment::{MemoryPoolAssessment, MemoryPoolAssessmentVerdict, assess_pool_request};
pub use builder::{
    MemoryProviderBuildSpec,
    MemoryProviderConflictPolicy,
    MemoryProviderDiscoveryPolicy,
};
pub use from_pal::{
    memory_object_from_catalog_resource,
    memory_resource_from_catalog_resource,
    memory_strategy_from_catalog_strategy,
    topology_link_from_catalog,
    topology_node_from_catalog,
};
pub use groups::{
    MemoryGroupDescriptor,
    MemoryGroupId,
    MemoryGroupWriteSummary,
    MemoryPoolCandidateGroup,
    write_candidate_groups,
    write_groups,
};
pub use inventory::{
    MemoryCompatibilityEnvelope,
    MemoryPoolClass,
    MemoryPoolClassId,
    MemoryProviderInventory,
    MemoryResourceDescriptor,
    MemoryResourceId,
    MemoryStrategyCapacity,
    MemoryStrategyDescriptor,
    MemoryStrategyId,
    MemoryStrategyKind,
    MemoryStrategyOutputDescriptor,
};
pub use object::{
    MemoryObjectDescriptor,
    MemoryObjectEnvelope,
    MemoryObjectId,
    MemoryObjectOrigin,
    MemoryResourceReadiness,
};
pub use plan::{
    MemoryPoolPlan,
    MemoryPoolPlanStep,
    MemoryPoolPlanVerdict,
    MemoryPoolPreparationKind,
    plan_pool_request,
};
pub use request::{MemoryPoolContractRequirements, MemoryPoolRequest};
pub use support::{
    CriticalSafetyRequirements,
    MemoryPoolAssessmentIssues,
    MemoryProviderCaps,
    MemoryProviderSupport,
};
pub use topology::{
    MemoryTopology,
    MemoryTopologyLink,
    MemoryTopologyLinkId,
    MemoryTopologyLinkKind,
    MemoryTopologyNode,
    MemoryTopologyNodeId,
    MemoryTopologyNodeKind,
    MemoryTopologyPreference,
};

/// Pool-facing provider contract for inventory, topology, and resource orchestration.
pub trait MemoryProvider {
    /// Returns the provider's coarse support surface.
    fn support(&self) -> MemoryProviderSupport;

    /// Returns the provider's current topology view.
    fn topology(&self) -> MemoryTopology<'_>;

    /// Returns the provider's current object, resource, strategy, and pool-class inventory.
    fn inventory(&self) -> MemoryProviderInventory<'_>;

    /// Assesses a pool request against the provider's current truth.
    ///
    /// Implementations may override this for richer planning logic, but the default behavior
    /// is intentionally conservative and derived only from the exposed inventory surfaces.
    fn assess_pool(&self, request: &MemoryPoolRequest<'_>) -> MemoryPoolAssessment {
        assess_pool_request(self.inventory(), request, self.support())
    }

    /// Writes provider-authored compatibility groups into `out_groups`.
    ///
    /// This surface exists so downstream code can inspect canonical provider grouping without
    /// re-deriving compatibility on its own.
    fn write_groups(&self, out_groups: &mut [MemoryGroupDescriptor]) -> MemoryGroupWriteSummary {
        write_groups(self.inventory(), out_groups)
    }

    /// Writes request-scoped candidate groups into `out_groups`.
    ///
    /// These are the provider-authored compatible groups that actually match the supplied
    /// request, together with their ready and transitionable capacity. The returned summary
    /// reports both canonical inventory-group count and request-matching candidate count so
    /// callers can distinguish "no groups existed" from "groups existed but none matched."
    fn write_candidate_groups(
        &self,
        request: &MemoryPoolRequest<'_>,
        out_groups: &mut [MemoryPoolCandidateGroup],
    ) -> MemoryGroupWriteSummary {
        write_candidate_groups(self.inventory(), request, out_groups)
    }

    /// Writes a provisioning plan for `request` into `out_steps`.
    ///
    /// The default plan is still intentionally conservative and derived only from the
    /// exposed inventory surfaces. Implementations may override this when they have richer
    /// knowledge about fragmentation, dynamic policy, or platform-native provisioning.
    fn plan_pool(
        &self,
        request: &MemoryPoolRequest<'_>,
        out_steps: &mut [MemoryPoolPlanStep],
    ) -> MemoryPoolPlan {
        plan_pool_request(self.inventory(), request, self.support(), out_steps)
    }
}
