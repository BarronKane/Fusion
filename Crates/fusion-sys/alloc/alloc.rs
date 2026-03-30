//! Public allocation and allocator-facing memory surface for `fusion-sys`.
//!
//! `fusion-sys::mem::resource` and `fusion-sys::mem::provider` remain public because they model
//! governed memory truth: concrete ranges, inventories, topology, and provisioning plans.
//! `fusion-sys::alloc` sits above that substrate and is the sanctioned public path for
//! allocator-facing policy, bounded slabs and arenas, general heap negotiation, and the
//! pool-backed extent machinery that allocators consume.
//!
//! The lower `mem::pool` module is intentionally internal. Its types are re-exported here
//! while allocator surfaces are still being built so callers migrate toward the right public
//! namespace instead of wiring themselves directly into lower plumbing.

use core::mem::{ManuallyDrop, align_of, size_of};
use core::ptr::NonNull;
use fusion_pal::sys::mem::Region;

use crate::sync::{SharedHeader, SharedRelease};

mod arena;
mod control;
mod domain;
mod error;
mod heap;
mod lifetime;
mod metadata;
mod policy;
mod protocol;
mod root;
mod service;
mod slab;

pub use arena::{ArenaAllocation, ArenaInitError, ArenaSlice, BoundedArena};
pub use control::ControlLease;
pub use domain::{
    AllocatorDomainAudit,
    AllocatorDomainId,
    AllocatorDomainInfo,
    AllocatorDomainKind,
};
#[allow(unused_imports)]
pub use error::{AllocError, AllocErrorKind};
pub use heap::HeapAllocator;
pub use lifetime::{Immortal, LifetimePolicy, Mortal};
pub(crate) use metadata::{
    AllocSubsystemKind,
    MetadataPageHeader,
    front_metadata_layout_with_policy,
};
pub use policy::{AllocCapabilities, AllocHazards, AllocModeSet, AllocPolicy};
pub use protocol::{
    AllocatorControlRequest,
    AllocatorControlStatusMessage,
    AllocatorControlStatusProtocol,
    AllocatorControlWriteProtocol,
    AllocatorDomainMetadataMessage,
    AllocatorDomainMetadataProtocol,
};
#[allow(unused_imports)]
pub use root::{Allocator, AllocatorBuilder};
pub use service::{
    AllocatorChannelService,
    AllocatorChannelServiceError,
    AllocatorChannelServiceErrorKind,
};
pub use slab::{Slab, SlabAllocation};

#[allow(unused_imports)]
pub use crate::mem::pool::{
    MemoryPool,
    MemoryPoolBuilder,
    MemoryPoolContributor,
    MemoryPoolContributorOrigin,
    MemoryPoolError,
    MemoryPoolErrorKind,
    MemoryPoolExtentDisposition,
    MemoryPoolExtentInfo,
    MemoryPoolExtentRequest,
    MemoryPoolLease,
    MemoryPoolLeaseId,
    MemoryPoolLeaseView,
    MemoryPoolMemberId,
    MemoryPoolMemberInfo,
    MemoryPoolMetadataLayout,
    MemoryPoolPolicy,
    MemoryPoolProvisioningPolicy,
    MemoryPoolStats,
};
pub use crate::mem::provider::CriticalSafetyRequirements;
#[allow(unused_imports)]
pub use crate::mem::resource::{
    MemoryDomain,
    MemoryDomainSet,
    MemoryGeometry,
    ResourceAttrs,
    ResourceHazardSet,
};

/// Request for one allocator-managed memory block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AllocRequest {
    /// Required allocation length in bytes.
    pub len: usize,
    /// Required alignment in bytes.
    pub align: usize,
    /// Whether the returned allocation must be zero-initialized.
    pub zeroed: bool,
}

impl AllocRequest {
    /// Creates a new allocation request with byte alignment `1`.
    #[must_use]
    pub const fn new(len: usize) -> Self {
        Self {
            len,
            align: 1,
            zeroed: false,
        }
    }

