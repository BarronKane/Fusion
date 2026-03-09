use fusion_pal::sys::mem::Region;

use super::attrs::ResourceAttrs;
use super::domain::{MemoryDomain, ResourceBackingKind};
use super::geometry::MemoryGeometry;
use super::ops::{ResourceHazardSet, ResourcePreferenceSet};
use super::request::ResourceContract;
use super::state::ResourceState;
use super::support::ResourceSupport;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceInfo {
    pub range: Region,
    pub domain: MemoryDomain,
    pub backing: ResourceBackingKind,
    pub attrs: ResourceAttrs,
    pub geometry: MemoryGeometry,
    pub contract: ResourceContract,
    pub support: ResourceSupport,
    pub hazards: ResourceHazardSet,
}

impl ResourceInfo {
    #[must_use]
    pub const fn ops(self) -> super::ops::ResourceOpSet {
        self.support.ops
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolvedResource {
    pub info: ResourceInfo,
    pub initial_state: ResourceState,
    pub unmet_preferences: ResourcePreferenceSet,
}

impl ResolvedResource {
    #[must_use]
    pub const fn info(self) -> ResourceInfo {
        self.info
    }
}
