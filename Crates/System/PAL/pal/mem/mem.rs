use core::fmt;
use core::num::NonZeroUsize;
use core::ptr::NonNull;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MemCaps: u64 {
        const MAP_ANON              = 1 << 0;
        const MAP_FILE              = 1 << 1;
        const MAP_FIXED_NOREPLACE   = 1 << 2;
        const MAP_FIXED_REPLACE     = 1 << 3;
        const MAP_HINT              = 1 << 4;
        const PROTECT               = 1 << 5;
        const ADVISE                = 1 << 6;
        const LOCK                  = 1 << 7;
        const QUERY                 = 1 << 8;
        const RESERVE_ONLY          = 1 << 9;
        const COMMIT_CONTROL        = 1 << 10;
        const DECOMMIT_CONTROL      = 1 << 11;
        const HUGE_PAGES            = 1 << 12;
        const NUMA_HINTS            = 1 << 13;
        const PHYSICAL_MAP          = 1 << 14;
        const DEVICE_MAP            = 1 << 15;
        const TAGGED_MEMORY         = 1 << 16;
        const INTEGRITY_CONTROL     = 1 << 17;
        const CACHE_POLICY          = 1 << 18;
        const EXECUTE_MAP           = 1 << 19;
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Protect: u32 {
        const NONE    = 0;
        const READ    = 1 << 0;
        const WRITE   = 1 << 1;
        const EXEC    = 1 << 2;
        const GUARD   = 1 << 3;
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MapFlags: u32 {
        const SHARED          = 1 << 0;
        const PRIVATE         = 1 << 1;
        const STACK           = 1 << 2;
        const GROWSDOWN       = 1 << 3;
        const HUGE_PAGE       = 1 << 4;
        const POPULATE        = 1 << 5;
        const LOCKED          = 1 << 6;
        const RESERVE_ONLY    = 1 << 7;
        const COMMIT_NOW      = 1 << 8;
        const WIPE_ON_FREE    = 1 << 9;
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct RegionAttrs: u32 {
        const DMA_VISIBLE       = 1 << 0;
        const DEVICE_LOCAL      = 1 << 1;
        const CACHEABLE         = 1 << 2;
        const COHERENT          = 1 << 3;
        const PHYS_CONTIGUOUS   = 1 << 4;
        const EXECUTABLE        = 1 << 5;
        const TAGGED            = 1 << 6;
        const INTEGRITY_MANAGED = 1 << 7;
        const STATIC_REGION     = 1 << 8;
        const VIRTUAL_ONLY      = 1 << 9;
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PoolCapabilitySet: u64 {
        const PRIVATE_BACKING   = 1 << 0;
        const SHARED_BACKING    = 1 << 1;
        const EXECUTABLE        = 1 << 2;
        const LOCKABLE          = 1 << 3;
        const POPULATE          = 1 << 4;
        const FIXED_NOREPLACE   = 1 << 5;
        const ADVISE            = 1 << 6;
        const QUERY             = 1 << 7;
        const ZERO_ON_FREE      = 1 << 8;
        const PHYSICAL          = 1 << 9;
        const DEVICE_LOCAL      = 1 << 10;
        const INTEGRITY         = 1 << 11;
        const CACHE_POLICY      = 1 << 12;
        const GROWABLE          = 1 << 13;
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PoolPreferenceSet: u32 {
        const PLACEMENT    = 1 << 0;
        const POPULATE     = 1 << 1;
        const LOCK         = 1 << 2;
        const HUGE_PAGES   = 1 << 3;
        const ZERO_ON_FREE = 1 << 4;
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PoolHazardSet: u32 {
        const EXECUTABLE = 1 << 0;
        const SHARED     = 1 << 1;
        const EMULATED   = 1 << 2;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CachePolicy {
    Default,
    Uncached,
    WriteCombined,
    WriteThrough,
    WriteBack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Advise {
    Normal,
    Sequential,
    Random,
    WillNeed,
    DontNeed,
    Free,
    NoHugePage,
    HugePage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntegrityMode {
    Default,
    Strict,
    Relaxed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TagMode {
    None,
    Checked,
    Unchecked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Backing<'a> {
    Anonymous,
    File { fd: i32, offset: u64 },
    Device { id: u64, offset: u64 },
    Physical { addr: usize },
    NativePool { id: u64 },
    BorrowedRegion { name: &'a str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Placement {
    Anywhere,
    Hint(usize),
    FixedNoReplace(usize),
    PreferredNode(u32),
    RequiredNode(u32),
    RegionId(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReplacePlacement {
    FixedReplace(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageInfo {
    pub base_page: NonZeroUsize,
    pub alloc_granule: NonZeroUsize,
    pub huge_page: Option<NonZeroUsize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MapRequest<'a, P = Placement> {
    pub len: usize,
    pub align: usize,
    pub protect: Protect,
    pub flags: MapFlags,
    pub attrs: RegionAttrs,
    pub cache: CachePolicy,
    pub placement: P,
    pub backing: Backing<'a>,
}

pub type MapReplaceRequest<'a> = MapRequest<'a, ReplacePlacement>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhysicalMapRequest {
    pub addr: usize,
    pub len: usize,
    pub align: usize,
    pub protect: Protect,
    pub cache: CachePolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceMapRequest {
    pub id: u64,
    pub offset: u64,
    pub len: usize,
    pub align: usize,
    pub protect: Protect,
    pub cache: CachePolicy,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Region {
    pub base: NonNull<u8>,
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
    #[must_use]
    pub fn end_addr(self) -> usize {
        self.base.as_ptr() as usize + self.len
    }

    #[must_use]
    pub fn contains(self, addr: usize) -> bool {
        addr >= self.base.as_ptr() as usize && addr < self.end_addr()
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegionInfo {
    pub region: Region,
    pub protect: Protect,
    pub attrs: RegionAttrs,
    pub cache: CachePolicy,
    pub placement: Placement,
    pub committed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoolBackingKind {
    AnonymousPrivate,
    AnonymousShared,
    StaticRegion,
    Partition,
    DeviceLocal,
    Physical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PoolBounds {
    pub initial_capacity: usize,
    pub max_capacity: Option<usize>,
    pub growable: bool,
}

impl PoolBounds {
    #[must_use]
    pub const fn fixed(capacity: usize) -> Self {
        Self {
            initial_capacity: capacity,
            max_capacity: Some(capacity),
            growable: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoolAccess {
    ReadWrite,
    ReadWriteExecute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoolSharing {
    Private,
    Shared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoolLatency {
    BestEffort,
    Prefault,
    Locked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IntegrityConstraints {
    pub mode: IntegrityMode,
    pub tag: Option<TagMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoolRequirement {
    Placement(Placement),
    Query,
    Locked,
    NoOvercommit,
    CachePolicy(CachePolicy),
    Integrity(IntegrityConstraints),
    DmaVisible,
    PhysicalContiguous,
    DeviceLocal,
    Shared,
    ZeroOnFree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoolPreference {
    Placement(Placement),
    Populate,
    Lock,
    HugePages,
    ZeroOnFree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoolProhibition {
    Executable,
    ReplaceMapping,
    Overcommit,
    Shared,
    DeviceLocal,
    Physical,
    Emulation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PoolRequest<'a> {
    pub name: Option<&'a str>,
    pub bounds: PoolBounds,
    pub access: PoolAccess,
    pub sharing: PoolSharing,
    pub latency: PoolLatency,
    pub requirements: &'a [PoolRequirement],
    pub preferences: &'a [PoolPreference],
    pub prohibitions: &'a [PoolProhibition],
}

impl<'a> PoolRequest<'a> {
    #[must_use]
    pub const fn new(
        bounds: PoolBounds,
        access: PoolAccess,
        sharing: PoolSharing,
        latency: PoolLatency,
        requirements: &'a [PoolRequirement],
        preferences: &'a [PoolPreference],
        prohibitions: &'a [PoolProhibition],
    ) -> Self {
        Self {
            name: None,
            bounds,
            access,
            sharing,
            latency,
            requirements,
            preferences,
            prohibitions,
        }
    }

    #[must_use]
    pub const fn anonymous_private(capacity: usize) -> Self {
        Self {
            name: None,
            bounds: PoolBounds::fixed(capacity),
            access: PoolAccess::ReadWrite,
            sharing: PoolSharing::Private,
            latency: PoolLatency::BestEffort,
            requirements: &[],
            preferences: &[],
            prohibitions: &[],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolvedPoolConfig {
    pub backing: PoolBackingKind,
    pub bounds: PoolBounds,
    pub granted_capabilities: PoolCapabilitySet,
    pub unmet_preferences: PoolPreferenceSet,
    pub emulated_capabilities: PoolCapabilitySet,
    pub residual_hazards: PoolHazardSet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoolErrorKind {
    InvalidRequest,
    UnsupportedRequirement,
    ProhibitionViolated,
    OutOfMemory,
    InvalidRange,
    Platform(MemErrorKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PoolError {
    pub kind: PoolErrorKind,
}

impl PoolError {
    #[must_use]
    pub const fn invalid_request() -> Self {
        Self {
            kind: PoolErrorKind::InvalidRequest,
        }
    }

    #[must_use]
    pub const fn unsupported_requirement() -> Self {
        Self {
            kind: PoolErrorKind::UnsupportedRequirement,
        }
    }

    #[must_use]
    pub const fn prohibition_violated() -> Self {
        Self {
            kind: PoolErrorKind::ProhibitionViolated,
        }
    }

    #[must_use]
    pub const fn out_of_memory() -> Self {
        Self {
            kind: PoolErrorKind::OutOfMemory,
        }
    }

    #[must_use]
    pub const fn invalid_range() -> Self {
        Self {
            kind: PoolErrorKind::InvalidRange,
        }
    }

    #[must_use]
    pub const fn platform(kind: MemErrorKind) -> Self {
        Self {
            kind: PoolErrorKind::Platform(kind),
        }
    }
}

impl From<MemError> for PoolError {
    fn from(value: MemError) -> Self {
        match value.kind {
            MemErrorKind::OutOfMemory => Self::out_of_memory(),
            MemErrorKind::Unsupported => Self::unsupported_requirement(),
            other => Self::platform(other),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemErrorKind {
    Unsupported,
    InvalidInput,
    InvalidAddress,
    Misaligned,
    OutOfMemory,
    OutOfBounds,
    PermissionDenied,
    Busy,
    Overflow,
    Platform(i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemError {
    pub kind: MemErrorKind,
}

impl MemError {
    pub const fn unsupported() -> Self {
        Self {
            kind: MemErrorKind::Unsupported,
        }
    }

    pub const fn invalid() -> Self {
        Self {
            kind: MemErrorKind::InvalidInput,
        }
    }

    pub const fn invalid_addr() -> Self {
        Self {
            kind: MemErrorKind::InvalidAddress,
        }
    }

    pub const fn misaligned() -> Self {
        Self {
            kind: MemErrorKind::Misaligned,
        }
    }

    pub const fn oom() -> Self {
        Self {
            kind: MemErrorKind::OutOfMemory,
        }
    }

    pub const fn out_of_bounds() -> Self {
        Self {
            kind: MemErrorKind::OutOfBounds,
        }
    }

    pub const fn busy() -> Self {
        Self {
            kind: MemErrorKind::Busy,
        }
    }

    pub const fn overflow() -> Self {
        Self {
            kind: MemErrorKind::Overflow,
        }
    }

    pub const fn platform(errno: i32) -> Self {
        Self {
            kind: MemErrorKind::Platform(errno),
        }
    }
}

pub trait MemBase {
    fn caps(&self) -> MemCaps;
    fn page_info(&self) -> PageInfo;
}

pub trait MemMap: MemBase {
    /// # Safety
    /// Caller is responsible for aliasing and lifetime discipline of the mapped region.
    unsafe fn map(&self, req: &MapRequest<'_>) -> Result<Region, MemError>;

    /// # Safety
    /// Caller must ensure no live references remain into the region.
    unsafe fn unmap(&self, region: Region) -> Result<(), MemError>;
}

pub unsafe trait MemMapReplace: MemBase {
    /// # Safety
    /// Caller must ensure the replacement range is fully owned and that destroying any
    /// overlapping mapping is valid for the current address space and synchronization model.
    unsafe fn map_replace(&self, _req: &MapReplaceRequest<'_>) -> Result<Region, MemError> {
        Err(MemError::unsupported())
    }
}

pub trait MemProtect: MemBase {
    /// # Safety
    /// Caller must ensure protection changes do not invalidate live assumptions.
    unsafe fn protect(&self, region: Region, protect: Protect) -> Result<(), MemError>;
}

pub trait MemCommit: MemBase {
    /// # Safety
    /// Caller must ensure region is valid and owned for this operation.
    unsafe fn commit(&self, _region: Region, _protect: Protect) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }

    /// # Safety
    /// Caller must ensure region is valid and owned for this operation.
    unsafe fn decommit(&self, _region: Region) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

pub trait MemQuery: MemBase {
    fn query(&self, _addr: NonNull<u8>) -> Result<RegionInfo, MemError> {
        Err(MemError::unsupported())
    }
}

pub trait MemAdvise: MemBase {
    /// # Safety
    /// Caller must ensure region is valid.
    unsafe fn advise(&self, _region: Region, _advice: Advise) -> Result<(), MemError> {
        Ok(())
    }
}

pub trait MemLock: MemBase {
    /// # Safety
    /// Caller must ensure region is valid and pinning is legal in current context.
    unsafe fn lock(&self, _region: Region) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }

    /// # Safety
    /// Caller must ensure region is valid and locked.
    unsafe fn unlock(&self, _region: Region) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

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

pub unsafe trait MemPhysical: MemBase {
    /// # Safety
    /// Caller must ensure mapping the requested physical memory is legal and owned.
    unsafe fn map_physical(&self, _req: &PhysicalMapRequest) -> Result<Region, MemError> {
        Err(MemError::unsupported())
    }
}

pub unsafe trait MemDevice: MemBase {
    /// # Safety
    /// Caller must ensure the device-local or MMIO mapping is legal for the target device.
    unsafe fn map_device(&self, _req: &DeviceMapRequest) -> Result<Region, MemError> {
        Err(MemError::unsupported())
    }
}

pub trait PoolHandle {
    fn region(&self) -> Region;
    fn page_size(&self) -> usize;

    #[must_use]
    fn contains(&self, ptr: *const u8) -> bool {
        self.region().contains(ptr as usize)
    }
}

pub trait MemPool: MemBase {
    type PoolHandle: PoolHandle;

    fn create_pool(
        &self,
        request: &PoolRequest<'_>,
    ) -> Result<(Self::PoolHandle, ResolvedPoolConfig), PoolError>;

    /// # Safety
    /// Caller must ensure the handle is no longer in use and any allocator state using
    /// the pool has already been torn down.
    unsafe fn destroy_pool(&self, pool: Self::PoolHandle) -> Result<(), PoolError>;
}
