use core::num::NonZeroUsize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryGeometry {
    pub base_granule: NonZeroUsize,
    pub alloc_granule: NonZeroUsize,
    pub protect_granule: Option<NonZeroUsize>,
    pub commit_granule: Option<NonZeroUsize>,
    pub lock_granule: Option<NonZeroUsize>,
    pub large_granule: Option<NonZeroUsize>,
}
