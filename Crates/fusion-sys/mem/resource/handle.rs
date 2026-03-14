use super::{
    BoundMemoryResource, MemoryResource, QueryableResource, RangeView, ResolvedResource,
    ResourceError, ResourceInfo, ResourceRange, ResourceState, VirtualMemoryResource,
};
use core::ptr::NonNull;
use fusion_pal::sys::mem::RegionInfo;

/// Owned sum type for concrete `fusion-sys::mem::resource` instances.
///
/// `MemoryPool` and future orchestration layers need to own real resources without falling
/// back to trait-object allocation or duplicating per-type storage. This enum keeps that
/// ownership explicit while preserving the common [`MemoryResource`] contract.
#[derive(Debug)]
pub enum MemoryResourceHandle {
    /// Hosted virtual-memory resource acquired through the active fusion-pal backend.
    Virtual(VirtualMemoryResource),
    /// Externally governed bound range.
    Bound(BoundMemoryResource),
}

impl MemoryResourceHandle {
    /// Returns creation-time resolution metadata for the owned resource.
    #[must_use]
    pub const fn resolved(&self) -> ResolvedResource {
        match self {
            Self::Virtual(resource) => resource.resolved(),
            Self::Bound(resource) => resource.resolved(),
        }
    }

    /// Returns a borrowed view of the governed range.
    #[must_use]
    pub fn view(&self) -> RangeView<'_> {
        self.range()
    }

    /// Returns a checked borrowed subrange of the resource.
    ///
    /// # Errors
    /// Returns an error when the requested range falls outside the governed range.
    pub fn subview(&self, range: ResourceRange) -> Result<RangeView<'_>, ResourceError> {
        self.subrange(range)
    }
}

impl From<VirtualMemoryResource> for MemoryResourceHandle {
    fn from(value: VirtualMemoryResource) -> Self {
        Self::Virtual(value)
    }
}

impl From<BoundMemoryResource> for MemoryResourceHandle {
    fn from(value: BoundMemoryResource) -> Self {
        Self::Bound(value)
    }
}

impl MemoryResource for MemoryResourceHandle {
    fn info(&self) -> &ResourceInfo {
        match self {
            Self::Virtual(resource) => resource.info(),
            Self::Bound(resource) => resource.info(),
        }
    }

    fn state(&self) -> ResourceState {
        match self {
            Self::Virtual(resource) => resource.state(),
            Self::Bound(resource) => resource.state(),
        }
    }
}

impl QueryableResource for MemoryResourceHandle {
    fn query(&self, addr: NonNull<u8>) -> Result<RegionInfo, ResourceError> {
        match self {
            Self::Virtual(resource) => resource.query(addr),
            Self::Bound(resource) => resource.query(addr),
        }
    }
}
