//! Linux fusion-pal implementation of the low-level memory contract.
//!
//! This backend exposes only Linux semantics it can actually perform through
//! `mmap`, `mprotect`, `madvise`, `mlock`, and `/proc/self/maps`. Anything
//! stronger or more exotic is rejected rather than quietly emulated.

use core::ffi::c_void;
use core::num::NonZeroUsize;
use core::str;
use core::sync::atomic::{AtomicU64, Ordering};

use rustix::fd::BorrowedFd;
use rustix::fs::{CWD, Mode, OFlags, openat};
use rustix::io::Errno;
use rustix::io::read;
use rustix::mm::{self, Advice as MmAdvice, MapFlags as MmMapFlags, MprotectFlags, ProtFlags};
use rustix::param;
use rustix::system;

use crate::contract::hardware::mem::{
    Address,
    Advise,
    Backing,
    CachePolicy,
    MapFlags,
    MapReplaceRequest,
    MapRequest,
    MemAdviceCaps,
    MemAdvise,
    MemBackingCaps,
    MemBase,
    MemCaps,
    MemCommit,
    MemError,
    MemErrorKind,
    MemLock,
    MemMap,
    MemMapReplace,
    MemPlacementCaps,
    MemProtect,
    MemQuery,
    MemSupport,
    PageInfo,
    Placement,
    Protect,
    Region,
    RegionAttrs,
    RegionInfo,
    ReplacePlacement,
};
use crate::sys::sync::{OnceBeginResult, PlatformRawOnce, RawOnce};

/// Linux implementation of the fusion-pal memory provider contract.
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxMem;

/// Target-selected fusion-pal memory provider alias for Linux builds.
pub type PlatformMem = LinuxMem;

#[allow(clippy::useless_nonzero_new_unchecked)]
const DEFAULT_PAGE_SIZE: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(4096) };

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct KernelVersion {
    major: u16,
    minor: u16,
    patch: u16,
}

const LINUX_2_5_37: KernelVersion = KernelVersion {
    major: 2,
    minor: 5,
    patch: 37,
};
const LINUX_2_6_23: KernelVersion = KernelVersion {
    major: 2,
    minor: 6,
    patch: 23,
};
const LINUX_2_6_38: KernelVersion = KernelVersion {
    major: 2,
    minor: 6,
    patch: 38,
};
const LINUX_4_5_0: KernelVersion = KernelVersion {
    major: 4,
    minor: 5,
    patch: 0,
};
const LINUX_4_17_0: KernelVersion = KernelVersion {
    major: 4,
    minor: 17,
    patch: 0,
};

const KERNEL_VERSION_UNAVAILABLE: u64 = 0;

static KERNEL_VERSION_CACHE: AtomicU64 = AtomicU64::new(KERNEL_VERSION_UNAVAILABLE);
static KERNEL_VERSION_ONCE: PlatformRawOnce = PlatformRawOnce::new();

/// Returns the process-wide Linux memory provider handle.
#[must_use]
pub const fn system_mem() -> PlatformMem {
    PlatformMem::new()
}

impl LinuxMem {
    /// Creates a new Linux fusion-pal memory provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    const fn map_errno(errno: Errno) -> MemError {
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

