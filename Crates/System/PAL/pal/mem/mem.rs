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
use core::marker::PhantomData;
use core::num::NonZeroUsize;
use core::ptr::NonNull;

mod catalog;
mod topology;

pub use catalog::*;
pub use topology::*;

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
    /// Detailed backing combinations supported for ordinary mappings.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MemBackingCaps: u32 {
        /// Supports private anonymous mappings.
        const ANON_PRIVATE = 1 << 0;
        /// Supports shared anonymous mappings.
        const ANON_SHARED  = 1 << 1;
        /// Supports privately mapped file-backed memory.
        const FILE_PRIVATE = 1 << 2;
        /// Supports shared file-backed memory.
        const FILE_SHARED  = 1 << 3;
        /// Supports device-local or device-identified backing.
        const DEVICE       = 1 << 4;
        /// Supports direct physical-memory backing.
        const PHYSICAL     = 1 << 5;
        /// Supports backend-native pool-backed mappings.
        const NATIVE_POOL  = 1 << 6;
        /// Supports bindings to borrowed pre-existing regions.
        const BORROWED     = 1 << 7;
    }
}

bitflags::bitflags! {
    /// Detailed ordinary placement modes supported by the backend.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MemPlacementCaps: u32 {
        /// Supports unconstrained placement chosen by the backend.
        const ANYWHERE        = 1 << 0;
        /// Supports non-binding address hints.
        const HINT            = 1 << 1;
        /// Supports exact-address placement that fails rather than replacing.
        const FIXED_NOREPLACE = 1 << 2;
        /// Supports soft NUMA-node preference.
        const PREFERRED_NODE  = 1 << 3;
        /// Supports hard NUMA-node requirement.
        const REQUIRED_NODE   = 1 << 4;
        /// Supports backend-defined region identifiers.
        const REGION_ID       = 1 << 5;
    }
}

bitflags::bitflags! {
    /// Detailed advisory operations supported by the backend.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MemAdviceCaps: u32 {
        /// Supports clearing special access-pattern advice.
        const NORMAL       = 1 << 0;
        /// Supports sequential-access advice.
        const SEQUENTIAL   = 1 << 1;
        /// Supports random-access advice.
        const RANDOM       = 1 << 2;
        /// Supports "will need soon" advice.
        const WILL_NEED    = 1 << 3;
        /// Supports "do not need soon" advice.
        const DONT_NEED    = 1 << 4;
        /// Supports discard-or-free style advice.
        const FREE         = 1 << 5;
        /// Supports disabling huge-page treatment for a region.
        const NO_HUGE_PAGE = 1 << 6;
        /// Supports preferring huge-page treatment for a region.
        const HUGE_PAGE    = 1 << 7;
    }
}

/// Detailed backend support surface for memory operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemSupport {
    /// Coarse backend-native capabilities.
    pub caps: MemCaps,
    /// Mapping-construction flags supported for ordinary mappings.
    pub map_flags: MapFlags,
    /// Ordinary protection bits supported for mappings and protection changes.
    pub protect: Protect,
    /// Detailed supported backing combinations for ordinary mappings.
    pub backings: MemBackingCaps,
    /// Detailed supported ordinary placement modes.
    pub placements: MemPlacementCaps,
    /// Detailed supported advisory operations.
    pub advice: MemAdviceCaps,
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
        ///
        /// On some backends this remains a best-effort acquisition hint rather than a proof
        /// that the full region is resident after mapping.
        const POPULATE        = 1 << 3;
        /// Request a backend-specific "locked on create" mapping hint.
        ///
        /// This is not equivalent to a successful explicit [`MemLock::lock`] call unless the
        /// backend documents that stronger guarantee.
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

/// Lifetime-bound borrowed handle naming a file-like backing object.
///
/// This keeps the PAL memory contract platform-independent while still forcing safe callers to
/// acknowledge that a file-backed mapping request borrows an external handle whose lifetime must
/// outlive the mapping call that consumes it.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct BorrowedBackingHandle<'a> {
    raw: usize,
    _lifetime: PhantomData<&'a ()>,
}

impl fmt::Debug for BorrowedBackingHandle<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BorrowedBackingHandle")
            .field("raw", &self.raw)
            .finish()
    }
}

