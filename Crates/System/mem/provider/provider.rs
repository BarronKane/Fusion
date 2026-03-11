//! Provider and topology orchestration for `fusion_sys::mem`.
//!
//! `MemoryResource` answers "what is this contiguous governed range?".
//! `MemoryProvider` answers the next set of questions that allocators and pool builders
//! actually care about:
//!
//! - What resources already exist on this machine or board?
//! - Which of those resources are allocator-usable?
//! - Which resources are compatible enough to live in the same `MemoryPool`?
//! - What topology or locality story does the target expose?
//! - What acquisition strategies can create additional compatible resources later?
//! - Which constraints are hard requirements, and which are merely preferences?
//!
//! That makes the provider layer the orchestration seam above concrete resources:
//!
//! - `VirtualMemoryResource` remains the hosted virtual-memory acquisition path.
//! - `BoundMemoryResource` remains the truthful binding path for pre-existing ranges.
//! - `MemoryProvider` inventories those concrete resource stories and classifies them for
//!   pool composition.
//!
//! The provider layer should preserve safety-critical truth rather than optimize for
//! convenience. A pool request that needs deterministic capacity, no shared aliasing, and no
//! emulated semantics should be rejected or marked provisionable only when the provider can
//! actually prove those properties. The point of this module is to keep higher layers from
//! guessing.
//!
//! Provider remains intentionally platform-independent. It should orchestrate normalized
//! memory truth surfaced through `fusion_pal::sys::mem`, not branch on Linux, Windows,
//! firmware tables, or board files directly. If the provider needs a fact and the PAL cannot
//! express it, that is a PAL contract gap to fix below this layer.
//!
//! The planned composition model is:
//!
//! - default discovery from the selected PAL memory catalog
//! - optional explicit overlays for bound resources, strategies, and topology
//! - conflict resolution that stays explicit rather than silently picking winners
//! - pool assessment and later pool provisioning based on the merged inventory
//!
//! What this layer should do:
//!
//! - Inventory concrete resources that already exist or are already bound.
//! - Inventory acquisition strategies that may produce more resources later.
//! - Normalize topology into a pool-facing locality model.
//! - Classify which resources are compatible enough to share a pool.
//! - Assess whether a future `MemoryPool` request is ready now, provisionable, or dead.
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
//!   PAL or whatever sits below it.
//!
//! This module is still a mockup surface, not a full implementation. The types here are
//! meant to make the intended seams explicit before `MemoryPool` arrives and calcifies the
//! wrong assumptions.

mod assessment;
mod builder;
mod inventory;
mod request;
mod support;
mod topology;

pub use assessment::{MemoryPoolAssessment, MemoryPoolAssessmentVerdict, assess_pool_request};
pub use builder::{
    MemoryProviderBuildSpec, MemoryProviderConflictPolicy, MemoryProviderDiscoveryPolicy,
};
pub use inventory::{
    MemoryCompatibilityEnvelope, MemoryPoolClass, MemoryPoolClassId, MemoryProviderInventory,
    MemoryResourceDescriptor, MemoryResourceId, MemoryResourceOrigin, MemoryStrategyCapacity,
    MemoryStrategyDescriptor, MemoryStrategyId, MemoryStrategyKind,
};
pub use request::{MemoryPoolContractRequirements, MemoryPoolRequest};
pub use support::{
    CriticalSafetyRequirements, MemoryPoolAssessmentIssues, MemoryProviderCaps,
    MemoryProviderSupport,
};
pub use topology::{
    MemoryTopology, MemoryTopologyLink, MemoryTopologyLinkId, MemoryTopologyLinkKind,
    MemoryTopologyNode, MemoryTopologyNodeId, MemoryTopologyNodeKind, MemoryTopologyPreference,
};

/// Pool-facing provider contract for inventory, topology, and resource orchestration.
pub trait MemoryProvider {
    /// Returns the provider's coarse support surface.
    fn support(&self) -> MemoryProviderSupport;

    /// Returns the provider's current topology view.
    fn topology(&self) -> MemoryTopology<'_>;

    /// Returns the provider's current resource, strategy, and pool-class inventory.
    fn inventory(&self) -> MemoryProviderInventory<'_>;

    /// Assesses a pool request against the provider's current truth.
    ///
    /// Implementations may override this for richer planning logic, but the default behavior
    /// is intentionally conservative and derived only from the exposed inventory surfaces.
    fn assess_pool(&self, request: &MemoryPoolRequest<'_>) -> MemoryPoolAssessment {
        assess_pool_request(self.inventory(), request, self.support())
    }
}
