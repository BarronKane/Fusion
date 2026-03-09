use core::ffi::c_void;
use core::num::NonZeroUsize;
use core::ptr::NonNull;

use rustix::fd::BorrowedFd;
use rustix::io::Errno;
use rustix::mm::{self, Advice as MmAdvice, MapFlags as MmMapFlags, MprotectFlags, ProtFlags};
use rustix::param;

use crate::pal::mem::{
    Advise, Backing, CachePolicy, MapFlags, MapReplaceRequest, MapRequest, MemAdvise, MemBase,
    MemCaps, MemError, MemErrorKind, MemLock, MemMap, MemMapReplace, MemProtect, MemQuery,
    PageInfo, Placement, Protect, Region, RegionAttrs, RegionInfo, ReplacePlacement,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxMem;

pub type PlatformMem = LinuxMem;

#[must_use]
pub const fn system_mem() -> PlatformMem {
    PlatformMem::new()
}

impl LinuxMem {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    fn map_errno(errno: Errno) -> MemError {
        match errno {
            Errno::NOMEM => MemError::oom(),
            Errno::INVAL => MemError::invalid(),
            Errno::EXIST => MemError::busy(),
            Errno::ACCESS | Errno::PERM => MemError {
                kind: MemErrorKind::PermissionDenied,
            },
            _ => MemError::platform(errno.raw_os_error()),
        }
    }

    fn page_size_raw() -> usize {
        param::page_size()
    }

    fn to_mmap_prot(prot: Protect) -> Result<ProtFlags, MemError> {
        if prot.contains(Protect::GUARD) {
            return Err(MemError::unsupported());
        }

        let mut out = ProtFlags::empty();
        if prot.contains(Protect::READ) {
            out |= ProtFlags::READ;
        }
        if prot.contains(Protect::WRITE) {
            out |= ProtFlags::WRITE;
        }
        if prot.contains(Protect::EXEC) {
            out |= ProtFlags::EXEC;
        }
        Ok(out)
    }

    fn to_mprotect_flags(prot: Protect) -> Result<MprotectFlags, MemError> {
        if prot.contains(Protect::GUARD) {
            return Err(MemError::unsupported());
        }

        let mut out = MprotectFlags::empty();
        if prot.contains(Protect::READ) {
            out |= MprotectFlags::READ;
        }
        if prot.contains(Protect::WRITE) {
            out |= MprotectFlags::WRITE;
        }
        if prot.contains(Protect::EXEC) {
            out |= MprotectFlags::EXEC;
        }
        Ok(out)
    }

    fn to_common_mmap_flags<P>(req: &MapRequest<'_, P>) -> Result<MmMapFlags, MemError> {
        let mut flags = MmMapFlags::empty();

        if req.flags.contains(MapFlags::SHARED) && req.flags.contains(MapFlags::PRIVATE) {
            return Err(MemError::invalid());
        }

        if req.flags.contains(MapFlags::SHARED) {
            flags |= MmMapFlags::SHARED;
        } else {
            flags |= MmMapFlags::PRIVATE;
        }

        match req.backing {
            Backing::Anonymous => {}
            Backing::File { .. } => {}
            Backing::Device { .. }
            | Backing::Physical { .. }
            | Backing::NativePool { .. }
            | Backing::BorrowedRegion { .. } => {
                return Err(MemError::unsupported());
            }
        }

        if req.flags.contains(MapFlags::LOCKED) {
            flags |= MmMapFlags::LOCKED;
        }

        if req.flags.contains(MapFlags::POPULATE) {
            flags |= MmMapFlags::POPULATE;
        }

        Ok(flags)
    }

    fn validate_common<P>(&self, req: &MapRequest<'_, P>) -> Result<(), MemError> {
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
            // Higher alignment requires overmapping and trimming. This backend
            // doesn't claim that capability yet, so fail rather than lie.
            return Err(MemError::unsupported());
        }

        if req.cache != CachePolicy::Default {
            return Err(MemError::unsupported());
        }

        if req.protect.contains(Protect::GUARD)
            || req.flags.contains(MapFlags::HUGE_PAGE)
            || req.flags.contains(MapFlags::RESERVE_ONLY)
            || req.flags.contains(MapFlags::COMMIT_NOW)
            || req.flags.contains(MapFlags::WIPE_ON_FREE)
        {
            return Err(MemError::unsupported());
        }

        if req.attrs.contains(RegionAttrs::DMA_VISIBLE)
            || req.attrs.contains(RegionAttrs::PHYS_CONTIGUOUS)
            || req.attrs.contains(RegionAttrs::DEVICE_LOCAL)
        {
            return Err(MemError::unsupported());
        }

        if let Backing::File { offset, .. } = req.backing {
            if offset as usize % page != 0 {
                return Err(MemError::misaligned());
            }
        }

        Ok(())
    }

    fn validate_safe_placement(&self, placement: Placement) -> Result<(), MemError> {
        let page = Self::page_size_raw();

        match placement {
            Placement::Anywhere => Ok(()),
            Placement::Hint(addr) | Placement::FixedNoReplace(addr) => {
                if addr % page == 0 {
                    Ok(())
                } else {
                    Err(MemError::misaligned())
                }
            }
            Placement::PreferredNode(_) | Placement::RequiredNode(_) | Placement::RegionId(_) => {
                Err(MemError::unsupported())
            }
        }
    }

    fn validate_replace_placement(&self, placement: ReplacePlacement) -> Result<(), MemError> {
        let page = Self::page_size_raw();

        match placement {
            ReplacePlacement::FixedReplace(addr) => {
                if addr % page == 0 {
                    Ok(())
                } else {
                    Err(MemError::misaligned())
                }
            }
        }
    }

    fn addr_hint(placement: Placement) -> Result<*mut c_void, MemError> {
        match placement {
            Placement::Anywhere => Ok(core::ptr::null_mut()),
            Placement::Hint(addr) | Placement::FixedNoReplace(addr) => Ok(addr as *mut c_void),
            Placement::PreferredNode(_) | Placement::RequiredNode(_) | Placement::RegionId(_) => {
                Err(MemError::unsupported())
            }
        }
    }

    fn replace_addr(placement: ReplacePlacement) -> *mut c_void {
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
        let base = NonNull::new(ptr.cast::<u8>()).ok_or(MemError::invalid_addr())?;
        let len = Self::mapped_extent(len)?;
        Ok(Region { base, len })
    }

    fn enforce_no_replace(region: Region, requested: Placement) -> Result<Region, MemError> {
        match requested {
            Placement::FixedNoReplace(addr) if region.base.as_ptr() as usize != addr => {
                let _ = unsafe { mm::munmap(region.base.as_ptr().cast::<c_void>(), region.len) };
                Err(MemError::busy())
            }
            _ => Ok(region),
        }
    }
}