impl BorrowedBackingHandle<'_> {
    /// Borrows a raw platform handle for a file-backed mapping request.
    ///
    /// # Safety
    /// The caller must ensure the supplied handle remains valid, open, and names the intended
    /// backing object for the entire duration of any request that borrows it. Reusing or closing
    /// the handle before the request is consumed violates the contract.
    #[must_use]
    pub const unsafe fn borrow_raw(raw: usize) -> Self {
        Self {
            raw,
            _lifetime: PhantomData,
        }
    }

    /// Returns the borrowed raw platform handle value.
    #[must_use]
    pub const fn as_raw(self) -> usize {
        self.raw
    }

    /// Borrows a raw Unix file descriptor for a file-backed mapping request.
    ///
    /// # Safety
    /// The caller must ensure the descriptor remains valid, open, and names the intended backing
    /// object for the entire duration of any request that borrows it.
    ///
    /// # Errors
    /// Returns [`MemErrorKind::InvalidInput`] when `fd` is negative.
    #[cfg(unix)]
    pub unsafe fn borrow_raw_fd(fd: i32) -> Result<Self, MemError> {
        let raw = usize::try_from(fd).map_err(|_| MemError::invalid())?;
        Ok(Self {
            raw,
            _lifetime: PhantomData,
        })
    }

    /// Returns the borrowed Unix file descriptor value.
    ///
    /// # Errors
    /// Returns [`MemErrorKind::InvalidInput`] when this borrowed backing handle does not fit in
    /// an `i32` Unix file descriptor.
    #[cfg(unix)]
    pub fn as_raw_fd(self) -> Result<i32, MemError> {
        i32::try_from(self.raw).map_err(|_| MemError::invalid())
    }
}

/// Backing object or memory domain for a map request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Backing<'a> {
    /// Fresh anonymous memory with no external backing object.
    Anonymous,
    /// File-backed memory using a borrowed platform handle and byte offset.
    File {
        /// Borrowed platform handle that names the backing object.
        fd: BorrowedBackingHandle<'a>,
        /// Byte offset into the backing object.
        offset: u64,
    },
    /// Device-local or device-owned backing identified by a backend-specific id and offset.
    Device {
        /// Backend-defined device or heap identifier.
        id: u64,
        /// Byte offset into the device-local backing.
        offset: u64,
    },
    /// Physical memory at the given address.
    Physical {
        /// Physical base address of the requested range.
        addr: usize,
    },
    /// A backend-native pool object identified by a backend-specific id.
    NativePool {
        /// Backend-defined pool identifier.
        id: u64,
    },
    /// A pre-existing named region borrowed from the surrounding platform environment.
    BorrowedRegion {
        /// Backend- or provider-defined borrowed region name.
        name: &'a str,
    },
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

// SAFETY: `Region` is a passive address-range descriptor. Sending or sharing it across threads
// does not, by itself, dereference memory or transfer ownership of the backing. Correct
// lifetime, synchronization, and mapping validity remain the responsibility of the higher-level
// owner that governs the region.
unsafe impl Send for Region {}

// SAFETY: See the `Send` rationale above. A shared `Region` is still just immutable address
// metadata, not a synchronization bypass.
unsafe impl Sync for Region {}

impl Region {
    /// Returns the exclusive end address of the region when it does not overflow the address
    /// space.
    #[must_use]
    pub fn checked_end_addr(self) -> Option<usize> {
        (self.base.as_ptr() as usize).checked_add(self.len)
    }

    /// Returns the exclusive end address of the region when it does not overflow the address
    /// space.
    #[must_use]
    pub fn end_addr(self) -> Option<usize> {
        self.checked_end_addr()
    }

    /// Returns `true` if the address lies within the region.
    #[must_use]
    pub fn contains(self, addr: usize) -> bool {
        let start = self.base.as_ptr() as usize;
        self.checked_end_addr()
            .is_some_and(|end| addr >= start && addr < end)
    }

    /// Returns a checked subrange of this region.
    ///
    /// # Errors
    /// Returns an error when the requested range overflows or falls outside the original
    /// region.
    pub fn subrange(self, offset: usize, len: usize) -> Result<Self, MemError> {
        let end = offset.checked_add(len).ok_or(MemError::overflow())?;
        if end > self.len {
            return Err(MemError::out_of_bounds());
        }

        let ptr = unsafe { self.base.as_ptr().add(offset) };
        let base = NonNull::new(ptr).ok_or(MemError::invalid())?;
        Ok(Self { base, len })
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

impl fmt::Display for MemErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("unsupported operation or semantic"),
            Self::InvalidInput => f.write_str("invalid input"),
            Self::InvalidAddress => f.write_str("invalid address"),
            Self::Misaligned => f.write_str("misaligned input"),
            Self::OutOfMemory => f.write_str("out of memory"),
            Self::OutOfBounds => f.write_str("out of bounds"),
            Self::PermissionDenied => f.write_str("permission denied"),
            Self::Busy => f.write_str("target range is busy"),
            Self::Overflow => f.write_str("integer overflow"),
            Self::Platform(errno) => write!(f, "platform error {errno}"),
        }
    }
}