    /// Creates a new zero-initialized allocation request with byte alignment `1`.
    #[must_use]
    pub const fn zeroed(len: usize) -> Self {
        Self {
            len,
            align: 1,
            zeroed: true,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub(crate) enum AllocationBacking {
    SlabSlot {
        pool_marker: usize,
        lease_id: MemoryPoolLeaseId,
        slot: usize,
    },
    ArenaBlock {
        pool_marker: usize,
        lease_id: MemoryPoolLeaseId,
        offset: usize,
        len: usize,
    },
}

/// Successful allocator result together with the resource truth attached to it.
///
/// This is intentionally a linear token, not a copyable descriptor. Releasing an allocation
/// consumes the token so higher layers do not casually duplicate ownership.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct AllocResult {
    /// Base address of the allocation.
    pub ptr: NonNull<u8>,
    /// Allocation length in bytes.
    pub len: usize,
    /// Alignment satisfied by the allocation.
    pub align: usize,
    /// Domain that produced the allocation.
    pub domain: MemoryDomain,
    /// Intrinsic attributes of the backing resource.
    pub attrs: ResourceAttrs,
    /// Hazards the caller must still account for.
    pub hazards: ResourceHazardSet,
    /// Operation granularity exposed by the backing resource.
    pub geometry: MemoryGeometry,
    pub(crate) backing: AllocationBacking,
}

impl AllocResult {
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn from_parts(
        ptr: NonNull<u8>,
        len: usize,
        align: usize,
        domain: MemoryDomain,
        attrs: ResourceAttrs,
        hazards: ResourceHazardSet,
        geometry: MemoryGeometry,
        backing: AllocationBacking,
    ) -> Self {
        Self {
            ptr,
            len,
            align,
            domain,
            attrs,
            hazards,
            geometry,
            backing,
        }
    }
}

#[derive(Clone, Copy)]
struct PoolHandleVTable {
    acquire_extent:
        unsafe fn(NonNull<()>, &MemoryPoolExtentRequest) -> Result<MemoryPoolLease, AllocError>,
    release_extent: unsafe fn(NonNull<()>, MemoryPoolLease) -> Result<(), AllocError>,
    lease_region: unsafe fn(NonNull<()>, &MemoryPoolLease) -> Result<Region, AllocError>,
    stats: unsafe fn(NonNull<()>) -> Result<MemoryPoolStats, AllocError>,
    member_info:
        unsafe fn(NonNull<()>, MemoryPoolMemberId) -> Result<MemoryPoolMemberInfo, AllocError>,
    member_info_at:
        unsafe fn(NonNull<()>, usize) -> Result<Option<MemoryPoolMemberInfo>, AllocError>,
    extent_info_at:
        unsafe fn(NonNull<()>, usize) -> Result<Option<MemoryPoolExtentInfo>, AllocError>,
    retain: unsafe fn(NonNull<()>) -> Result<(), AllocError>,
    release: unsafe fn(NonNull<()>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PoolControlStorage {
    InBandResource,
}

struct PoolControlBlock<const MEMBERS: usize, const EXTENTS: usize> {
    header: SharedHeader,
    storage: PoolControlStorage,
    pool: ManuallyDrop<MemoryPool<MEMBERS, EXTENTS>>,
}

pub(crate) struct PoolHandle {
    ptr: NonNull<()>,
    vtable: PoolHandleVTable,
}

impl core::fmt::Debug for PoolHandle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PoolHandle")
            .field("marker", &self.marker())
            .finish_non_exhaustive()
    }
}

unsafe impl Send for PoolHandle {}
unsafe impl Sync for PoolHandle {}

impl PoolHandle {
    pub(crate) fn new_in_region<const MEMBERS: usize, const EXTENTS: usize>(
        pool: MemoryPool<MEMBERS, EXTENTS>,
        region: Region,
    ) -> Result<Self, AllocError> {
        Self::new_in_storage(pool, region, PoolControlStorage::InBandResource)
    }

    fn new_in_storage<const MEMBERS: usize, const EXTENTS: usize>(
        pool: MemoryPool<MEMBERS, EXTENTS>,
        region: Region,
        storage: PoolControlStorage,
    ) -> Result<Self, AllocError> {
        if region.len < size_of::<PoolControlBlock<MEMBERS, EXTENTS>>()
            || !region
                .base
                .get()
                .is_multiple_of(core::mem::align_of::<PoolControlBlock<MEMBERS, EXTENTS>>())
        {
            return Err(AllocError::invalid_request());
        }

        let ptr = NonNull::new(region.base.cast::<PoolControlBlock<MEMBERS, EXTENTS>>())
            .ok_or_else(AllocError::invalid_request)?;
        // SAFETY: the mapped control region is uniquely owned here, properly aligned, and large
        // enough to host exactly one pool control block.
        unsafe {
            ptr.as_ptr().write(PoolControlBlock {
                header: SharedHeader::new(),
                storage,
                pool: ManuallyDrop::new(pool),
            });
        }

        Ok(Self {
            ptr: ptr.cast::<()>(),
            vtable: PoolHandleVTable {
                acquire_extent: acquire_extent_impl::<MEMBERS, EXTENTS>,
                release_extent: release_extent_impl::<MEMBERS, EXTENTS>,
                lease_region: lease_region_impl::<MEMBERS, EXTENTS>,
                stats: stats_impl::<MEMBERS, EXTENTS>,
                member_info: member_info_impl::<MEMBERS, EXTENTS>,
                member_info_at: member_info_at_impl::<MEMBERS, EXTENTS>,
                extent_info_at: extent_info_at_impl::<MEMBERS, EXTENTS>,
                retain: retain_impl::<MEMBERS, EXTENTS>,
                release: release_impl::<MEMBERS, EXTENTS>,
            },
        })
    }

    pub(crate) fn marker(&self) -> usize {
        self.ptr.as_ptr() as usize
    }

    pub(crate) fn try_clone(&self) -> Result<Self, AllocError> {
        unsafe { (self.vtable.retain)(self.ptr)? };
        Ok(Self {
            ptr: self.ptr,
            vtable: self.vtable,
        })
    }

    pub(crate) fn acquire_extent(
        &self,
        request: &MemoryPoolExtentRequest,
    ) -> Result<MemoryPoolLease, AllocError> {
        unsafe { (self.vtable.acquire_extent)(self.ptr, request) }
    }

    pub(crate) fn release_extent(&self, lease: MemoryPoolLease) -> Result<(), AllocError> {
        unsafe { (self.vtable.release_extent)(self.ptr, lease) }
    }

    pub(crate) fn lease_region(&self, lease: &MemoryPoolLease) -> Result<Region, AllocError> {
        unsafe { (self.vtable.lease_region)(self.ptr, lease) }
    }

    pub(crate) fn stats(&self) -> Result<MemoryPoolStats, AllocError> {
        unsafe { (self.vtable.stats)(self.ptr) }
    }

    pub(crate) fn member_info(
        &self,
        member: MemoryPoolMemberId,
    ) -> Result<MemoryPoolMemberInfo, AllocError> {
        unsafe { (self.vtable.member_info)(self.ptr, member) }
    }

    pub(crate) fn member_info_at(
        &self,
        index: usize,
    ) -> Result<Option<MemoryPoolMemberInfo>, AllocError> {
        unsafe { (self.vtable.member_info_at)(self.ptr, index) }
    }

    pub(crate) fn extent_info_at(
        &self,
        index: usize,
    ) -> Result<Option<MemoryPoolExtentInfo>, AllocError> {
        unsafe { (self.vtable.extent_info_at)(self.ptr, index) }
    }
}

impl Drop for PoolHandle {
    fn drop(&mut self) {
        unsafe { (self.vtable.release)(self.ptr) };
    }
}

fn pool_block<const MEMBERS: usize, const EXTENTS: usize>(
    ptr: NonNull<()>,
) -> &'static PoolControlBlock<MEMBERS, EXTENTS> {
    // SAFETY: the pointer is created from a live pool control mapping and the vtable ensures each
    // method uses the matching concrete instantiation.
    unsafe { &*ptr.cast::<PoolControlBlock<MEMBERS, EXTENTS>>().as_ptr() }
}

fn pool_ref<const MEMBERS: usize, const EXTENTS: usize>(
    ptr: NonNull<()>,
) -> &'static MemoryPool<MEMBERS, EXTENTS> {
    let block = pool_block::<MEMBERS, EXTENTS>(ptr);
    // SAFETY: the pool lives inside the control block until the final handle release.
    unsafe { &*((&raw const block.pool).cast::<MemoryPool<MEMBERS, EXTENTS>>()) }
}

