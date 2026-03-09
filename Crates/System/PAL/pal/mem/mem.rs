//! Platform memory contract for `fusion-pal`.
//!
//! This module defines the low-level, platform-truthful memory vocabulary used by the
//! selected PAL backend. The types here describe what a backend can actually map,
//! protect, query, lock, or otherwise manipulate without inventing fake portability.
//!
//! Safe intent and hazardous operations are modeled separately. Ordinary placement uses
//! [`Placement`], while destructive overwrite mapping uses [`ReplacePlacement`] and an
//! explicit unsafe trait boundary.

use core::fmt;
use core::num::NonZeroUsize;
use core::ptr::NonNull;

bitflags::bitflags! {
    /// Backend-native capabilities reported by a [`MemBase`] provider.
    ///
    /// These flags describe operations the backend knows how to perform at all. They are
    /// not, by themselves, a guarantee that every combination of request fields is valid.
    /// Callers still have to issue a concrete request and handle rejection.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MemCaps: u64 {
        /// Supports anonymous mappings with no file or device backing object.
        const MAP_ANON              = 1 << 0;
        /// Supports mappings backed by a file-like object or equivalent OS handle.
        const MAP_FILE              = 1 << 1;
        /// Supports exact-address mapping that fails rather than replacing an existing mapping.
        const MAP_FIXED_NOREPLACE   = 1 << 2;
        /// Supports exact-address mapping that may replace an existing mapping in the range.
        ///
        /// This capability is hazardous and is intended for [`MemMapReplace`], not ordinary
        /// safe placement.
        const MAP_FIXED_REPLACE     = 1 << 3;
        /// Supports an address hint that the backend may ignore if it cannot honor it safely.
        const MAP_HINT              = 1 << 4;
        /// Supports changing protection on an existing region through [`MemProtect::protect`].
        const PROTECT               = 1 << 5;
        /// Supports advisory usage hints through [`MemAdvise::advise`].
        const ADVISE                = 1 << 6;
        /// Supports locking or pinning pages to improve residency guarantees.
        const LOCK                  = 1 << 7;
        /// Supports querying region metadata from an arbitrary address.
        const QUERY                 = 1 << 8;
        /// Supports reserving address space without fully materializing the backing yet.
        const RESERVE_ONLY          = 1 << 9;
        /// Supports an explicit commit step for reserved memory.
        const COMMIT_CONTROL        = 1 << 10;
        /// Supports explicitly releasing committed backing while keeping the reservation alive.
        const DECOMMIT_CONTROL      = 1 << 11;
        /// Supports huge page mappings or equivalent large-granule backing.
        const HUGE_PAGES            = 1 << 12;
        /// Supports NUMA-aware placement hints or requirements.
        const NUMA_HINTS            = 1 << 13;
        /// Supports mapping physical memory directly.
        const PHYSICAL_MAP          = 1 << 14;
        /// Supports mapping device-local memory or MMIO-style regions.
        const DEVICE_MAP            = 1 << 15;
        /// Supports memory with hardware tag semantics distinct from ordinary memory.
        const TAGGED_MEMORY         = 1 << 16;
        /// Supports changing integrity or tag-management policy after acquisition.
        const INTEGRITY_CONTROL     = 1 << 17;
        /// Supports requesting or changing a non-default cache policy.
        const CACHE_POLICY          = 1 << 18;
        /// Supports executable mappings.
        const EXECUTE_MAP           = 1 << 19;
    }
}

bitflags::bitflags! {
    /// Access-protection bits for a mapped or reserved region.
    ///
    /// These flags describe the intended access model of a region. Support for a specific
    /// combination is backend-defined.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Protect: u32 {
        /// No access is permitted.
        const NONE    = 0;
        /// Read access is permitted.
        const READ    = 1 << 0;
        /// Write access is permitted.
        const WRITE   = 1 << 1;
        /// Execute access is permitted.
        const EXEC    = 1 << 2;
        /// Guard-page style semantics are requested.
        ///
        /// This is intentionally separate from `NONE`; some platforms expose one-shot or
        /// trap-producing guard behavior that is stronger than simple no-access.
        const GUARD   = 1 << 3;
    }
}

