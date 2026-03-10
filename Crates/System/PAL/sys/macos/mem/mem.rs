//! macOS PAL memory backend stub.
//!
//! This keeps the PAL memory contract present on macOS-targeted builds until
//! the real backend is implemented. Operations fail explicitly with
//! `Unsupported`.

use core::num::NonZeroUsize;

use crate::pal::mem::{
    Advise, MapFlags, MapReplaceRequest, MapRequest, MemAdviceCaps, MemAdvise, MemBackingCaps,
    MemBase, MemCaps, MemCommit, MemError, MemLock, MemMap, MemMapReplace, MemPlacementCaps,
    MemProtect, MemQuery, MemSupport, PageInfo, Protect, Region, RegionInfo,
};

/// Placeholder macOS implementation of the PAL memory provider contract.
#[derive(Debug, Clone, Copy, Default)]
pub struct MacOsMem;

/// Target-selected PAL memory provider alias for macOS builds.
pub type PlatformMem = MacOsMem;

/// Returns the process-wide macOS memory provider handle.
#[must_use]
pub const fn system_mem() -> PlatformMem {
    PlatformMem::new()
}

impl MacOsMem {
    /// Creates a new macOS PAL memory provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl MemBase for MacOsMem {
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
        let base = NonZeroUsize::new(4096).expect("stub page size must be non-zero");
        PageInfo {
            base_page: base,
            alloc_granule: base,
            huge_page: None,
        }
    }
}

impl MemMap for MacOsMem {
    unsafe fn map(&self, _req: &MapRequest<'_>) -> Result<Region, MemError> {
        Err(MemError::unsupported())
    }

    unsafe fn unmap(&self, _region: Region) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

unsafe impl MemMapReplace for MacOsMem {
    unsafe fn map_replace(&self, _req: &MapReplaceRequest<'_>) -> Result<Region, MemError> {
        Err(MemError::unsupported())
    }
}

impl MemProtect for MacOsMem {
    unsafe fn protect(&self, _region: Region, _protect: Protect) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

impl MemCommit for MacOsMem {}

impl MemQuery for MacOsMem {
    fn query(&self, _addr: core::ptr::NonNull<u8>) -> Result<RegionInfo, MemError> {
        Err(MemError::unsupported())
    }
}

impl MemAdvise for MacOsMem {
    unsafe fn advise(&self, _region: Region, _advice: Advise) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

impl MemLock for MacOsMem {
    unsafe fn lock(&self, _region: Region) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }

    unsafe fn unlock(&self, _region: Region) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}
