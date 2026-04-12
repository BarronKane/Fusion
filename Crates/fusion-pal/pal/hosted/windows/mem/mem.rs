//! Windows fusion-pal implementation of the low-level memory contract.
//!
//! This backend exposes only Windows virtual-memory semantics it can actually perform through
//! `VirtualAlloc`, `VirtualFree`, `VirtualProtect`, `VirtualQuery`, `VirtualLock`, and
//! `VirtualUnlock`. Anything that would require synthesis, address-space races, or semantics the
//! Win32 VM model does not directly provide is rejected explicitly.

use core::ffi::c_void;
use core::mem::MaybeUninit;
use core::num::NonZeroUsize;

use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED,
    ERROR_ALREADY_EXISTS,
    ERROR_BUSY,
    ERROR_INSUFFICIENT_BUFFER,
    ERROR_INVALID_ADDRESS,
    ERROR_INVALID_PARAMETER,
    ERROR_LOCK_VIOLATION,
    ERROR_NOT_ENOUGH_MEMORY,
    ERROR_NOT_SUPPORTED,
    ERROR_OUTOFMEMORY,
    GetLastError,
    WIN32_ERROR,
};
use windows::Win32::System::Memory::{
    GetLargePageMinimum,
    MEMORY_BASIC_INFORMATION,
    MEM_COMMIT,
    MEM_DECOMMIT,
    MEM_FREE,
    MEM_IMAGE,
    MEM_MAPPED,
    MEM_PRIVATE,
    MEM_RELEASE,
    MEM_RESERVE,
    MEMORY_MAPPED_VIEW_ADDRESS,
    PAGE_EXECUTE,
    PAGE_EXECUTE_READ,
    PAGE_EXECUTE_READWRITE,
    PAGE_EXECUTE_WRITECOPY,
    PAGE_GUARD,
    PAGE_NOCACHE,
    PAGE_NOACCESS,
    PAGE_PROTECTION_FLAGS,
    PAGE_READONLY,
    PAGE_READWRITE,
    PAGE_WRITECOMBINE,
    PAGE_WRITECOPY,
    UnmapViewOfFile,
    VirtualAlloc,
    VirtualFree,
    VirtualLock,
    VirtualProtect,
    VirtualQuery,
    VirtualUnlock,
};
use windows::Win32::System::SystemInformation::{
    GetSystemInfo,
    SYSTEM_INFO,
};

use crate::contract::pal::mem::{
    Address,
    Advise,
    Backing,
    CachePolicy,
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
    MemErrorKind,
    MemLockContract,
    MemMapContract,
    MemMapReplace,
    MemPlacementCaps,
    MemProtectContract,
    MemQueryContract,
    MemSupport,
    PageInfo,
    Placement,
    Protect,
    Region,
    RegionAttrs,
    RegionInfo,
    ReplacePlacement,
};

/// Windows implementation of the fusion-pal memory provider contract.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsMem;

/// Target-selected fusion-pal memory provider alias for Windows builds.
pub type PlatformMem = WindowsMem;

#[allow(clippy::useless_nonzero_new_unchecked)]
const DEFAULT_PAGE_SIZE: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(4096) };

/// Returns the process-wide Windows memory provider handle.
#[must_use]
pub const fn system_mem() -> PlatformMem {
    PlatformMem::new()
}

