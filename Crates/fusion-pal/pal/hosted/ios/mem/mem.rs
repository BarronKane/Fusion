//! iOS fusion-pal memory backend stub.
//!
//! This preserves the fusion-pal memory contract on iOS-targeted builds until a real
//! implementation exists. Operations fail explicitly with `Unsupported`.

use core::num::NonZeroUsize;

use crate::contract::pal::mem::{
    Address,
    Advise,
    MapFlags,
    MapReplaceRequest,
    MapRequest,
    MemAdviceCaps,
    MemAdviseContract,
    MemBackingCaps,
    MemBaseContract,
    MemCaps,
    MemCommitContract,
    MemError,
    MemLockContract,
    MemMapContract,
    MemMapReplace,
    MemPlacementCaps,
    MemProtectContract,
    MemQueryContract,
    MemSupport,
    PageInfo,
    Protect,
    Region,
    RegionInfo,
};

/// Placeholder iOS implementation of the fusion-pal memory provider contract.
#[derive(Debug, Clone, Copy, Default)]
pub struct IosMem;

/// Target-selected fusion-pal memory provider alias for iOS builds.
pub type PlatformMem = IosMem;

const STUB_PAGE_SIZE: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(4096) };

/// Returns the process-wide iOS memory provider handle.
#[must_use]
pub const fn system_mem() -> PlatformMem {
    PlatformMem::new()
}

impl IosMem {
    /// Creates a new iOS fusion-pal memory provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl MemBaseContract for IosMem {
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
            base_page: STUB_PAGE_SIZE,
            alloc_granule: STUB_PAGE_SIZE,
            huge_page: None,
        }
    }
}

impl MemMapContract for IosMem {
    unsafe fn map(&self, _req: &MapRequest<'_>) -> Result<Region, MemError> {
        Err(MemError::unsupported())
    }

    unsafe fn unmap(&self, _region: Region) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

unsafe impl MemMapReplace for IosMem {
    unsafe fn map_replace(&self, _req: &MapReplaceRequest<'_>) -> Result<Region, MemError> {
        Err(MemError::unsupported())
    }
}

impl MemProtectContract for IosMem {
    unsafe fn protect(&self, _region: Region, _protect: Protect) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

impl MemCommitContract for IosMem {}

impl MemQueryContract for IosMem {
    fn query(&self, _addr: Address) -> Result<RegionInfo, MemError> {
        Err(MemError::unsupported())
    }
}

impl MemAdviseContract for IosMem {
    unsafe fn advise(&self, _region: Region, _advice: Advise) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

impl MemLockContract for IosMem {
    unsafe fn lock(&self, _region: Region) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }

    unsafe fn unlock(&self, _region: Region) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

impl crate::contract::pal::mem::MemCatalogContract for IosMem {}