bitflags::bitflags! {
    /// Mapping-construction flags for [`MemMap::map`].
    ///
    /// These flags describe how a region should be created, shared, or populated. They are
    /// part of the request contract and may be rejected on platforms that do not support the
    /// requested semantics.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MapFlags: u32 {
        /// Request shared visibility of the mapping according to backend semantics.
        const SHARED          = 1 << 0;
        /// Request a private copy-on-write or otherwise non-shared mapping.
        const PRIVATE         = 1 << 1;
        /// Prefer or require huge pages for this mapping.
        const HUGE_PAGE       = 1 << 2;
        /// Request eager population or prefaulting rather than lazy fault-on-access.
        const POPULATE        = 1 << 3;
        /// Request that the mapping be locked or pinned on creation.
        const LOCKED          = 1 << 4;
        /// Request reservation without immediate commit where that model exists.
        const RESERVE_ONLY    = 1 << 5;
        /// Request immediate commit on creation where reserve/commit are distinct.
        const COMMIT_NOW      = 1 << 6;
        /// Request wipe-on-free behavior from the platform rather than user-space emulation.
        const WIPE_ON_FREE    = 1 << 7;
    }
}

bitflags::bitflags! {
    /// Region-level attributes that describe the backing or required memory domain.
    ///
    /// These are stronger than tuning hints. They characterize the memory itself or the
    /// domain it must belong to.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct RegionAttrs: u32 {
        /// Memory must be visible to DMA-capable devices.
        const DMA_VISIBLE       = 1 << 0;
        /// Memory resides in a device-local heap rather than ordinary system memory.
        const DEVICE_LOCAL      = 1 << 1;
        /// Memory is cacheable under the active cache policy.
        const CACHEABLE         = 1 << 2;
        /// Memory participates in the relevant coherency domain.
        const COHERENT          = 1 << 3;
        /// Memory is physically contiguous.
        const PHYS_CONTIGUOUS   = 1 << 4;
        /// Memory is executable in addition to any dynamic protection state.
        const EXECUTABLE        = 1 << 5;
        /// Memory carries hardware tags or tag-checked semantics.
        const TAGGED            = 1 << 6;
        /// Memory participates in a platform integrity-management regime.
        const INTEGRITY_MANAGED = 1 << 7;
        /// Memory refers to a predeclared or static region rather than a fresh mapping.
        const STATIC_REGION     = 1 << 8;
        /// Memory exists only as virtual address space, not a specialized physical domain.
        const VIRTUAL_ONLY      = 1 << 9;
    }
}

/// Cache-policy request or description for a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CachePolicy {
    /// Use the platform's normal cache policy for the backing.
    Default,
    /// Disable normal caching for the region.
    Uncached,
    /// Prefer write-combined behavior.
    WriteCombined,
    /// Prefer write-through behavior.
    WriteThrough,
    /// Prefer write-back behavior.
    WriteBack,
}

/// Advisory memory-usage pattern hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Advise {
    /// No specialized access pattern is requested.
    Normal,
    /// Reads are expected to progress mostly forward.
    Sequential,
    /// Access order is expected to be effectively random.
    Random,
    /// Data is expected to be needed soon.
    WillNeed,
    /// Data is not expected to be needed soon.
    DontNeed,
    /// The contents may be discarded if the platform supports that model.
    Free,
    /// Explicitly discourage huge-page treatment for the region.
    NoHugePage,
    /// Explicitly prefer huge-page treatment for the region.
    HugePage,
}

/// Integrity-policy mode for a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntegrityMode {
    /// Use the platform default integrity mode.
    Default,
    /// Request the strongest supported integrity enforcement.
    Strict,
    /// Request a less restrictive integrity mode when the platform supports it.
    Relaxed,
}

/// Tagging mode for tagged-memory platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TagMode {
    /// Disable tagging or use ordinary untagged access semantics.
    None,
    /// Enable checked or faulting tag behavior.
    Checked,
    /// Enable unchecked tagging where the platform exposes that distinction.
    Unchecked,
}

