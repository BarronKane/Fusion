use core::num::NonZeroUsize;

/// Granularity information that governs operations on a memory resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryGeometry {
    /// Smallest meaningful granule for the domain, usually the base page size.
    pub base_granule: NonZeroUsize,
    /// Minimum acquisition or materialization granule.
    pub alloc_granule: NonZeroUsize,
    /// Protection-change granule when protection control exists.
    pub protect_granule: Option<NonZeroUsize>,
    /// Commit/decommit granule when commitment control exists.
    pub commit_granule: Option<NonZeroUsize>,
    /// Lock/unlock granule when residency locking exists.
    pub lock_granule: Option<NonZeroUsize>,
    /// Larger granule such as huge-page size when the backend exposes one.
    pub large_granule: Option<NonZeroUsize>,
}