impl fmt::Display for MemError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "memory operation failed: {}", self.kind)
    }
}

impl MemError {
    /// Returns an unsupported-operation error.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: MemErrorKind::Unsupported,
        }
    }

    /// Returns an invalid-input error.
    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: MemErrorKind::InvalidInput,
        }
    }

    /// Returns an invalid-address error.
    #[must_use]
    pub const fn invalid_addr() -> Self {
        Self {
            kind: MemErrorKind::InvalidAddress,
        }
    }

    /// Returns a misaligned-input error.
    #[must_use]
    pub const fn misaligned() -> Self {
        Self {
            kind: MemErrorKind::Misaligned,
        }
    }

    /// Returns an out-of-memory error.
    #[must_use]
    pub const fn oom() -> Self {
        Self {
            kind: MemErrorKind::OutOfMemory,
        }
    }

    /// Returns an out-of-bounds error.
    #[must_use]
    pub const fn out_of_bounds() -> Self {
        Self {
            kind: MemErrorKind::OutOfBounds,
        }
    }

    /// Returns a busy-range error.
    #[must_use]
    pub const fn busy() -> Self {
        Self {
            kind: MemErrorKind::Busy,
        }
    }

    /// Returns an overflow error.
    #[must_use]
    pub const fn overflow() -> Self {
        Self {
            kind: MemErrorKind::Overflow,
        }
    }

    /// Returns a platform-specific error code wrapper.
    #[must_use]
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

    /// Returns detailed backend support information.
    fn support(&self) -> MemSupport;

    /// Returns page-size and allocation-granule information for this provider.
    fn page_info(&self) -> PageInfo;
}

/// Ordinary low-level region mapping operations.
pub trait MemMap: MemBase {
    /// # Safety
    /// Caller is responsible for aliasing, lifetime, and ownership discipline of the
    /// returned region. The backend only acquires the region; it does not prove higher-level
    /// synchronization or exclusivity.
    ///
    /// # Errors
    /// Returns an error when the request is invalid, unsupported, or the backend cannot
    /// acquire the requested mapping.
    unsafe fn map(&self, req: &MapRequest<'_>) -> Result<Region, MemError>;

    /// # Safety
    /// Caller must ensure no live references, aliases, or dependent objects remain in the
    /// region being unmapped.
    ///
    /// # Errors
    /// Returns an error when the region is invalid or the backend rejects the unmap.
    unsafe fn unmap(&self, region: Region) -> Result<(), MemError>;
}

/// Hazardous overwrite mapping operations.
///
/// This trait is separate because replacement mapping can destroy existing address-space
/// state and is therefore not ordinary placement.
///
/// # Safety
/// Implementors must only expose replacement mapping when overwriting existing address-space
/// contents is a real backend capability and callers can be held to the corresponding unsafe
/// preconditions.
pub unsafe trait MemMapReplace: MemBase {
    /// # Safety
    /// Caller must ensure the replacement range is fully owned and that destroying any
    /// overlapping mapping is valid for the current address space and synchronization model.
    ///
    /// # Errors
    /// Returns an error when replacement mapping is unsupported, invalid, or rejected by the
    /// backend.
    unsafe fn map_replace(&self, _req: &MapReplaceRequest<'_>) -> Result<Region, MemError> {
        Err(MemError::unsupported())
    }
}

/// Protection-changing operations for existing regions.
pub trait MemProtect: MemBase {
    /// # Safety
    /// Caller must ensure protection changes do not invalidate live references, executable
    /// assumptions, or synchronization guarantees relied on elsewhere in the program.
    ///
    /// # Errors
    /// Returns an error when protection changes are unsupported, invalid, or rejected by the
    /// backend.
    unsafe fn protect(&self, region: Region, protect: Protect) -> Result<(), MemError>;
}

/// Reserve/commit style operations where the backend exposes them.
pub trait MemCommit: MemBase {
    /// # Safety
    /// Caller must ensure the region is valid for commit and that making the backing
    /// accessible under `protect` does not violate higher-level invariants.
    ///
    /// # Errors
    /// Returns an error when commit control is unsupported, invalid, or rejected by the
    /// backend.
    unsafe fn commit(&self, _region: Region, _protect: Protect) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }

