use fusion_pal::sys::mem::Region;

use super::attrs::ResourceAttrs;
use super::domain::{
    MemoryDomain,
    ResourceBackingKind,
};
use super::geometry::MemoryGeometry;
use super::layout::AllocatorLayoutPolicy;
use super::ops::{
    ResourceHazardSet,
    ResourcePreferenceSet,
};
use super::request::ResourceContract;
use super::state::ResourceState;
use super::support::ResourceSupport;

/// Immutable descriptive information for a live memory resource instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceInfo {
    /// Contiguous governed range represented by the resource.
    pub(crate) range: Region,
    /// Memory domain classification for the range.
    pub domain: MemoryDomain,
    /// Concrete backing shape for the range.
    pub backing: ResourceBackingKind,
    /// Intrinsic attributes of the range.
    pub attrs: ResourceAttrs,
    /// Operation granularity information.
    pub geometry: MemoryGeometry,
    /// Allocator-facing metadata and extent layout policy.
    pub layout: AllocatorLayoutPolicy,
    /// Immutable lifetime contract the resource must continue to satisfy.
    pub contract: ResourceContract,
    /// Runtime support surface of this instance.
    pub support: ResourceSupport,
    /// Inherent hazards that apply to the range.
    pub hazards: ResourceHazardSet,
}

impl ResourceInfo {
    /// Creates immutable descriptive information for a resource instance.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        range: Region,
        domain: MemoryDomain,
        backing: ResourceBackingKind,
        attrs: ResourceAttrs,
        geometry: MemoryGeometry,
        layout: AllocatorLayoutPolicy,
        contract: ResourceContract,
        support: ResourceSupport,
        hazards: ResourceHazardSet,
    ) -> Self {
        Self {
            range,
            domain,
            backing,
            attrs,
            geometry,
            layout,
            contract,
            support,
            hazards,
        }
    }

    /// Returns the operation set advertised by this resource instance.
    #[must_use]
    pub const fn ops(self) -> super::ops::ResourceOpSet {
        self.support.ops
    }

    /// Returns the contiguous governed range represented by this resource.
    #[must_use]
    pub const fn range(self) -> Region {
        self.range
    }
}

/// Creation-time resolution record for a resource instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolvedResource {
    /// Immutable descriptive information for the created resource.
    pub info: ResourceInfo,
    /// Initial runtime state after creation and post-map preference application.
    pub initial_state: ResourceState,
    /// Soft preferences that could not be honored during creation.
    pub unmet_preferences: ResourcePreferenceSet,
}

impl ResolvedResource {
    /// Returns the immutable descriptive information for the resolved resource.
    #[must_use]
    pub const fn info(self) -> ResourceInfo {
        self.info
    }
}