impl WindowsMem {
    /// Creates a new Windows fusion-pal memory provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    fn page_size_raw() -> usize {
        usize::try_from(system_info().dwPageSize).unwrap_or(DEFAULT_PAGE_SIZE.get())
    }

    fn alloc_granule_raw() -> usize {
        usize::try_from(system_info().dwAllocationGranularity).unwrap_or(Self::page_size_raw())
    }

    fn huge_page_size() -> Option<NonZeroUsize> {
        NonZeroUsize::new(unsafe { GetLargePageMinimum() })
    }

    fn validate_common<P>(req: &MapRequest<'_, P>) -> Result<(), MemError> {
        if req.len == 0 {
            return Err(MemError::invalid());
        }

        if req.align != 0 && !req.align.is_power_of_two() {
            return Err(MemError::misaligned());
        }

        if req.len.checked_add(Self::alloc_granule_raw()).is_none() {
            return Err(MemError::overflow());
        }

        if req.align > Self::alloc_granule_raw() {
            // Higher-than-granule alignment would need over-allocation and trimming. This backend
            // does not claim that semantic yet.
            return Err(MemError::unsupported());
        }

        if req.cache != CachePolicy::Default {
            return Err(MemError::unsupported());
        }

        if req.flags.contains(MapFlags::SHARED) {
            return Err(MemError::unsupported());
        }

        if req.flags.contains(MapFlags::HUGE_PAGE)
            || req.flags.contains(MapFlags::POPULATE)
            || req.flags.contains(MapFlags::WIPE_ON_FREE)
            || req.protect.contains(Protect::GUARD)
        {
            return Err(MemError::unsupported());
        }

        if req.flags.contains(MapFlags::RESERVE_ONLY) && req.flags.contains(MapFlags::COMMIT_NOW) {
            return Err(MemError::invalid());
        }

        if req.flags.contains(MapFlags::RESERVE_ONLY) && req.flags.contains(MapFlags::LOCKED) {
            return Err(MemError::invalid());
        }

        if req.flags.contains(MapFlags::RESERVE_ONLY) && req.protect != Protect::NONE {
            return Err(MemError::invalid());
        }

        let allowed_attrs = RegionAttrs::VIRTUAL_ONLY | RegionAttrs::EXECUTABLE;
        if !(req.attrs - allowed_attrs).is_empty() {
            return Err(MemError::unsupported());
        }

        if req.attrs.contains(RegionAttrs::EXECUTABLE) && !req.protect.contains(Protect::EXEC) {
            return Err(MemError::invalid());
        }

        match req.backing {
            Backing::Anonymous => {}
            Backing::File { .. }
            | Backing::Device { .. }
            | Backing::Physical { .. }
            | Backing::NativePool { .. }
            | Backing::BorrowedRegion { .. } => {
                return Err(MemError::unsupported());
            }
        }

        Ok(())
    }

    fn validate_safe_placement(placement: Placement) -> Result<(), MemError> {
        let granule = Self::alloc_granule_raw();
        match placement {
            Placement::Anywhere => Ok(()),
            Placement::FixedNoReplace(addr) => {
                if !addr.is_multiple_of(granule) {
                    return Err(MemError::misaligned());
                }
                Ok(())
            }
            Placement::Hint(_)
            | Placement::PreferredNode(_)
            | Placement::RequiredNode(_)
            | Placement::RegionId(_) => Err(MemError::unsupported()),
        }
    }

    fn validate_replace_placement(_placement: ReplacePlacement) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }

    fn mapped_extent(len: usize) -> Result<usize, MemError> {
        let page = Self::page_size_raw();
        let mask = page.checked_sub(1).ok_or(MemError::overflow())?;
        len.checked_add(mask)
            .map(|rounded| rounded & !mask)
            .ok_or(MemError::overflow())
    }

    fn is_page_aligned_region(region: Region) -> bool {
        let page = Self::page_size_raw();
        region.base.get().is_multiple_of(page) && region.len.is_multiple_of(page)
    }

    fn coerce_region(ptr: *mut c_void, len: usize) -> Result<Region, MemError> {
        Ok(Region {
            base: Address::new(ptr.cast::<u8>() as usize),
            len: Self::mapped_extent(len)?,
        })
    }

    fn to_page_protect(protect: Protect) -> Result<PAGE_PROTECTION_FLAGS, MemError> {
        if protect.contains(Protect::GUARD) {
            return Err(MemError::unsupported());
        }

        match protect {
            Protect::NONE => Ok(PAGE_NOACCESS),
            bits if bits == Protect::READ => Ok(PAGE_READONLY),
            bits if bits == (Protect::READ | Protect::WRITE) => Ok(PAGE_READWRITE),
            bits if bits == Protect::EXEC => Ok(PAGE_EXECUTE),
            bits if bits == (Protect::READ | Protect::EXEC) => Ok(PAGE_EXECUTE_READ),
            bits if bits == (Protect::READ | Protect::WRITE | Protect::EXEC) => {
                Ok(PAGE_EXECUTE_READWRITE)
            }
            _ => Err(MemError::unsupported()),
        }
    }

    fn query_page(addr: usize) -> Result<MEMORY_BASIC_INFORMATION, MemError> {
        let mut info = MaybeUninit::<MEMORY_BASIC_INFORMATION>::uninit();
        let written = unsafe {
            VirtualQuery(
                Some(addr as *const c_void),
                info.as_mut_ptr(),
                core::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };

        if written == 0 {
            return Err(map_win32_error(unsafe { GetLastError() }));
        }

        Ok(unsafe { info.assume_init() })
    }

    fn ensure_fixed_range_is_free(base: usize, len: usize) -> Result<(), MemError> {
        let end = base.checked_add(len).ok_or(MemError::overflow())?;
        let mut cursor = base;

        // This probe is inherently TOCTOU on a hosted OS: another actor may claim the range after
        // we observe it free and before a later fixed reservation attempt commits.
        while cursor < end {
            let info = Self::query_page(cursor)?;
            if info.State != MEM_FREE {
                return Err(MemError::busy());
            }

            let next = (info.BaseAddress as usize)
                .checked_add(info.RegionSize)
                .ok_or(MemError::overflow())?;
            if next <= cursor {
                return Err(MemError::invalid_addr());
            }
            cursor = next;
        }

        Ok(())
    }

    fn map_placement_addr(placement: Placement) -> Result<Option<*const c_void>, MemError> {
        match placement {
            Placement::Anywhere => Ok(None),
            Placement::FixedNoReplace(addr) => Ok(Some(addr as *const c_void)),
            Placement::Hint(_)
            | Placement::PreferredNode(_)
            | Placement::RequiredNode(_)
            | Placement::RegionId(_) => Err(MemError::unsupported()),
        }
    }

    fn region_info_from_page(info: MEMORY_BASIC_INFORMATION) -> Result<RegionInfo, MemError> {
        if info.State == MEM_FREE {
            return Err(MemError::invalid_addr());
        }

        let base = Address::new(info.BaseAddress as usize);
        let region = Region {
            base,
            len: info.RegionSize,
        };
        let protect = protect_from_page(info.Protect);
        let mut attrs = RegionAttrs::VIRTUAL_ONLY;
        if protect.contains(Protect::EXEC) {
            attrs |= RegionAttrs::EXECUTABLE;
        }

        Ok(RegionInfo {
            region,
            protect,
            attrs,
            cache: cache_policy_from_page(info.Protect),
            placement: Placement::Anywhere,
            committed: info.State == MEM_COMMIT,
        })
    }
}