/// Backing object or memory domain for a map request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Backing<'a> {
    /// Fresh anonymous memory with no external backing object.
    Anonymous,
    /// File-backed memory using a raw file descriptor and byte offset.
    File { fd: i32, offset: u64 },
    /// Device-local or device-owned backing identified by a backend-specific id and offset.
    Device { id: u64, offset: u64 },
    /// Physical memory at the given address.
    Physical { addr: usize },
    /// A backend-native pool object identified by a backend-specific id.
    NativePool { id: u64 },
    /// A pre-existing named region borrowed from the surrounding platform environment.
    BorrowedRegion { name: &'a str },
}

/// Safe placement intent for ordinary mappings.
///
/// These variants describe non-destructive placement policy. They intentionally exclude
/// destructive replacement semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Placement {
    /// Let the backend choose the address.
    Anywhere,
    /// Suggest an address without requiring exact placement.
    Hint(usize),
    /// Require an exact address and fail if the range is already occupied.
    FixedNoReplace(usize),
    /// Prefer a NUMA node when the backend supports that model.
    PreferredNode(u32),
    /// Require a NUMA node when the backend supports that model.
    RequiredNode(u32),
    /// Request placement within a backend-specific region identifier.
    RegionId(u64),
}

/// Hazardous placement intent for overwrite mapping.
///
/// This is kept separate from [`Placement`] because it can destroy existing address-space
/// state and therefore belongs behind an unsafe trait boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReplacePlacement {
    /// Map at an exact address even if doing so replaces an existing mapping.
    FixedReplace(usize),
}

/// Fundamental page-size information for the provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageInfo {
    /// Smallest ordinary page size exposed by the backend.
    pub base_page: NonZeroUsize,
    /// Allocation granule that callers should treat as the minimum practical unit.
    pub alloc_granule: NonZeroUsize,
    /// Huge-page size if the backend exposes one through this contract.
    pub huge_page: Option<NonZeroUsize>,
}

/// Low-level region mapping request.
///
/// The placement type is generic so the same shape can express both ordinary mapping
/// requests and explicit replacement mapping requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MapRequest<'a, P = Placement> {
    /// Requested region length in bytes.
    pub len: usize,
    /// Requested alignment in bytes. `0` means backend default alignment.
    pub align: usize,
    /// Requested protection for the new region.
    pub protect: Protect,
    /// Mapping-construction flags.
    pub flags: MapFlags,
    /// Required or descriptive region attributes.
    pub attrs: RegionAttrs,
    /// Requested cache policy.
    pub cache: CachePolicy,
    /// Placement policy for the mapping.
    pub placement: P,
    /// Requested backing object or memory domain.
    pub backing: Backing<'a>,
}

/// Replacement-mapping request using [`ReplacePlacement`].
pub type MapReplaceRequest<'a> = MapRequest<'a, ReplacePlacement>;

/// Request to map physical memory directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhysicalMapRequest {
    /// Physical base address.
    pub addr: usize,
    /// Requested mapping length in bytes.
    pub len: usize,
    /// Requested alignment in bytes. `0` means backend default alignment.
    pub align: usize,
    /// Requested protection for the mapping.
    pub protect: Protect,
    /// Requested cache policy.
    pub cache: CachePolicy,
}

/// Request to map device-local memory or MMIO-style ranges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceMapRequest {
    /// Backend-defined device or heap identifier.
    pub id: u64,
    /// Byte offset into the device-local range.
    pub offset: u64,
    /// Requested mapping length in bytes.
    pub len: usize,
    /// Requested alignment in bytes. `0` means backend default alignment.
    pub align: usize,
    /// Requested protection for the mapping.
    pub protect: Protect,
    /// Requested cache policy.
    pub cache: CachePolicy,
}

/// Owned virtual or physical region returned by the backend.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Region {
    /// Base address of the region.
    pub base: NonNull<u8>,
    /// Actual extent owned by this region handle, in bytes.
    ///
    /// This is the backend-owned extent, which may be page-rounded relative to the original
    /// request size.
    pub len: usize,
}

impl fmt::Debug for Region {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Region")
            .field("base", &self.base)
            .field("len", &self.len)
            .finish()
    }
}

impl Region {
    /// Returns the exclusive end address of the region.
    #[must_use]
    pub fn end_addr(self) -> usize {
        self.base.as_ptr() as usize + self.len
    }

    /// Returns `true` if the address lies within the region.
    #[must_use]
    pub fn contains(self, addr: usize) -> bool {
        addr >= self.base.as_ptr() as usize && addr < self.end_addr()
    }