unsafe fn acquire_extent_impl<const MEMBERS: usize, const EXTENTS: usize>(
    ptr: NonNull<()>,
    request: &MemoryPoolExtentRequest,
) -> Result<MemoryPoolLease, AllocError> {
    pool_ref::<MEMBERS, EXTENTS>(ptr)
        .acquire_extent(request)
        .map_err(Into::into)
}

unsafe fn release_extent_impl<const MEMBERS: usize, const EXTENTS: usize>(
    ptr: NonNull<()>,
    lease: MemoryPoolLease,
) -> Result<(), AllocError> {
    pool_ref::<MEMBERS, EXTENTS>(ptr)
        .release_extent(lease)
        .map_err(Into::into)
}

unsafe fn lease_region_impl<const MEMBERS: usize, const EXTENTS: usize>(
    ptr: NonNull<()>,
    lease: &MemoryPoolLease,
) -> Result<Region, AllocError> {
    let view = pool_ref::<MEMBERS, EXTENTS>(ptr).lease_view(lease)?;
    // SAFETY: the lease remains live while the assigned extent exists.
    Ok(unsafe { view.as_range_view().raw_region() })
}

unsafe fn member_info_impl<const MEMBERS: usize, const EXTENTS: usize>(
    ptr: NonNull<()>,
    member: MemoryPoolMemberId,
) -> Result<MemoryPoolMemberInfo, AllocError> {
    pool_ref::<MEMBERS, EXTENTS>(ptr)
        .member_info(member)
        .map_err(Into::into)
}

