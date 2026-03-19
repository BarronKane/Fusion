//! Cortex-M bare-metal memory backend.
//!
//! Memory on Cortex-M is flat SRAM at known addresses. No MMU, no demand paging.
//! Bound/static memory resources are the primary path. Generic virtual mapping remains
//! unsupported until the SoC memory catalogs are wired.

use core::num::NonZeroUsize;

use crate::pal::mem::{
    Advise, MapFlags, MapReplaceRequest, MapRequest, MemAdviceCaps, MemAdvise, MemBackingCaps,
    MemBase, MemCaps, MemCommit, MemError, MemLock, MemMap, MemMapReplace, MemPlacementCaps,
    MemProtect, MemQuery, MemSupport, PageInfo, Protect, Region, RegionInfo,
};

/// Cortex-M bare-metal memory provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct CortexMMem;

/// Target-selected memory provider alias for Cortex-M builds.
pub type PlatformMem = CortexMMem;

/// Cortex-M has no page table — use a nominal 4-byte alignment granule.
const STUB_GRANULE: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(4) };

/// Returns the process-wide Cortex-M memory provider handle.
#[must_use]
pub const fn system_mem() -> PlatformMem {
    PlatformMem::new()
}

impl CortexMMem {
    /// Creates a new Cortex-M memory provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl MemBase for CortexMMem {
    fn caps(&self) -> MemCaps {
        MemCaps::empty()
    }

    fn support(&self) -> MemSupport {
        MemSupport {
            caps: MemCaps::empty(),
            map_flags: MapFlags::empty(),
            protect: Protect::empty(),
            backings: MemBackingCaps::empty(),
            placements: MemPlacementCaps::empty(),
            advice: MemAdviceCaps::empty(),
        }
    }

    fn page_info(&self) -> PageInfo {
        PageInfo {
            base_page: STUB_GRANULE,
            alloc_granule: STUB_GRANULE,
            huge_page: None,
        }
    }
}

impl MemMap for CortexMMem {
    unsafe fn map(&self, _req: &MapRequest<'_>) -> Result<Region, MemError> {
        Err(MemError::unsupported())
    }

    unsafe fn unmap(&self, _region: Region) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

unsafe impl MemMapReplace for CortexMMem {
    unsafe fn map_replace(&self, _req: &MapReplaceRequest<'_>) -> Result<Region, MemError> {
        Err(MemError::unsupported())
    }
}

impl MemProtect for CortexMMem {
    unsafe fn protect(&self, _region: Region, _protect: Protect) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

impl MemCommit for CortexMMem {}

impl MemQuery for CortexMMem {
    fn query(&self, _addr: core::ptr::NonNull<u8>) -> Result<RegionInfo, MemError> {
        Err(MemError::unsupported())
    }
}

impl MemAdvise for CortexMMem {
    unsafe fn advise(&self, _region: Region, _advice: Advise) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

impl MemLock for CortexMMem {
    unsafe fn lock(&self, _region: Region) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }

    unsafe fn unlock(&self, _region: Region) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

impl crate::pal::mem::MemCatalog for CortexMMem {}