impl MemBaseContract for WindowsMem {
    fn caps(&self) -> MemCaps {
        MemCaps::MAP_ANON
            | MemCaps::MAP_FIXED_NOREPLACE
            | MemCaps::PROTECT
            | MemCaps::LOCK
            | MemCaps::QUERY
            | MemCaps::RESERVE_ONLY
            | MemCaps::COMMIT_CONTROL
            | MemCaps::DECOMMIT_CONTROL
            | MemCaps::EXECUTE_MAP
    }

    fn support(&self) -> MemSupport {
        MemSupport {
            caps: self.caps(),
            map_flags: MapFlags::PRIVATE
                | MapFlags::RESERVE_ONLY
                | MapFlags::COMMIT_NOW
                | MapFlags::LOCKED,
            protect: Protect::READ | Protect::WRITE | Protect::EXEC,
            backings: MemBackingCaps::ANON_PRIVATE,
            placements: MemPlacementCaps::ANYWHERE | MemPlacementCaps::FIXED_NOREPLACE,
            advice: MemAdviceCaps::empty(),
        }
    }

    fn page_info(&self) -> PageInfo {
        let base_page = NonZeroUsize::new(Self::page_size_raw()).unwrap_or(DEFAULT_PAGE_SIZE);
        let alloc_granule =
            NonZeroUsize::new(Self::alloc_granule_raw()).unwrap_or(DEFAULT_PAGE_SIZE);
        PageInfo {
            base_page,
            alloc_granule,
            huge_page: Self::huge_page_size(),
        }
    }
}