impl MemBase for LinuxMem {
    fn caps(&self) -> MemCaps {
        MemCaps::MAP_ANON
            | MemCaps::MAP_FILE
            | MemCaps::MAP_FIXED_NOREPLACE
            | MemCaps::MAP_FIXED_REPLACE
            | MemCaps::MAP_HINT
            | MemCaps::PROTECT
            | MemCaps::ADVISE
            | MemCaps::LOCK
            | MemCaps::EXECUTE_MAP
    }

    fn page_info(&self) -> PageInfo {
        let base = NonZeroUsize::new(Self::page_size_raw())
            .unwrap_or_else(|| NonZeroUsize::new(4096).unwrap());
        PageInfo {
            base_page: base,
            alloc_granule: base,
            huge_page: None,
        }
    }
}

impl MemMap for LinuxMem {
    unsafe fn map(&self, req: &MapRequest<'_>) -> Result<Region, MemError> {
        self.validate_common(req)?;
        self.validate_safe_placement(req.placement)?;

        let prot = Self::to_mmap_prot(req.protect)?;
        let mut flags = Self::to_common_mmap_flags(req)?;

        if let Placement::FixedNoReplace(_) = req.placement {
            flags |= MmMapFlags::FIXED_NOREPLACE;
        }

        let addr_hint = Self::addr_hint(req.placement)?;
        let ptr = match req.backing {
            Backing::Anonymous => unsafe { mm::mmap_anonymous(addr_hint, req.len, prot, flags) },
            Backing::File { fd, offset } => {
                let fd = unsafe { BorrowedFd::borrow_raw(fd) };
                unsafe { mm::mmap(addr_hint, req.len, prot, flags, fd, offset) }
            }
            _ => return Err(MemError::unsupported()),
        }
        .map_err(Self::map_errno)?;

        let region = Self::coerce_region(ptr, req.len)?;
        Self::enforce_no_replace(region, req.placement)
    }

    unsafe fn unmap(&self, region: Region) -> Result<(), MemError> {
        unsafe { mm::munmap(region.base.as_ptr().cast::<c_void>(), region.len) }
            .map_err(Self::map_errno)
    }
}