    /// # Safety
    /// Caller must ensure the region is valid for decommit and that discarding backing does
    /// not invalidate live references or required residency guarantees.
    ///
    /// # Errors
    /// Returns an error when decommit control is unsupported, invalid, or rejected by the
    /// backend.
    unsafe fn decommit(&self, _region: Region) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

/// Query interface for discovering region metadata from an address.
pub trait MemQuery: MemBase {
    /// Returns information about the region containing `addr`.
    ///
    /// The default implementation reports [`MemErrorKind::Unsupported`].
    ///
    /// # Errors
    /// Returns an error when query is unsupported or the backend cannot describe `addr`.
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
    ///
    /// # Errors
    /// Returns an error when the advisory update is invalid or explicitly rejected by the
    /// backend.
    unsafe fn advise(&self, _region: Region, _advice: Advise) -> Result<(), MemError> {
        Ok(())
    }
}

/// Locking or pinning operations for existing regions.
pub trait MemLock: MemBase {
    /// # Safety
    /// Caller must ensure the region is valid and that pinning it is legal in the current
    /// execution context and process policy.
    ///
    /// # Errors
    /// Returns an error when locking is unsupported, invalid, or rejected by the backend.
    unsafe fn lock(&self, _region: Region) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }

    /// # Safety
    /// Caller must ensure the region is valid and was previously locked in a compatible way.
    ///
    /// # Errors
    /// Returns an error when unlocking is unsupported, invalid, or rejected by the backend.
    unsafe fn unlock(&self, _region: Region) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

/// Hazardous control over region cache-policy attributes.
///
/// # Safety
/// Implementors must only expose cache-policy mutation when the backend can actually change
/// cache behavior and callers can be held to the required coherency preconditions.
pub unsafe trait MemAttrsControl: MemBase {
    /// # Safety
    /// Caller must ensure cache changes are legal for the region and coherent with any
    /// active users of the mapping.
    ///
    /// # Errors
    /// Returns an error when cache-policy control is unsupported, invalid, or rejected by the
    /// backend.
    unsafe fn set_cache_policy(
        &self,
        _region: Region,
        _policy: CachePolicy,
    ) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

/// Hazardous control over integrity and tag-mode state.
///
/// # Safety
/// Implementors must only expose integrity control when the backend can enforce the requested
/// transitions and callers can satisfy the corresponding platform rules.
pub unsafe trait MemIntegrityControl: MemBase {
    /// # Safety
    /// Caller must ensure tag-mode transitions respect platform integrity rules.
    ///
    /// # Errors
    /// Returns an error when tag-mode control is unsupported, invalid, or rejected by the
    /// backend.
    unsafe fn set_tag_mode(&self, _region: Region, _mode: TagMode) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }

    /// # Safety
    /// Caller must ensure integrity-mode transitions respect platform integrity rules.
    ///
    /// # Errors
    /// Returns an error when integrity-mode control is unsupported, invalid, or rejected by
    /// the backend.
    unsafe fn set_integrity_mode(
        &self,
        _region: Region,
        _mode: IntegrityMode,
    ) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

/// Hazardous direct physical-memory mapping operations.
///
/// # Safety
/// Implementors must only expose physical mapping when the backend can safely bind physical
/// memory into the process and callers can uphold ownership and side-effect requirements.
pub unsafe trait MemPhysical: MemBase {
    /// # Safety
    /// Caller must ensure mapping the requested physical memory is legal and owned.
    ///
    /// # Errors
    /// Returns an error when physical mapping is unsupported, invalid, or rejected by the
    /// backend.
    unsafe fn map_physical(&self, _req: &PhysicalMapRequest) -> Result<Region, MemError> {
        Err(MemError::unsupported())
    }
}

/// Hazardous device-memory or MMIO mapping operations.
///
/// # Safety
/// Implementors must only expose device mapping when the backend can legally create the
/// mapping and callers can uphold device-specific ownership and synchronization rules.
pub unsafe trait MemDevice: MemBase {
    /// # Safety
    /// Caller must ensure the device-local or MMIO mapping is legal for the target device.
    ///
    /// # Errors
    /// Returns an error when device mapping is unsupported, invalid, or rejected by the
    /// backend.
    unsafe fn map_device(&self, _req: &DeviceMapRequest) -> Result<Region, MemError> {
        Err(MemError::unsupported())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protect_none_remains_a_vacuous_subset() {
        assert!(Protect::READ.contains(Protect::NONE));
        assert!(Protect::READ | Protect::WRITE != Protect::NONE);
    }

    #[test]
    fn region_checked_end_addr_fails_closed_on_overflow() {
        let region = Region {
            base: NonNull::new((usize::MAX - 1) as *mut u8).expect("non-null pointer"),
            len: 8,
        };

        assert_eq!(region.checked_end_addr(), None);
        assert!(!region.contains((usize::MAX - 1) as *const u8 as usize));
    }
}