    fn kernel_version() -> Option<KernelVersion> {
        loop {
            match KERNEL_VERSION_ONCE.begin() {
                Ok(OnceBeginResult::Complete) => {
                    return decode_kernel_version(KERNEL_VERSION_CACHE.load(Ordering::Acquire));
                }
                Ok(OnceBeginResult::InProgress) => {
                    if KERNEL_VERSION_ONCE.wait().is_err() {
                        return parse_kernel_release(system::uname().release().to_bytes());
                    }
                }
                Ok(OnceBeginResult::Initialize) => {
                    let detected = parse_kernel_release(system::uname().release().to_bytes());
                    KERNEL_VERSION_CACHE.store(encode_kernel_version(detected), Ordering::Release);
                    // SAFETY: this path owns the one-time initialization right returned by begin.
                    unsafe { KERNEL_VERSION_ONCE.complete_unchecked() };
                    return detected;
                }
                Err(_) => return parse_kernel_release(system::uname().release().to_bytes()),
            }
        }
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
            Backing::Anonymous | Backing::File { .. } => {}
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

    fn validate_common<P>(req: &MapRequest<'_, P>) -> Result<(), MemError> {
        let version = Self::kernel_version();

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

        if req.flags.contains(MapFlags::LOCKED) && !supports_map_locked_at(version) {
            return Err(MemError::unsupported());
        }

        if req.flags.contains(MapFlags::POPULATE) && !supports_map_populate_at(version) {
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
            let offset = usize::try_from(offset).map_err(|_| MemError::overflow())?;
            if !offset.is_multiple_of(page) {
                return Err(MemError::misaligned());
            }
        }

        Ok(())
    }

    fn validate_safe_placement(placement: Placement) -> Result<(), MemError> {
        let version = Self::kernel_version();
        let page = Self::page_size_raw();

        match placement {
            Placement::Anywhere => Ok(()),
            Placement::Hint(addr) | Placement::FixedNoReplace(addr) => {
                if matches!(placement, Placement::FixedNoReplace(_))
                    && !supports_fixed_noreplace_at(version)
                {
                    return Err(MemError::unsupported());
                }
                if addr.is_multiple_of(page) {
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

    const fn addr_hint(placement: Placement) -> Result<*mut c_void, MemError> {
        match placement {
            Placement::Anywhere => Ok(core::ptr::null_mut()),
            Placement::Hint(addr) | Placement::FixedNoReplace(addr) => Ok(addr as *mut c_void),
            Placement::PreferredNode(_) | Placement::RequiredNode(_) | Placement::RegionId(_) => {
                Err(MemError::unsupported())
            }
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

    fn enforce_no_replace(region: Region, requested: Placement) -> Result<Region, MemError> {
        match requested {
            Placement::FixedNoReplace(addr) if region.base.get() != addr => {
                // Older kernels may ignore `MAP_FIXED_NOREPLACE` as a non-binding placement hint.
                // If that happens, the returned address will not match and we fail closed here.
                let _ = unsafe { mm::munmap(region.base.as_ptr().cast::<c_void>(), region.len) };
                Err(MemError::busy())
            }
            _ => Ok(region),
        }
    }

    fn query_proc_maps(addr: usize) -> Result<RegionInfo, MemError> {
        let fd = openat(CWD, "/proc/self/maps", OFlags::RDONLY, Mode::empty())
            .map_err(Self::map_errno)?;
        let mut read_buf = [0_u8; 4096];
        // Linux maps lines are usually far shorter than this, but deeply nested or synthetic
        // pathnames can exceed the buffer. In that case we fail closed with `overflow` rather
        // than risk parsing a truncated line into fiction.
        let mut line_buf = [0_u8; 4096];
        let mut line_len = 0_usize;

        loop {
            let nread = read(&fd, &mut read_buf).map_err(Self::map_errno)?;
            if nread == 0 {
                break;
            }

            for byte in &read_buf[..nread] {
                if *byte == b'\n' {
                    if let Some(info) = Self::parse_maps_line(&line_buf[..line_len], addr) {
                        return Ok(info);
                    }
                    line_len = 0;
                    continue;
                }

                if line_len == line_buf.len() {
                    return Err(MemError::overflow());
                }

                line_buf[line_len] = *byte;
                line_len += 1;
            }
        }

        if line_len != 0
            && let Some(info) = Self::parse_maps_line(&line_buf[..line_len], addr)
        {
            return Ok(info);
        }

        Err(MemError::invalid_addr())
    }

    fn parse_maps_line(line: &[u8], addr: usize) -> Option<RegionInfo> {
        let text = str::from_utf8(line).ok()?;
        let mut fields = text.split_ascii_whitespace();
        let range = fields.next()?;
        let perms = fields.next()?;
        let (start, end) = range.split_once('-')?;
        let start = usize::from_str_radix(start, 16).ok()?;
        let end = usize::from_str_radix(end, 16).ok()?;

        if addr < start || addr >= end {
            return None;
        }

        let mut protect = Protect::empty();
        let perm_bytes = perms.as_bytes();
        if perm_bytes.first() == Some(&b'r') {
            protect |= Protect::READ;
        }
        if perm_bytes.get(1) == Some(&b'w') {
            protect |= Protect::WRITE;
        }
        if perm_bytes.get(2) == Some(&b'x') {
            protect |= Protect::EXEC;
        }

        let mut attrs = RegionAttrs::VIRTUAL_ONLY;
        if protect.contains(Protect::EXEC) {
            attrs |= RegionAttrs::EXECUTABLE;
        }

        Some(RegionInfo {
            region: Region {
                base: Address::new(start),
                len: end.checked_sub(start)?,
            },
            protect,
            attrs,
            cache: CachePolicy::Default,
            placement: Placement::Anywhere,
            committed: true,
        })
    }
}

impl MemBase for LinuxMem {
    fn caps(&self) -> MemCaps {
        let version = Self::kernel_version();
        let mut caps = MemCaps::MAP_ANON
            | MemCaps::MAP_FILE
            | MemCaps::MAP_FIXED_REPLACE
            | MemCaps::MAP_HINT
            | MemCaps::PROTECT
            | MemCaps::ADVISE
            | MemCaps::LOCK
            | MemCaps::QUERY
            | MemCaps::EXECUTE_MAP;

        if supports_fixed_noreplace_at(version) {
            caps |= MemCaps::MAP_FIXED_NOREPLACE;
        }

        caps
    }

    fn support(&self) -> MemSupport {
        let version = Self::kernel_version();
        let mut map_flags = MapFlags::PRIVATE | MapFlags::SHARED;
        if supports_map_populate_at(version) {
            map_flags |= MapFlags::POPULATE;
        }
        if supports_map_locked_at(version) {
            map_flags |= MapFlags::LOCKED;
        }

        let mut placements = MemPlacementCaps::ANYWHERE | MemPlacementCaps::HINT;
        if supports_fixed_noreplace_at(version) {
            placements |= MemPlacementCaps::FIXED_NOREPLACE;
        }

        let mut advice = MemAdviceCaps::NORMAL
            | MemAdviceCaps::SEQUENTIAL
            | MemAdviceCaps::RANDOM
            | MemAdviceCaps::WILL_NEED
            | MemAdviceCaps::DONT_NEED;
        if supports_advise_free_at(version) {
            advice |= MemAdviceCaps::FREE;
        }
        if supports_thp_advice_at(version) {
            advice |= MemAdviceCaps::NO_HUGE_PAGE | MemAdviceCaps::HUGE_PAGE;
        }

        MemSupport {
            caps: self.caps(),
            map_flags,
            protect: Protect::READ | Protect::WRITE | Protect::EXEC,
            backings: MemBackingCaps::ANON_PRIVATE
                | MemBackingCaps::ANON_SHARED
                | MemBackingCaps::FILE_PRIVATE
                | MemBackingCaps::FILE_SHARED,
            placements,
            advice,
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

impl MemMap for LinuxMem {
    unsafe fn map(&self, req: &MapRequest<'_>) -> Result<Region, MemError> {
        Self::validate_common(req)?;
        Self::validate_safe_placement(req.placement)?;

        let prot = Self::to_mmap_prot(req.protect)?;
        let mut flags = Self::to_common_mmap_flags(req)?;

        if let Placement::FixedNoReplace(_) = req.placement {
            flags |= MmMapFlags::FIXED_NOREPLACE;
        }

        let addr_hint = Self::addr_hint(req.placement)?;
        let ptr = match req.backing {
            Backing::Anonymous => unsafe { mm::mmap_anonymous(addr_hint, req.len, prot, flags) },
            Backing::File { fd, offset } => {
                let raw_fd = fd.as_raw_fd().map_err(|_| MemError::invalid())?;
                let fd = unsafe { BorrowedFd::borrow_raw(raw_fd) };
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
        Self::validate_common(req)?;
        Self::validate_replace_placement(req.placement)?;

        let prot = Self::to_mmap_prot(req.protect)?;
        let mut flags = Self::to_common_mmap_flags(req)?;
        flags |= MmMapFlags::FIXED;

        let addr = Self::replace_addr(req.placement);
        let ptr = match req.backing {
            Backing::Anonymous => unsafe { mm::mmap_anonymous(addr, req.len, prot, flags) },
            Backing::File { fd, offset } => {
                let raw_fd = fd.as_raw_fd().map_err(|_| MemError::invalid())?;
                let fd = unsafe { BorrowedFd::borrow_raw(raw_fd) };
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

impl MemCommit for LinuxMem {}

impl MemQuery for LinuxMem {
    fn query(&self, addr: Address) -> Result<RegionInfo, MemError> {
        Self::query_proc_maps(addr.get())
    }
}

impl MemAdvise for LinuxMem {
    unsafe fn advise(&self, region: Region, advice: Advise) -> Result<(), MemError> {
        let version = Self::kernel_version();
        match advice {
            Advise::Free if !supports_advise_free_at(version) => {
                return Err(MemError::unsupported());
            }
            Advise::NoHugePage | Advise::HugePage if !supports_thp_advice_at(version) => {
                return Err(MemError::unsupported());
            }
            _ => {}
        }

        let adv = match advice {
            Advise::Normal => MmAdvice::Normal,
            Advise::Sequential => MmAdvice::Sequential,
            Advise::Random => MmAdvice::Random,
            Advise::WillNeed => MmAdvice::WillNeed,
            Advise::DontNeed => MmAdvice::DontNeed,
            Advise::Free => MmAdvice::LinuxFree,
            Advise::NoHugePage => MmAdvice::LinuxNoHugepage,
            Advise::HugePage => MmAdvice::LinuxHugepage,
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

impl crate::contract::hardware::mem::MemCatalog for LinuxMem {}

fn parse_kernel_release(release: &[u8]) -> Option<KernelVersion> {
    let (major, rest) = parse_release_component(release)?;
    let (minor, rest) = parse_optional_release_component(rest)?;
    let (patch, _) = parse_optional_release_component(rest)?;

    Some(KernelVersion {
        major: u16::try_from(major).ok()?,
        minor: u16::try_from(minor).ok()?,
        patch: u16::try_from(patch).ok()?,
    })
}

fn parse_optional_release_component(bytes: &[u8]) -> Option<(u32, &[u8])> {
    if bytes.first().copied() != Some(b'.') {
        return Some((0, bytes));
    }

    parse_release_component(&bytes[1..])
}

fn parse_release_component(bytes: &[u8]) -> Option<(u32, &[u8])> {
    let mut value = 0u32;
    let mut index = 0usize;

    while let Some(byte) = bytes.get(index).copied() {
        match byte {
            b'0'..=b'9' => {
                value = value.checked_mul(10)?.checked_add(u32::from(byte - b'0'))?;
                index += 1;
            }
            _ => break,
        }
    }

    if index == 0 {
        return None;
    }

    Some((value, &bytes[index..]))
}

const fn encode_kernel_version(version: Option<KernelVersion>) -> u64 {
    match version {
        Some(version) => {
            ((version.major as u64) << 32) | ((version.minor as u64) << 16) | version.patch as u64
        }
        None => KERNEL_VERSION_UNAVAILABLE,
    }
}

const fn decode_kernel_version(encoded: u64) -> Option<KernelVersion> {
    if encoded == KERNEL_VERSION_UNAVAILABLE {
        return None;
    }

    Some(KernelVersion {
        major: ((encoded >> 32) & 0xffff) as u16,
        minor: ((encoded >> 16) & 0xffff) as u16,
        patch: (encoded & 0xffff) as u16,
    })
}

const fn kernel_at_least(actual: Option<KernelVersion>, required: KernelVersion) -> bool {
    match actual {
        Some(version) => {
            version.major > required.major
                || (version.major == required.major
                    && (version.minor > required.minor
                        || (version.minor == required.minor && version.patch >= required.patch)))
        }
        None => false,
    }
}

const fn supports_map_locked_at(version: Option<KernelVersion>) -> bool {
    kernel_at_least(version, LINUX_2_5_37)
}

const fn supports_map_populate_at(version: Option<KernelVersion>) -> bool {
    // Linux added MAP_POPULATE in 2.5.46, but private mappings were not covered until 2.6.23.
    // This fusion-pal exposes POPULATE generically across private and shared mappings, so stay
    // conservative and only advertise it once both common cases are covered.
    kernel_at_least(version, LINUX_2_6_23)
}

const fn supports_fixed_noreplace_at(version: Option<KernelVersion>) -> bool {
    kernel_at_least(version, LINUX_4_17_0)
}

const fn supports_advise_free_at(version: Option<KernelVersion>) -> bool {
    kernel_at_least(version, LINUX_4_5_0)
}

const fn supports_thp_advice_at(version: Option<KernelVersion>) -> bool {
    kernel_at_least(version, LINUX_2_6_38)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::hardware::mem::{MapFlags, MemAdviceCaps, MemPlacementCaps, RegionAttrs};

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
            placement: ReplacePlacement::FixedReplace(region.base.get()),
            backing: Backing::Anonymous,
        };

        let replaced = unsafe { mem.map_replace(&replace) }.expect("replace map");
        assert_eq!(replaced.base, region.base);

        unsafe { mem.unmap(replaced) }.expect("cleanup");
    }

    #[test]
    fn query_reports_mapped_region() {
        let mem = LinuxMem::new();
        let page = mem.page_info().base_page.get();
        let region = unsafe { mem.map(&anon_request(page)) }.expect("map");
        let info = mem.query(region.base).expect("query");

        assert!(info.region.contains(region.base.get()));
        assert!(info.region.len >= region.len);
        assert!(info.protect.contains(Protect::READ));
        assert!(info.protect.contains(Protect::WRITE));

        unsafe { mem.unmap(region) }.expect("cleanup");
    }

    #[test]
    fn huge_page_advice_is_supported() {
        let mem = LinuxMem::new();
        if !mem.support().advice.contains(MemAdviceCaps::HUGE_PAGE) {
            return;
        }
        let page = mem.page_info().base_page.get();
        let region = unsafe { mem.map(&anon_request(page)) }.expect("map");

        unsafe { mem.advise(region, Advise::HugePage) }.expect("advise");

        unsafe { mem.unmap(region) }.expect("cleanup");
    }

    #[test]
    fn parses_kernel_release_with_distribution_suffix() {
        let version = parse_kernel_release(b"6.8.12-arch1-1").expect("version");

        assert_eq!(
            version,
            KernelVersion {
                major: 6,
                minor: 8,
                patch: 12,
            }
        );
    }

    #[test]
    fn parses_bare_major_kernel_release() {
        let version = parse_kernel_release(b"3").expect("version");

        assert_eq!(
            version,
            KernelVersion {
                major: 3,
                minor: 0,
                patch: 0,
            }
        );
    }

    #[test]
    fn rejects_empty_kernel_release() {
        assert_eq!(parse_kernel_release(b""), None);
    }

    #[test]
    fn parses_centos_legacy_kernel_release() {
        let version = parse_kernel_release(b"2.6.32-696.el6.x86_64").expect("version");

        assert_eq!(
            version,
            KernelVersion {
                major: 2,
                minor: 6,
                patch: 32,
            }
        );
    }

    #[test]
    fn version_gates_linux_capabilities_conservatively() {
        let before_fixed = Some(KernelVersion {
            major: 4,
            minor: 16,
            patch: 9,
        });
        let fixed = Some(KernelVersion {
            major: 4,
            minor: 17,
            patch: 0,
        });
        let before_free = Some(KernelVersion {
            major: 4,
            minor: 4,
            patch: 302,
        });
        let free = Some(KernelVersion {
            major: 4,
            minor: 5,
            patch: 0,
        });
        let before_thp = Some(KernelVersion {
            major: 2,
            minor: 6,
            patch: 37,
        });
        let thp = Some(LINUX_2_6_38);
        let before_populate = Some(KernelVersion {
            major: 2,
            minor: 6,
            patch: 22,
        });
        let populate = Some(LINUX_2_6_23);

        assert!(!supports_fixed_noreplace_at(before_fixed));
        assert!(supports_fixed_noreplace_at(fixed));
        assert!(!supports_advise_free_at(before_free));
        assert!(supports_advise_free_at(free));
        assert!(!supports_thp_advice_at(before_thp));
        assert!(supports_thp_advice_at(thp));
        assert!(!supports_map_populate_at(before_populate));
        assert!(supports_map_populate_at(populate));
        assert!(supports_map_locked_at(Some(LINUX_2_5_37)));
    }

    #[test]
    fn runtime_support_matches_kernel_version_gates() {
        let mem = LinuxMem::new();
        let version = LinuxMem::kernel_version();
        let support = mem.support();
        let caps = mem.caps();

        assert_eq!(
            caps.contains(MemCaps::MAP_FIXED_NOREPLACE),
            supports_fixed_noreplace_at(version)
        );
        assert_eq!(
            support
                .placements
                .contains(MemPlacementCaps::FIXED_NOREPLACE),
            supports_fixed_noreplace_at(version)
        );
        assert_eq!(
            support.map_flags.contains(MapFlags::LOCKED),
            supports_map_locked_at(version)
        );
        assert_eq!(
            support.map_flags.contains(MapFlags::POPULATE),
            supports_map_populate_at(version)
        );
        assert_eq!(
            support.advice.contains(MemAdviceCaps::FREE),
            supports_advise_free_at(version)
        );
        assert_eq!(
            support.advice.contains(MemAdviceCaps::HUGE_PAGE),
            supports_thp_advice_at(version)
        );
        assert_eq!(
            support.advice.contains(MemAdviceCaps::NO_HUGE_PAGE),
            supports_thp_advice_at(version)
        );
    }
}