    /// Returns a checked subrange of this region.
    ///
    /// Fails if the requested range overflows or falls outside the original region.
    pub fn subrange(self, offset: usize, len: usize) -> Result<Region, MemError> {
        let end = offset.checked_add(len).ok_or(MemError::overflow())?;
        if end > self.len {
            return Err(MemError::out_of_bounds());
        }

        let ptr = unsafe { self.base.as_ptr().add(offset) };
        let base = NonNull::new(ptr).ok_or(MemError::invalid())?;
        Ok(Region { base, len })
    }
}

/// Best-effort metadata about a region containing a queried address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegionInfo {
    /// Region that was identified by the query.
    pub region: Region,
    /// Effective protection known to the backend.
    pub protect: Protect,
    /// Effective region attributes known to the backend.
    pub attrs: RegionAttrs,
    /// Effective cache policy known to the backend.
    pub cache: CachePolicy,
    /// Placement information the backend can truthfully report.
    pub placement: Placement,
    /// Whether the region is currently committed where that distinction exists.
    pub committed: bool,
}

/// Low-level memory operation error classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemErrorKind {
    /// The backend does not support the requested operation or semantic.
    Unsupported,
    /// The request shape or inputs are invalid.
    InvalidInput,
    /// An address is invalid for the requested operation.
    InvalidAddress,
    /// An address, offset, or alignment requirement was not properly aligned.
    Misaligned,
    /// The backend could not satisfy the request because memory was exhausted.
    OutOfMemory,
    /// A subrange or requested extent lies outside a valid region.
    OutOfBounds,
    /// The operation was rejected by the platform's permission model.
    PermissionDenied,
    /// The target range is busy or already occupied.
    Busy,
    /// Integer overflow was detected while validating the request.
    Overflow,
    /// Backend-specific platform error code.
    Platform(i32),
}

/// Error returned by low-level PAL memory operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemError {
    /// Classified reason for the failure.
    pub kind: MemErrorKind,
}

impl MemError {
    /// Returns an unsupported-operation error.
    pub const fn unsupported() -> Self {
        Self {
            kind: MemErrorKind::Unsupported,
        }
    }

    /// Returns an invalid-input error.
    pub const fn invalid() -> Self {
        Self {
            kind: MemErrorKind::InvalidInput,
        }
    }

    /// Returns an invalid-address error.
    pub const fn invalid_addr() -> Self {
        Self {
            kind: MemErrorKind::InvalidAddress,
        }
    }

    /// Returns a misaligned-input error.
    pub const fn misaligned() -> Self {
        Self {
            kind: MemErrorKind::Misaligned,
        }
    }

    /// Returns an out-of-memory error.
    pub const fn oom() -> Self {
        Self {
            kind: MemErrorKind::OutOfMemory,
        }
    }

    /// Returns an out-of-bounds error.
    pub const fn out_of_bounds() -> Self {
        Self {
            kind: MemErrorKind::OutOfBounds,
        }
    }

    /// Returns a busy-range error.
    pub const fn busy() -> Self {
        Self {
            kind: MemErrorKind::Busy,
        }
    }

    /// Returns an overflow error.
    pub const fn overflow() -> Self {
        Self {
            kind: MemErrorKind::Overflow,
        }
    }

    /// Returns a platform-specific error code wrapper.
    pub const fn platform(errno: i32) -> Self {
        Self {
            kind: MemErrorKind::Platform(errno),
        }
    }
}

/// Base capability and page-geometry surface shared by all memory providers.
pub trait MemBase {
    /// Returns backend-native memory capabilities.
    fn caps(&self) -> MemCaps;

    /// Returns page-size and allocation-granule information for this provider.
    fn page_info(&self) -> PageInfo;
}