unsafe impl MemMapReplace for LinuxMem {
    unsafe fn map_replace(&self, req: &MapReplaceRequest<'_>) -> Result<Region, MemError> {
        self.validate_common(req)?;
        self.validate_replace_placement(req.placement)?;

        let prot = Self::to_mmap_prot(req.protect)?;
        let mut flags = Self::to_common_mmap_flags(req)?;
        flags |= MmMapFlags::FIXED;

        let addr = Self::replace_addr(req.placement);
        let ptr = match req.backing {
            Backing::Anonymous => unsafe { mm::mmap_anonymous(addr, req.len, prot, flags) },
            Backing::File { fd, offset } => {
                let fd = unsafe { BorrowedFd::borrow_raw(fd) };
                unsafe { mm::mmap(addr, req.len, prot, flags, fd, offset) }
            }
            _ => return Err(MemError::unsupported()),
        }
        .map_err(Self::map_errno)?;

        Self::coerce_region(ptr, req.len)
    }
}

impl MemProtect for LinuxMem {
    unsafe fn protect(&self, region: Region, protect: Protect) -> Result<(), MemError> {
        let flags = Self::to_mprotect_flags(protect)?;
        unsafe { mm::mprotect(region.base.as_ptr().cast::<c_void>(), region.len, flags) }
            .map_err(Self::map_errno)
    }
}

impl MemQuery for LinuxMem {
    fn query(&self, _addr: NonNull<u8>) -> Result<RegionInfo, MemError> {
        Err(MemError::unsupported())
    }
}

impl MemAdvise for LinuxMem {
    unsafe fn advise(&self, region: Region, advice: Advise) -> Result<(), MemError> {
        let adv = match advice {
            Advise::Normal => MmAdvice::Normal,
            Advise::Sequential => MmAdvice::Sequential,
            Advise::Random => MmAdvice::Random,
            Advise::WillNeed => MmAdvice::WillNeed,
            Advise::DontNeed => MmAdvice::DontNeed,
            Advise::Free | Advise::NoHugePage | Advise::HugePage => {
                return Err(MemError::unsupported());
            }
        };

        unsafe { mm::madvise(region.base.as_ptr().cast::<c_void>(), region.len, adv) }
            .map_err(Self::map_errno)
    }
}

impl MemLock for LinuxMem {
    unsafe fn lock(&self, region: Region) -> Result<(), MemError> {
        unsafe { mm::mlock(region.base.as_ptr().cast::<c_void>(), region.len) }
            .map_err(Self::map_errno)
    }

    unsafe fn unlock(&self, region: Region) -> Result<(), MemError> {
        unsafe { mm::munlock(region.base.as_ptr().cast::<c_void>(), region.len) }
            .map_err(Self::map_errno)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pal::mem::{MapFlags, RegionAttrs};

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
        let mem = LinuxMem::new();
        let page = mem.page_info().base_page.get();
        let region = unsafe { mem.map(&anon_request(page)) }.expect("map");
        assert_eq!(region.len, page);
        unsafe { mem.unmap(region) }.expect("unmap");
    }

    #[test]
    fn region_len_reports_page_rounded_extent() {
        let mem = LinuxMem::new();
        let page = mem.page_info().base_page.get();
        let region = unsafe { mem.map(&anon_request(page - 1)) }.expect("map");

        assert_eq!(region.len, page);

        unsafe { mem.unmap(region) }.expect("unmap");
    }

    #[test]
    fn fixed_no_replace_rejects_overlap() {
        let mem = LinuxMem::new();
        let page = mem.page_info().base_page.get();
        let region = unsafe { mem.map(&anon_request(page)) }.expect("seed map");

        let mut req = anon_request(page);
        req.placement = Placement::FixedNoReplace(region.base.as_ptr() as usize);
        let err = unsafe { mem.map(&req) }.expect_err("fixed-no-replace should fail");
        assert_eq!(err.kind, MemErrorKind::Busy);

        unsafe { mem.unmap(region) }.expect("cleanup");
    }

    #[test]
    fn replace_mapping_overwrites_existing_region() {
        let mem = LinuxMem::new();
        let page = mem.page_info().base_page.get();
        let region = unsafe { mem.map(&anon_request(page)) }.expect("seed map");

        let replace = MapReplaceRequest {
            len: page,
            align: 0,
            protect: Protect::READ | Protect::WRITE,
            flags: MapFlags::PRIVATE,
            attrs: RegionAttrs::VIRTUAL_ONLY,
            cache: CachePolicy::Default,
            placement: ReplacePlacement::FixedReplace(region.base.as_ptr() as usize),
            backing: Backing::Anonymous,
        };

        let replaced = unsafe { mem.map_replace(&replace) }.expect("replace map");
        assert_eq!(replaced.base, region.base);

        unsafe { mem.unmap(replaced) }.expect("cleanup");
    }
}