impl MemMapContract for WindowsMem {
    unsafe fn map(&self, req: &MapRequest<'_>) -> Result<Region, MemError> {
        Self::validate_common(req)?;
        Self::validate_safe_placement(req.placement)?;

        let len = Self::mapped_extent(req.len)?;
        if let Placement::FixedNoReplace(base) = req.placement {
            Self::ensure_fixed_range_is_free(base, len)?;
        }

        let reserve_only = req.flags.contains(MapFlags::RESERVE_ONLY);
        let protect = if reserve_only {
            PAGE_NOACCESS
        } else {
            Self::to_page_protect(req.protect)?
        };

        let mut allocation_type = MEM_RESERVE;
        if !reserve_only {
            allocation_type |= MEM_COMMIT;
        }

        let ptr = unsafe {
            VirtualAlloc(
                Self::map_placement_addr(req.placement)?,
                len,
                allocation_type,
                protect,
            )
        };
        if ptr.is_null() {
            return Err(map_win32_error(unsafe { GetLastError() }));
        }

        let region = Self::coerce_region(ptr, req.len)?;

        if req.flags.contains(MapFlags::LOCKED) {
            if let Err(error) = unsafe { self.lock(region) } {
                let _ = unsafe { self.unmap(region) };
                return Err(error);
            }
        }

        Ok(region)
    }

    unsafe fn unmap(&self, region: Region) -> Result<(), MemError> {
        let info = Self::query_page(region.base.get())?;

        if info.AllocationBase != region.base.as_ptr().cast::<c_void>() {
            return Err(MemError::invalid_addr());
        }

        if info.Type == MEM_PRIVATE {
            unsafe { VirtualFree(region.base.as_ptr().cast::<c_void>(), 0, MEM_RELEASE) }
                .map_err(|_| map_win32_error(unsafe { GetLastError() }))
        } else if info.Type == MEM_MAPPED || info.Type == MEM_IMAGE {
            unsafe {
                UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                    Value: region.base.as_ptr().cast::<c_void>(),
                })
            }
            .map_err(|_| map_win32_error(unsafe { GetLastError() }))
        } else {
            Err(MemError::unsupported())
        }
    }
}

unsafe impl MemMapReplace for WindowsMem {
    unsafe fn map_replace(&self, req: &MapReplaceRequest<'_>) -> Result<Region, MemError> {
        Self::validate_common(req)?;
        Self::validate_replace_placement(req.placement)?;
        Err(MemError::unsupported())
    }
}

impl MemProtectContract for WindowsMem {
    unsafe fn protect(&self, region: Region, protect: Protect) -> Result<(), MemError> {
        if !Self::is_page_aligned_region(region) {
            return Err(MemError::misaligned());
        }

        let protect = Self::to_page_protect(protect)?;
        let mut old = PAGE_NOACCESS;
        unsafe {
            VirtualProtect(
                region.base.as_ptr().cast::<c_void>(),
                region.len,
                protect,
                &mut old,
            )
        }
        .map_err(|_| map_win32_error(unsafe { GetLastError() }))
    }
}