/// Ordinary low-level region mapping operations.
pub trait MemMap: MemBase {
    /// # Safety
    /// Caller is responsible for aliasing, lifetime, and ownership discipline of the
    /// returned region. The backend only acquires the region; it does not prove higher-level
    /// synchronization or exclusivity.
    unsafe fn map(&self, req: &MapRequest<'_>) -> Result<Region, MemError>;

    /// # Safety
    /// Caller must ensure no live references, aliases, or dependent objects remain in the
    /// region being unmapped.
    unsafe fn unmap(&self, region: Region) -> Result<(), MemError>;
}

/// Hazardous overwrite mapping operations.
///
/// This trait is separate because replacement mapping can destroy existing address-space
/// state and is therefore not ordinary placement.
pub unsafe trait MemMapReplace: MemBase {
    /// # Safety
    /// Caller must ensure the replacement range is fully owned and that destroying any
    /// overlapping mapping is valid for the current address space and synchronization model.
    unsafe fn map_replace(&self, _req: &MapReplaceRequest<'_>) -> Result<Region, MemError> {
        Err(MemError::unsupported())
    }
}

/// Protection-changing operations for existing regions.
pub trait MemProtect: MemBase {
    /// # Safety
    /// Caller must ensure protection changes do not invalidate live references, executable
    /// assumptions, or synchronization guarantees relied on elsewhere in the program.
    unsafe fn protect(&self, region: Region, protect: Protect) -> Result<(), MemError>;
}

/// Reserve/commit style operations where the backend exposes them.
pub trait MemCommit: MemBase {
    /// # Safety
    /// Caller must ensure the region is valid for commit and that making the backing
    /// accessible under `protect` does not violate higher-level invariants.
    unsafe fn commit(&self, _region: Region, _protect: Protect) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }

    /// # Safety
    /// Caller must ensure the region is valid for decommit and that discarding backing does
    /// not invalidate live references or required residency guarantees.
    unsafe fn decommit(&self, _region: Region) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

/// Query interface for discovering region metadata from an address.
pub trait MemQuery: MemBase {
    /// Returns information about the region containing `addr`.
    ///
    /// The default implementation reports [`MemErrorKind::Unsupported`].
    fn query(&self, _addr: NonNull<u8>) -> Result<RegionInfo, MemError> {
        Err(MemError::unsupported())
    }
}

/// Advisory hint interface for existing regions.
pub trait MemAdvise: MemBase {
    /// # Safety
    /// Caller must ensure the region is valid for advisory updates.
    ///
    /// The default implementation succeeds and ignores the hint, because advisory behavior
    /// can legitimately be a no-op without violating correctness.
    unsafe fn advise(&self, _region: Region, _advice: Advise) -> Result<(), MemError> {
        Ok(())
    }
}

/// Locking or pinning operations for existing regions.
pub trait MemLock: MemBase {
    /// # Safety
    /// Caller must ensure the region is valid and that pinning it is legal in the current
    /// execution context and process policy.
    unsafe fn lock(&self, _region: Region) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }

    /// # Safety
    /// Caller must ensure the region is valid and was previously locked in a compatible way.
    unsafe fn unlock(&self, _region: Region) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

/// Hazardous control over region cache-policy attributes.
pub unsafe trait MemAttrsControl: MemBase {
    /// # Safety
    /// Caller must ensure cache changes are legal for the region and coherent with any
    /// active users of the mapping.
    unsafe fn set_cache_policy(
        &self,
        _region: Region,
        _policy: CachePolicy,
    ) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

/// Hazardous control over integrity and tag-mode state.
pub unsafe trait MemIntegrityControl: MemBase {
    /// # Safety
    /// Caller must ensure tag-mode transitions respect platform integrity rules.
    unsafe fn set_tag_mode(&self, _region: Region, _mode: TagMode) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }

    /// # Safety
    /// Caller must ensure integrity-mode transitions respect platform integrity rules.
    unsafe fn set_integrity_mode(
        &self,
        _region: Region,
        _mode: IntegrityMode,
    ) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

/// Hazardous direct physical-memory mapping operations.
pub unsafe trait MemPhysical: MemBase {
    /// # Safety
    /// Caller must ensure mapping the requested physical memory is legal and owned.
    unsafe fn map_physical(&self, _req: &PhysicalMapRequest) -> Result<Region, MemError> {
        Err(MemError::unsupported())
    }
}

/// Hazardous device-memory or MMIO mapping operations.
pub unsafe trait MemDevice: MemBase {
    /// # Safety
    /// Caller must ensure the device-local or MMIO mapping is legal for the target device.
    unsafe fn map_device(&self, _req: &DeviceMapRequest) -> Result<Region, MemError> {
        Err(MemError::unsupported())
    }
}
