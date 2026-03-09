use core::ffi::c_void;
use core::num::NonZeroUsize;
use core::ptr::NonNull;

use rustix::fd::BorrowedFd;
use rustix::io::Errno;
use rustix::mm::{self, Advice as MmAdvice, MapFlags as MmMapFlags, MprotectFlags, ProtFlags};
use rustix::param;

use crate::pal::mem::{
    Advise, Backing, CachePolicy, IntegrityMode, MapFlags, MapReplaceRequest, MapRequest,
    MemAdvise, MemAttrsControl, MemBase, MemCaps, MemCommit, MemDevice, MemError, MemErrorKind,
    MemIntegrityControl, MemLock, MemMap, MemMapReplace, MemPhysical, MemPool, MemProtect,
    MemQuery, PageInfo, Placement, PoolAccess, PoolBackingKind, PoolBounds, PoolCapabilitySet,
    PoolError, PoolHandle, PoolHazardSet, PoolLatency, PoolPreference, PoolPreferenceSet,
    PoolProhibition, PoolRequest, PoolRequirement, PoolSharing, Protect, Region, RegionAttrs,
    RegionInfo, ReplacePlacement, ResolvedPoolConfig, TagMode,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxMem;

#[derive(Debug)]
pub struct LinuxPoolHandle {
    region: Region,
    page_size: usize,
}

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

        if req.flags.contains(MapFlags::STACK) {
            flags |= MmMapFlags::STACK;
        }

        if req.flags.contains(MapFlags::GROWSDOWN) {
            flags |= MmMapFlags::GROWSDOWN;
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

    fn coerce_region(ptr: *mut c_void, len: usize) -> Result<Region, MemError> {
        let base = NonNull::new(ptr.cast::<u8>()).ok_or(MemError::invalid_addr())?;
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

impl PoolHandle for LinuxPoolHandle {
    fn region(&self) -> Region {
        self.region
    }

    fn page_size(&self) -> usize {
        self.page_size
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

impl MemCommit for LinuxMem {
    unsafe fn commit(&self, region: Region, protect: Protect) -> Result<(), MemError> {
        unsafe { self.protect(region, protect) }
    }

    unsafe fn decommit(&self, region: Region) -> Result<(), MemError> {
        let _ = unsafe {
            mm::madvise(
                region.base.as_ptr().cast::<c_void>(),
                region.len,
                MmAdvice::DontNeed,
            )
        };

        unsafe { self.protect(region, Protect::NONE) }
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

unsafe impl MemAttrsControl for LinuxMem {
    unsafe fn set_cache_policy(
        &self,
        _region: Region,
        _policy: CachePolicy,
    ) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

unsafe impl MemIntegrityControl for LinuxMem {
    unsafe fn set_tag_mode(&self, _region: Region, _mode: TagMode) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }

    unsafe fn set_integrity_mode(
        &self,
        _region: Region,
        _mode: IntegrityMode,
    ) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

unsafe impl MemPhysical for LinuxMem {}
unsafe impl MemDevice for LinuxMem {}

impl MemPool for LinuxMem {
    type PoolHandle = LinuxPoolHandle;

    fn create_pool(
        &self,
        request: &PoolRequest<'_>,
    ) -> Result<(Self::PoolHandle, ResolvedPoolConfig), PoolError> {
        let page_info = self.page_info();
        let page_size = page_info.alloc_granule.get();

        let capacity = aligned_capacity(request.bounds, page_size)?;
        let mut bounds = request.bounds;
        bounds.initial_capacity = capacity;
        bounds.max_capacity = Some(capacity);
        bounds.growable = false;

        if matches!(request.sharing, PoolSharing::Shared) {
            return Err(PoolError::unsupported_requirement());
        }

        let mem_caps = self.caps();
        let mut granted = pool_caps_from_mem(mem_caps);
        let mut unmet = PoolPreferenceSet::empty();
        let emulated = PoolCapabilitySet::empty();
        let mut hazards = PoolHazardSet::empty();

        let protect = match request.access {
            PoolAccess::ReadWrite => Protect::READ | Protect::WRITE,
            PoolAccess::ReadWriteExecute => {
                hazards |= PoolHazardSet::EXECUTABLE;
                granted |= PoolCapabilitySet::EXECUTABLE;
                Protect::READ | Protect::WRITE | Protect::EXEC
            }
        };

        for prohibition in request.prohibitions {
            match prohibition {
                PoolProhibition::Executable => {
                    if protect.contains(Protect::EXEC) {
                        return Err(PoolError::prohibition_violated());
                    }
                }
                PoolProhibition::Overcommit => {
                    return Err(PoolError::unsupported_requirement());
                }
                PoolProhibition::Shared => {
                    if matches!(request.sharing, PoolSharing::Shared) {
                        return Err(PoolError::prohibition_violated());
                    }
                }
                PoolProhibition::ReplaceMapping
                | PoolProhibition::DeviceLocal
                | PoolProhibition::Physical
                | PoolProhibition::Emulation => {}
            }
        }

        let mut required_placement = None;
        let mut preferred_placement = None;
        let mut map_flags = MapFlags::PRIVATE;
        let mut prefer_populate = false;
        let mut prefer_lock = false;

        match request.latency {
            PoolLatency::BestEffort => {}
            PoolLatency::Prefault => {
                map_flags |= MapFlags::POPULATE;
                granted |= PoolCapabilitySet::POPULATE;
            }
            PoolLatency::Locked => {
                if !mem_caps.contains(MemCaps::LOCK) {
                    return Err(PoolError::unsupported_requirement());
                }
                map_flags |= MapFlags::LOCKED;
                granted |= PoolCapabilitySet::LOCKABLE;
            }
        }

        for requirement in request.requirements {
            match *requirement {
                PoolRequirement::Placement(placement) => {
                    validate_required_placement(placement, mem_caps)?;
                    required_placement = Some(placement);
                }
                PoolRequirement::Query => {
                    if !mem_caps.contains(MemCaps::QUERY) {
                        return Err(PoolError::unsupported_requirement());
                    }
                    granted |= PoolCapabilitySet::QUERY;
                }
                PoolRequirement::Locked => {
                    if !mem_caps.contains(MemCaps::LOCK) {
                        return Err(PoolError::unsupported_requirement());
                    }
                    map_flags |= MapFlags::LOCKED;
                    granted |= PoolCapabilitySet::LOCKABLE;
                }
                PoolRequirement::NoOvercommit => {
                    return Err(PoolError::unsupported_requirement());
                }
                PoolRequirement::CachePolicy(policy) => {
                    if policy != CachePolicy::Default {
                        return Err(PoolError::unsupported_requirement());
                    }
                }
                PoolRequirement::Integrity(_)
                | PoolRequirement::DmaVisible
                | PoolRequirement::PhysicalContiguous
                | PoolRequirement::DeviceLocal
                | PoolRequirement::Shared
                | PoolRequirement::ZeroOnFree => {
                    return Err(PoolError::unsupported_requirement());
                }
            }
        }

        for preference in request.preferences {
            match *preference {
                PoolPreference::Placement(placement) => {
                    if required_placement.is_none()
                        && validate_required_placement(placement, mem_caps).is_ok()
                    {
                        preferred_placement = Some(placement);
                    } else {
                        unmet |= PoolPreferenceSet::PLACEMENT;
                    }
                }
                PoolPreference::Populate => {
                    if !map_flags.contains(MapFlags::POPULATE) {
                        prefer_populate = true;
                    }
                }
                PoolPreference::Lock => {
                    if !map_flags.contains(MapFlags::LOCKED) && mem_caps.contains(MemCaps::LOCK) {
                        prefer_lock = true;
                    } else if !mem_caps.contains(MemCaps::LOCK) {
                        unmet |= PoolPreferenceSet::LOCK;
                    }
                }
                PoolPreference::HugePages => {
                    unmet |= PoolPreferenceSet::HUGE_PAGES;
                }
                PoolPreference::ZeroOnFree => {
                    unmet |= PoolPreferenceSet::ZERO_ON_FREE;
                }
            }
        }

        let required_request = base_pool_request(capacity, protect, map_flags, required_placement);
        let mut preferred_flags = map_flags;
        if prefer_lock {
            preferred_flags |= MapFlags::LOCKED;
        }
        if prefer_populate {
            preferred_flags |= MapFlags::POPULATE;
        }
        let preferred_request = base_pool_request(
            capacity,
            protect,
            preferred_flags,
            preferred_placement.or(required_placement),
        );

        let (region, final_placement, final_flags) = match unsafe { self.map(&preferred_request) } {
            Ok(region) => (region, preferred_request.placement, preferred_request.flags),
            Err(error) => {
                if preferred_request.flags == required_request.flags
                    && preferred_request.placement == required_request.placement
                {
                    return Err(error.into());
                }

                if prefer_lock {
                    unmet |= PoolPreferenceSet::LOCK;
                }
                if prefer_populate {
                    unmet |= PoolPreferenceSet::POPULATE;
                }
                if preferred_placement.is_some() {
                    unmet |= PoolPreferenceSet::PLACEMENT;
                }

                let region = unsafe { self.map(&required_request) }?;
                (region, required_request.placement, required_request.flags)
            }
        };

        if final_flags.contains(MapFlags::LOCKED) {
            granted |= PoolCapabilitySet::LOCKABLE;
        }
        if final_flags.contains(MapFlags::POPULATE) {
            granted |= PoolCapabilitySet::POPULATE;
        }
        if let Placement::FixedNoReplace(_) = final_placement {
            granted |= PoolCapabilitySet::FIXED_NOREPLACE;
        }

        let resolved = ResolvedPoolConfig {
            backing: PoolBackingKind::AnonymousPrivate,
            bounds,
            granted_capabilities: granted,
            unmet_preferences: unmet,
            emulated_capabilities: emulated,
            residual_hazards: hazards,
        };

        let handle = LinuxPoolHandle { region, page_size };
        Ok((handle, resolved))
    }

    unsafe fn destroy_pool(&self, pool: Self::PoolHandle) -> Result<(), PoolError> {
        unsafe { self.unmap(pool.region) }.map_err(Into::into)
    }
}

fn pool_caps_from_mem(mem_caps: MemCaps) -> PoolCapabilitySet {
    let mut out = PoolCapabilitySet::PRIVATE_BACKING;

    if mem_caps.contains(MemCaps::LOCK) {
        out |= PoolCapabilitySet::LOCKABLE;
    }
    if mem_caps.contains(MemCaps::MAP_FIXED_NOREPLACE) {
        out |= PoolCapabilitySet::FIXED_NOREPLACE;
    }
    if mem_caps.contains(MemCaps::ADVISE) {
        out |= PoolCapabilitySet::ADVISE;
    }
    if mem_caps.contains(MemCaps::QUERY) {
        out |= PoolCapabilitySet::QUERY;
    }
    if mem_caps.contains(MemCaps::PHYSICAL_MAP) {
        out |= PoolCapabilitySet::PHYSICAL;
    }
    if mem_caps.contains(MemCaps::DEVICE_MAP) {
        out |= PoolCapabilitySet::DEVICE_LOCAL;
    }
    if mem_caps.contains(MemCaps::INTEGRITY_CONTROL) {
        out |= PoolCapabilitySet::INTEGRITY;
    }
    if mem_caps.contains(MemCaps::CACHE_POLICY) {
        out |= PoolCapabilitySet::CACHE_POLICY;
    }

    out
}

fn validate_required_placement(placement: Placement, mem_caps: MemCaps) -> Result<(), PoolError> {
    match placement {
        Placement::Anywhere | Placement::Hint(_) => Ok(()),
        Placement::FixedNoReplace(_) if mem_caps.contains(MemCaps::MAP_FIXED_NOREPLACE) => Ok(()),
        Placement::FixedNoReplace(_) => Err(PoolError::unsupported_requirement()),
        Placement::PreferredNode(_) | Placement::RequiredNode(_) | Placement::RegionId(_) => {
            Err(PoolError::unsupported_requirement())
        }
    }
}

fn aligned_capacity(bounds: PoolBounds, granule: usize) -> Result<usize, PoolError> {
    if bounds.initial_capacity == 0 {
        return Err(PoolError::invalid_request());
    }

    if bounds.growable {
        return Err(PoolError::unsupported_requirement());
    }

    if let Some(max) = bounds.max_capacity {
        if max < bounds.initial_capacity {
            return Err(PoolError::invalid_request());
        }
    }

    align_up(bounds.initial_capacity, granule).ok_or_else(PoolError::invalid_request)
}

fn base_pool_request(
    capacity: usize,
    protect: Protect,
    mut flags: MapFlags,
    placement: Option<Placement>,
) -> MapRequest<'static> {
    let placement = placement.unwrap_or(Placement::Anywhere);
    if matches!(
        placement,
        Placement::Anywhere | Placement::Hint(_) | Placement::FixedNoReplace(_)
    ) {
        flags |= MapFlags::PRIVATE;
    }

    MapRequest {
        len: capacity,
        align: 0,
        protect,
        flags,
        attrs: RegionAttrs::VIRTUAL_ONLY,
        cache: CachePolicy::Default,
        placement,
        backing: Backing::Anonymous,
    }
}

fn align_up(value: usize, align: usize) -> Option<usize> {
    if align == 0 {
        return Some(value);
    }

    let mask = align.checked_sub(1)?;
    value.checked_add(mask).map(|v| v & !mask)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pal::mem::{
        MapFlags, PoolAccess, PoolErrorKind, PoolProhibition, PoolRequest, PoolRequirement,
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
        let mem = LinuxMem::new();
        let page = mem.page_info().base_page.get();
        let region = unsafe { mem.map(&anon_request(page)) }.expect("map");
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

    #[test]
    fn creates_pool_backing() {
        let mem = LinuxMem::new();
        let (pool, resolved) = mem
            .create_pool(&PoolRequest::anonymous_private(16 * 1024))
            .expect("pool backing");

        assert_eq!(resolved.backing, PoolBackingKind::AnonymousPrivate);
        assert_eq!(pool.region().len, 16 * 1024);

        unsafe { mem.destroy_pool(pool) }.expect("destroy");
    }

    #[test]
    fn pool_rejects_unsupported_requirement() {
        let mem = LinuxMem::new();
        let requirements = [PoolRequirement::DmaVisible];
        let request = PoolRequest {
            requirements: &requirements,
            ..PoolRequest::anonymous_private(16 * 1024)
        };

        let err = mem
            .create_pool(&request)
            .expect_err("dma-visible should fail");
        assert_eq!(err.kind, PoolErrorKind::UnsupportedRequirement);
    }

    #[test]
    fn pool_enforces_executable_prohibition() {
        let mem = LinuxMem::new();
        let prohibitions = [PoolProhibition::Executable];
        let request = PoolRequest {
            access: PoolAccess::ReadWriteExecute,
            prohibitions: &prohibitions,
            ..PoolRequest::anonymous_private(16 * 1024)
        };

        let err = mem
            .create_pool(&request)
            .expect_err("executable should be prohibited");
        assert_eq!(err.kind, PoolErrorKind::ProhibitionViolated);
    }
}