unsafe fn member_info_at_impl<const MEMBERS: usize, const EXTENTS: usize>(
    ptr: NonNull<()>,
    index: usize,
) -> Result<Option<MemoryPoolMemberInfo>, AllocError> {
    pool_ref::<MEMBERS, EXTENTS>(ptr)
        .member_info_at(index)
        .map_err(Into::into)
}

unsafe fn extent_info_at_impl<const MEMBERS: usize, const EXTENTS: usize>(
    ptr: NonNull<()>,
    index: usize,
) -> Result<Option<MemoryPoolExtentInfo>, AllocError> {
    pool_ref::<MEMBERS, EXTENTS>(ptr)
        .extent_info_at(index)
        .map_err(Into::into)
}

unsafe fn stats_impl<const MEMBERS: usize, const EXTENTS: usize>(
    ptr: NonNull<()>,
) -> Result<MemoryPoolStats, AllocError> {
    pool_ref::<MEMBERS, EXTENTS>(ptr)
        .stats()
        .map_err(Into::into)
}

unsafe fn retain_impl<const MEMBERS: usize, const EXTENTS: usize>(
    ptr: NonNull<()>,
) -> Result<(), AllocError> {
    pool_block::<MEMBERS, EXTENTS>(ptr)
        .header
        .try_retain()
        .map_err(|error| AllocError::synchronization(error.kind))
}