impl MemCommitContract for WindowsMem {
    unsafe fn commit(&self, region: Region, protect: Protect) -> Result<(), MemError> {
        if !Self::is_page_aligned_region(region) {
            return Err(MemError::misaligned());
        }

        let protect = Self::to_page_protect(protect)?;
        let ptr = unsafe {
            VirtualAlloc(
                Some(region.base.as_ptr().cast::<c_void>()),
                region.len,
                MEM_COMMIT,
                protect,
            )
        };
        if ptr.is_null() {
            Err(map_win32_error(unsafe { GetLastError() }))
        } else {
            Ok(())
        }
    }

    unsafe fn decommit(&self, region: Region) -> Result<(), MemError> {
        if !Self::is_page_aligned_region(region) {
            return Err(MemError::misaligned());
        }

        unsafe {
            VirtualFree(
                region.base.as_ptr().cast::<c_void>(),
                region.len,
                MEM_DECOMMIT,
            )
        }
        .map_err(|_| map_win32_error(unsafe { GetLastError() }))
    }
}

impl MemQueryContract for WindowsMem {
    fn query(&self, addr: Address) -> Result<RegionInfo, MemError> {
        WindowsMem::region_info_from_page(WindowsMem::query_page(addr.get())?)
    }
}

impl MemAdviseContract for WindowsMem {
    unsafe fn advise(&self, _region: Region, _advice: Advise) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

impl MemLockContract for WindowsMem {
    unsafe fn lock(&self, region: Region) -> Result<(), MemError> {
        if !WindowsMem::is_page_aligned_region(region) {
            return Err(MemError::misaligned());
        }

        unsafe { VirtualLock(region.base.as_ptr().cast::<c_void>(), region.len) }
            .map_err(|_| map_win32_error(unsafe { GetLastError() }))
    }

    unsafe fn unlock(&self, region: Region) -> Result<(), MemError> {
        if !WindowsMem::is_page_aligned_region(region) {
            return Err(MemError::misaligned());
        }

        unsafe { VirtualUnlock(region.base.as_ptr().cast::<c_void>(), region.len) }
            .map_err(|_| map_win32_error(unsafe { GetLastError() }))
    }
}

impl crate::contract::pal::mem::MemCatalogContract for WindowsMem {}

fn system_info() -> SYSTEM_INFO {
    let mut info = MaybeUninit::<SYSTEM_INFO>::uninit();
    unsafe { GetSystemInfo(info.as_mut_ptr()) };
    unsafe { info.assume_init() }
}

fn protect_from_page(protect: PAGE_PROTECTION_FLAGS) -> Protect {
    let mut out = match PAGE_PROTECTION_FLAGS(protect.0 & 0xff) {
        PAGE_NOACCESS => Protect::NONE,
        PAGE_READONLY => Protect::READ,
        PAGE_READWRITE | PAGE_WRITECOPY => Protect::READ | Protect::WRITE,
        PAGE_EXECUTE => Protect::EXEC,
        PAGE_EXECUTE_READ => Protect::READ | Protect::EXEC,
        PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY => {
            Protect::READ | Protect::WRITE | Protect::EXEC
        }
        _ => Protect::empty(),
    };

    if protect.contains(PAGE_GUARD) {
        out |= Protect::GUARD;
    }

    out
}

fn cache_policy_from_page(protect: PAGE_PROTECTION_FLAGS) -> CachePolicy {
    if protect.contains(PAGE_WRITECOMBINE) {
        CachePolicy::WriteCombined
    } else if protect.contains(PAGE_NOCACHE) {
        CachePolicy::Uncached
    } else {
        CachePolicy::Default
    }
}

const fn map_win32_error(error: WIN32_ERROR) -> MemError {
    match error {
        ERROR_NOT_ENOUGH_MEMORY | ERROR_OUTOFMEMORY | ERROR_INSUFFICIENT_BUFFER => MemError::oom(),
        ERROR_INVALID_PARAMETER => MemError::invalid(),
        ERROR_INVALID_ADDRESS => MemError::invalid_addr(),
        ERROR_ACCESS_DENIED => MemError {
            kind: MemErrorKind::PermissionDenied,
        },
        ERROR_ALREADY_EXISTS | ERROR_BUSY | ERROR_LOCK_VIOLATION => MemError::busy(),
        ERROR_NOT_SUPPORTED => MemError::unsupported(),
        _ => MemError::platform(error.0 as i32),
    }
}

#[cfg(all(test, feature = "std", target_os = "windows"))]
mod tests {
    extern crate std;

