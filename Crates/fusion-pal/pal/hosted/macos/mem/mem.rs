//! macOS fusion-pal implementation of the low-level memory contract.
//!
//! This backend exposes only Darwin semantics it can actually perform through
//! `mmap`, `munmap`, `mprotect`, `madvise`, `mlock`, and `munlock`.
//! Unsupported semantics are rejected explicitly.

use core::ffi::c_void;
use core::num::NonZeroUsize;

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
use crate::pal::hosted::macos::capability::runtime_capabilities;

/// macOS implementation of the fusion-pal memory provider contract.
#[derive(Debug, Clone, Copy, Default)]
pub struct MacOsMem;

/// Target-selected fusion-pal memory provider alias for macOS builds.
pub type PlatformMem = MacOsMem;

#[allow(clippy::useless_nonzero_new_unchecked)]
const DEFAULT_PAGE_SIZE: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(4096) };

/// Returns the process-wide macOS memory provider handle.
#[must_use]
pub const fn system_mem() -> PlatformMem {
    PlatformMem::new()
}

impl MacOsMem {
    /// Creates a new macOS fusion-pal memory provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    fn page_size_raw() -> usize {
        let raw = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        usize::try_from(raw).unwrap_or(DEFAULT_PAGE_SIZE.get())
    }

    const fn map_errno(errno: i32) -> MemError {
        match errno {
            libc::ENOMEM => MemError::oom(),
            libc::EINVAL => MemError::invalid(),
            libc::EEXIST => MemError::busy(),
            libc::EACCES | libc::EPERM => MemError {
                kind: MemErrorKind::PermissionDenied,
            },
            _ if errno == libc::ENOTSUP || errno == libc::EOPNOTSUPP => MemError::unsupported(),
            _ => MemError::platform(errno),
        }
    }

    fn last_errno() -> i32 {
        unsafe { *libc::__error() }
    }

    fn validate_common<P>(req: &MapRequest<'_, P>) -> Result<(), MemError> {
        if req.len == 0 {
            return Err(MemError::invalid());
        }

        let page = Self::page_size_raw();

        if req.align != 0 && !req.align.is_power_of_two() {
            return Err(MemError::misaligned());
        }

        if req.len.checked_add(page).is_none() {
            return Err(MemError::overflow());
        }

        if req.align > page {
            // Greater-than-page alignment needs overmapping and trimming; this backend does not
            // claim that semantic yet.
            return Err(MemError::unsupported());
        }

        if req.cache != CachePolicy::Default {
            return Err(MemError::unsupported());
        }

        if req.flags.contains(MapFlags::LOCKED)
            || req.flags.contains(MapFlags::POPULATE)
            || req.flags.contains(MapFlags::HUGE_PAGE)
            || req.flags.contains(MapFlags::RESERVE_ONLY)
            || req.flags.contains(MapFlags::COMMIT_NOW)
            || req.flags.contains(MapFlags::WIPE_ON_FREE)
            || req.protect.contains(Protect::GUARD)
        {
            return Err(MemError::unsupported());
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
            Backing::File { offset, .. } => {
                let offset = usize::try_from(offset).map_err(|_| MemError::overflow())?;
                if !offset.is_multiple_of(page) {
                    return Err(MemError::misaligned());
                }
            }
            Backing::Device { .. }
            | Backing::Physical { .. }
            | Backing::NativePool { .. }
            | Backing::BorrowedRegion { .. } => {
                return Err(MemError::unsupported());
            }
        }

        Ok(())
    }

    fn validate_safe_placement(placement: Placement) -> Result<(), MemError> {
        let page = Self::page_size_raw();

        match placement {
            Placement::Anywhere => Ok(()),
            Placement::Hint(addr) => {
                if addr.is_multiple_of(page) {
                    Ok(())
                } else {
                    Err(MemError::misaligned())
                }
            }
            Placement::FixedNoReplace(_) => Err(MemError::unsupported()),
            Placement::PreferredNode(_) | Placement::RequiredNode(_) | Placement::RegionId(_) => {
                Err(MemError::unsupported())
            }
        }
    }

    fn validate_replace_placement(placement: ReplacePlacement) -> Result<(), MemError> {
        let page = Self::page_size_raw();

        match placement {
            ReplacePlacement::FixedReplace(addr) => {
                if addr.is_multiple_of(page) {
                    Ok(())
                } else {
                    Err(MemError::misaligned())
                }
            }
        }
    }

    fn to_mmap_prot(prot: Protect) -> Result<i32, MemError> {
        if prot.contains(Protect::GUARD) {
            return Err(MemError::unsupported());
        }

        let mut out = 0;
        if prot.contains(Protect::READ) {
            out |= libc::PROT_READ;
        }
        if prot.contains(Protect::WRITE) {
            out |= libc::PROT_WRITE;
        }
        if prot.contains(Protect::EXEC) {
            out |= libc::PROT_EXEC;
        }
        if out == 0 {
            out = libc::PROT_NONE;
        }
        Ok(out)
    }

    fn to_mmap_flags<P>(req: &MapRequest<'_, P>) -> Result<i32, MemError> {
        if req.flags.contains(MapFlags::SHARED) && req.flags.contains(MapFlags::PRIVATE) {
            return Err(MemError::invalid());
        }

        let mut flags = if req.flags.contains(MapFlags::SHARED) {
            libc::MAP_SHARED
        } else {
            libc::MAP_PRIVATE
        };

        if matches!(req.backing, Backing::Anonymous) {
            flags |= libc::MAP_ANON;
        }

        Ok(flags)
    }

    const fn addr_hint(placement: Placement) -> Result<*mut c_void, MemError> {
        match placement {
            Placement::Anywhere => Ok(core::ptr::null_mut()),
            Placement::Hint(addr) => Ok(addr as *mut c_void),
            Placement::FixedNoReplace(_)
            | Placement::PreferredNode(_)
            | Placement::RequiredNode(_)
            | Placement::RegionId(_) => Err(MemError::unsupported()),
        }
    }

    const fn replace_addr(placement: ReplacePlacement) -> *mut c_void {
        match placement {
            ReplacePlacement::FixedReplace(addr) => addr as *mut c_void,
        }
    }

    fn mapped_extent(len: usize) -> Result<usize, MemError> {
        let page = Self::page_size_raw();
        let mask = page.checked_sub(1).ok_or(MemError::overflow())?;
        len.checked_add(mask)
            .map(|rounded| rounded & !mask)
            .ok_or(MemError::overflow())
    }

    fn coerce_region(ptr: *mut c_void, len: usize) -> Result<Region, MemError> {
        let base = Address::new(ptr.cast::<u8>() as usize);
        let len = Self::mapped_extent(len)?;
        Ok(Region { base, len })
    }
}