unsafe fn release_impl<const MEMBERS: usize, const EXTENTS: usize>(ptr: NonNull<()>) {
    let Ok(release) = pool_block::<MEMBERS, EXTENTS>(ptr).header.release() else {
        return;
    };
    if release != SharedRelease::Last {
        return;
    }
    let block = ptr.cast::<PoolControlBlock<MEMBERS, EXTENTS>>().as_ptr();
    let storage = unsafe { (*block).storage };
    // SAFETY: the final reference exclusively owns the control mapping. Move the pool out of the
    // control block before dropping it so in-band contributors can tear their own backing down
    // without invalidating the pool value mid-drop.
    unsafe {
        let pool = ManuallyDrop::take(&mut (*block).pool);
        drop(pool);
        let _ = storage;
    }
}

pub(crate) struct AssignedPoolExtent {
    pool: PoolHandle,
    pool_marker: usize,
    lease: Option<MemoryPoolLease>,
    lease_id: MemoryPoolLeaseId,
    region: Region,
    member: MemoryPoolMemberInfo,
}

impl core::fmt::Debug for AssignedPoolExtent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AssignedPoolExtent")
            .field("pool_marker", &self.pool_marker)
            .field("lease_id", &self.lease_id)
            .field("region", &self.region)
            .field("member", &self.member)
            .finish_non_exhaustive()
    }
}

impl AssignedPoolExtent {
    pub(crate) fn assign(
        pool: PoolHandle,
        request: &MemoryPoolExtentRequest,
    ) -> Result<Self, AllocError> {
        let lease = pool.acquire_extent(request)?;
        let lease_id = lease.id();
        let member = pool.member_info(lease.member())?;
        let region = pool.lease_region(&lease)?;
        Ok(Self {
            pool_marker: pool.marker(),
            pool,
            lease: Some(lease),
            lease_id,
            region,
            member,
        })
    }

    pub(crate) const fn pool_marker(&self) -> usize {
        self.pool_marker
    }

    pub(crate) const fn lease_id(&self) -> MemoryPoolLeaseId {
        self.lease_id
    }

    pub(crate) const fn region(&self) -> Region {
        self.region
    }

    pub(crate) const fn member(&self) -> MemoryPoolMemberInfo {
        self.member
    }
}

impl Drop for AssignedPoolExtent {
    fn drop(&mut self) {
        if let Some(lease) = self.lease.take() {
            let _ = self.pool.release_extent(lease);
        }
    }
}

#[derive(Debug)]
struct ExtentLeaseState {
    payload_offset: usize,
    len: usize,
    align: usize,
}

/// One exact owned pool extent leased from an allocator domain.
///
/// This is the public owned exact-backing surface for callers that need stable directly-usable
/// bytes without routing through slab or arena policy. Dropping the lease returns the extent to
/// the owning allocator domain.
pub struct ExtentLease {
    control: ControlLease<ExtentLeaseState>,
}

impl ExtentLease {
    fn payload_offset_for_request(request: MemoryPoolExtentRequest) -> Result<usize, AllocError> {
        let payload_align = request
            .align
            .max(ControlLease::<ExtentLeaseState>::backing_align());
        let header_bytes = ControlLease::<ExtentLeaseState>::backing_size();
        let mask = payload_align
            .checked_sub(1)
            .ok_or_else(AllocError::invalid_request)?;
        header_bytes
            .checked_add(mask)
            .map(|value| value & !mask)
            .ok_or_else(AllocError::invalid_request)
    }

    pub(crate) fn extent_request(
        request: MemoryPoolExtentRequest,
    ) -> Result<MemoryPoolExtentRequest, AllocError> {
        if request.len == 0 || request.align == 0 || !request.align.is_power_of_two() {
            return Err(AllocError::invalid_request());
        }
        let payload_offset = Self::payload_offset_for_request(request)?;
        let len = payload_offset
            .checked_add(request.len)
            .ok_or_else(AllocError::invalid_request)?;
        Ok(MemoryPoolExtentRequest {
            len,
            align: request
                .align
                .max(ControlLease::<ExtentLeaseState>::backing_align()),
        })
    }