    use super::*;
    use crate::contract::pal::mem::{
        CachePolicy,
        MapFlags,
        MemPlacementCaps,
        RegionAttrs,
    };

    fn anon_request(len: usize) -> MapRequest<'static> {
        MapRequest {
            len,
            align: 0,
            protect: Protect::READ | Protect::WRITE,
            flags: MapFlags::PRIVATE,
            attrs: RegionAttrs::VIRTUAL_ONLY,
            cache: CachePolicy::Default,
            placement: Placement::Anywhere,
            backing: Backing::Anonymous,
        }
    }

    #[test]
    fn maps_and_unmaps_anonymous_region() {
        let mem = WindowsMem::new();
        let page = mem.page_info().base_page.get();
        let region = unsafe { mem.map(&anon_request(page)) }.expect("map");
        assert_eq!(region.len, page);
        unsafe { mem.unmap(region) }.expect("unmap");
    }

    #[test]
    fn reserve_commit_and_decommit_roundtrip() {
        let mem = WindowsMem::new();
        let page = mem.page_info().base_page.get();

        let reserve = MapRequest {
            len: page,
            align: 0,
            protect: Protect::NONE,
            flags: MapFlags::PRIVATE | MapFlags::RESERVE_ONLY,
            attrs: RegionAttrs::VIRTUAL_ONLY,
            cache: CachePolicy::Default,
            placement: Placement::Anywhere,
            backing: Backing::Anonymous,
        };

        let region = unsafe { mem.map(&reserve) }.expect("reserve");
        assert!(!mem.query(region.base).expect("reserved query").committed);

        unsafe { mem.commit(region, Protect::READ | Protect::WRITE) }.expect("commit");
        let committed = mem.query(region.base).expect("committed query");
        assert!(committed.committed);
        assert!(committed.protect.contains(Protect::READ));
        assert!(committed.protect.contains(Protect::WRITE));

        unsafe { mem.decommit(region) }.expect("decommit");
        assert!(!mem.query(region.base).expect("decommitted query").committed);

        unsafe { mem.unmap(region) }.expect("cleanup");
    }

    #[test]
    fn fixed_no_replace_rejects_overlap() {
        let mem = WindowsMem::new();
        if !mem
            .support()
            .placements
            .contains(MemPlacementCaps::FIXED_NOREPLACE)
        {
            return;
        }

        let page = mem.page_info().base_page.get();
        let region = unsafe { mem.map(&anon_request(page)) }.expect("seed map");

        let mut req = anon_request(page);
        req.placement = Placement::FixedNoReplace(region.base.get());
        let err = unsafe { mem.map(&req) }.expect_err("fixed-no-replace should fail");
        assert_eq!(err.kind, MemErrorKind::Busy);

        unsafe { mem.unmap(region) }.expect("cleanup");
    }

    #[test]
    fn query_reports_mapped_region() {
        let mem = WindowsMem::new();
        let page = mem.page_info().base_page.get();
        let region = unsafe { mem.map(&anon_request(page)) }.expect("map");
        let info = mem.query(region.base).expect("query");

        assert!(info.region.contains(region.base.get()));
        assert!(info.region.len >= region.len);
        assert!(info.protect.contains(Protect::READ));
        assert!(info.protect.contains(Protect::WRITE));
        assert!(info.committed);

        unsafe { mem.unmap(region) }.expect("cleanup");
    }
}