impl MemBaseContract for MacOsMem {
    fn caps(&self) -> MemCaps {
        MemCaps::MAP_ANON
            | MemCaps::MAP_FILE
            | MemCaps::MAP_FIXED_REPLACE
            | MemCaps::MAP_HINT
            | MemCaps::PROTECT
            | MemCaps::ADVISE
            | MemCaps::LOCK
            | MemCaps::EXECUTE_MAP
    }

    fn support(&self) -> MemSupport {
        let runtime = runtime_capabilities();
        MemSupport {
            caps: self.caps(),
            map_flags: MapFlags::PRIVATE | MapFlags::SHARED,
            protect: Protect::READ | Protect::WRITE | Protect::EXEC,
            backings: MemBackingCaps::ANON_PRIVATE
                | MemBackingCaps::ANON_SHARED
                | MemBackingCaps::FILE_PRIVATE
                | MemBackingCaps::FILE_SHARED,
            placements: MemPlacementCaps::ANYWHERE | MemPlacementCaps::HINT,
            advice: {
                let mut advice = MemAdviceCaps::NORMAL
                    | MemAdviceCaps::SEQUENTIAL
                    | MemAdviceCaps::RANDOM
                    | MemAdviceCaps::WILL_NEED
                    | MemAdviceCaps::DONT_NEED;
                if runtime.madv_free && !cfg!(feature = "critical-safe") {
                    advice |= MemAdviceCaps::FREE;
                }
                advice
            },
        }
    }

    fn page_info(&self) -> PageInfo {
        let base = NonZeroUsize::new(Self::page_size_raw()).unwrap_or(DEFAULT_PAGE_SIZE);
        PageInfo {
            base_page: base,
            alloc_granule: base,
            huge_page: None,
        }
    }
}