    pub(crate) fn new(
        extent: AssignedPoolExtent,
        request: MemoryPoolExtentRequest,
    ) -> Result<Self, AllocError> {
        let payload_offset = Self::payload_offset_for_request(request)?;
        let region = extent.region();
        let payload_base = region
            .base
            .checked_add(payload_offset)
            .ok_or_else(AllocError::invalid_request)?;
        if region.len < payload_offset.saturating_add(request.len)
            || !payload_base.get().is_multiple_of(request.align)
        {
            return Err(AllocError::invalid_request());
        }
        let control = ControlLease::new(
            extent,
            ExtentLeaseState {
                payload_offset,
                len: request.len,
                align: request.align,
            },
        )?;
        Ok(Self { control })
    }

    /// Returns the owned payload region backing this extent.
    #[must_use]
    pub fn region(&self) -> Region {
        Region {
            base: self
                .control
                .region()
                .base
                .checked_add(self.control.payload_offset)
                .expect("extent payload base should remain within the owned region"),
            len: self.control.len,
        }
    }

    /// Returns the requested usable length in bytes for this extent.
    #[must_use]
    pub fn len(&self) -> usize {
        self.control.len
    }

    /// Returns whether this extent requested zero bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.control.len == 0
    }

    /// Returns the requested alignment in bytes for this extent.
    #[must_use]
    pub fn align(&self) -> usize {
        self.control.align
    }

    /// Returns the stable writable base pointer for this extent payload.
    #[must_use]
    pub fn as_non_null(&self) -> NonNull<u8> {
        self.region()
            .base
            .as_non_null::<u8>()
            .expect("leased extent payload base should never be null")
    }
}

impl core::fmt::Debug for ExtentLease {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ExtentLease")
            .field("region", &self.region())
            .field("len", &self.len())
            .field("align", &self.align())
            .finish_non_exhaustive()
    }
}

pub(crate) const fn pool_control_backing_request<const MEMBERS: usize, const EXTENTS: usize>()
-> Result<MemoryPoolExtentRequest, AllocError> {
    let len = size_of::<PoolControlBlock<MEMBERS, EXTENTS>>();
    if len == 0 {
        return Err(AllocError::invalid_request());
    }
    Ok(MemoryPoolExtentRequest {
        len,
        align: align_of::<PoolControlBlock<MEMBERS, EXTENTS>>(),
    })
}

pub(crate) fn align_up(value: usize, align: usize) -> Result<usize, AllocError> {
    if align == 0 || !align.is_power_of_two() {
        return Err(AllocError::invalid_request());
    }

    let mask = align - 1;
    value
        .checked_add(mask)
        .map(|rounded| rounded & !mask)
        .ok_or_else(AllocError::invalid_request)
}

/// Unified low-level allocator strategy contract.
pub trait AllocationStrategy {
    /// Returns the allocator's governing policy.
    fn policy(&self) -> AllocPolicy;

    /// Returns the coarse capability surface this allocator intends to provide.
    fn capabilities(&self) -> AllocCapabilities;

    /// Returns the coarse hazards this allocator may expose.
    fn hazards(&self) -> AllocHazards;

    /// Attempts to allocate one block matching `request`.
    ///
    /// # Errors
    ///
    /// Returns an error when the request is invalid, denied by policy, or unsupported by the
    /// current allocator implementation.
    fn allocate(&self, request: &AllocRequest) -> Result<AllocResult, AllocError>;

    /// Releases a previously allocated block.
    ///
    /// # Errors
    ///
    /// Returns an error when deallocation is unsupported or the allocator cannot accept the
    /// supplied result record honestly.
    fn deallocate(&self, allocation: AllocResult) -> Result<(), AllocError>;
}