impl MemMapContract for MacOsMem {
    unsafe fn map(&self, req: &MapRequest<'_>) -> Result<Region, MemError> {
        Self::validate_common(req)?;
        Self::validate_safe_placement(req.placement)?;

        let prot = Self::to_mmap_prot(req.protect)?;
        let flags = Self::to_mmap_flags(req)?;

        let addr = Self::addr_hint(req.placement)?;
        let (fd, offset) = match req.backing {
            Backing::Anonymous => (-1, 0),
            Backing::File { fd, offset } => {
                let raw_fd = fd.as_raw_fd().map_err(|_| MemError::invalid())?;
                let offset = libc::off_t::try_from(offset).map_err(|_| MemError::overflow())?;
                (raw_fd, offset)
            }
            _ => return Err(MemError::unsupported()),
        };

        let ptr = unsafe { libc::mmap(addr, req.len, prot, flags, fd, offset) };
        if ptr == libc::MAP_FAILED {
            return Err(Self::map_errno(Self::last_errno()));
        }

        Self::coerce_region(ptr, req.len)
    }

    unsafe fn unmap(&self, region: Region) -> Result<(), MemError> {
        if unsafe { libc::munmap(region.base.as_ptr().cast::<c_void>(), region.len) } == 0 {
            Ok(())
        } else {
            Err(Self::map_errno(Self::last_errno()))
        }
    }
}

unsafe impl MemMapReplace for MacOsMem {
    unsafe fn map_replace(&self, req: &MapReplaceRequest<'_>) -> Result<Region, MemError> {
        Self::validate_common(req)?;
        Self::validate_replace_placement(req.placement)?;

        let prot = Self::to_mmap_prot(req.protect)?;
        let mut flags = Self::to_mmap_flags(req)?;
        flags |= libc::MAP_FIXED;

        let addr = Self::replace_addr(req.placement);
        let (fd, offset) = match req.backing {
            Backing::Anonymous => (-1, 0),
            Backing::File { fd, offset } => {
                let raw_fd = fd.as_raw_fd().map_err(|_| MemError::invalid())?;
                let offset = libc::off_t::try_from(offset).map_err(|_| MemError::overflow())?;
                (raw_fd, offset)
            }
            _ => return Err(MemError::unsupported()),
        };

        let ptr = unsafe { libc::mmap(addr, req.len, prot, flags, fd, offset) };
        if ptr == libc::MAP_FAILED {
            return Err(Self::map_errno(Self::last_errno()));
        }

        Self::coerce_region(ptr, req.len)
    }
}

impl MemProtectContract for MacOsMem {
    unsafe fn protect(&self, region: Region, protect: Protect) -> Result<(), MemError> {
        let prot = Self::to_mmap_prot(protect)?;

        if unsafe { libc::mprotect(region.base.as_ptr().cast::<c_void>(), region.len, prot) } == 0 {
            Ok(())
        } else {
            Err(Self::map_errno(Self::last_errno()))
        }
    }
}

impl MemCommitContract for MacOsMem {}

impl MemQueryContract for MacOsMem {
    fn query(&self, _addr: Address) -> Result<RegionInfo, MemError> {
        Err(MemError::unsupported())
    }
}

impl MemAdviseContract for MacOsMem {
    unsafe fn advise(&self, region: Region, advice: Advise) -> Result<(), MemError> {
        let runtime = runtime_capabilities();
        let adv = match advice {
            Advise::Normal => libc::MADV_NORMAL,
            Advise::Sequential => libc::MADV_SEQUENTIAL,
            Advise::Random => libc::MADV_RANDOM,
            Advise::WillNeed => libc::MADV_WILLNEED,
            Advise::DontNeed => libc::MADV_DONTNEED,
            Advise::Free if runtime.madv_free && !cfg!(feature = "critical-safe") => {
                libc::MADV_FREE
            }
            Advise::Free | Advise::NoHugePage | Advise::HugePage => {
                return Err(MemError::unsupported());
            }
        };

        if unsafe { libc::madvise(region.base.as_ptr().cast::<c_void>(), region.len, adv) } == 0 {
            Ok(())
        } else {
            Err(Self::map_errno(Self::last_errno()))
        }
    }
}

impl MemLockContract for MacOsMem {
    unsafe fn lock(&self, region: Region) -> Result<(), MemError> {
        if unsafe { libc::mlock(region.base.as_ptr().cast::<c_void>(), region.len) } == 0 {
            Ok(())
        } else {
            Err(Self::map_errno(Self::last_errno()))
        }
    }

    unsafe fn unlock(&self, region: Region) -> Result<(), MemError> {
        if unsafe { libc::munlock(region.base.as_ptr().cast::<c_void>(), region.len) } == 0 {
            Ok(())
        } else {
            Err(Self::map_errno(Self::last_errno()))
        }
    }
}

impl crate::contract::pal::mem::MemCatalogContract for MacOsMem {}
