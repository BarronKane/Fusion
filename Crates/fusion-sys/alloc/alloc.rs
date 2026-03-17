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

use crate::rust_alloc::boxed::Box;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};
use fusion_pal::sys::mem::Region;

mod arena;
mod control;
mod domain;
mod error;
mod heap;
mod policy;
mod root;
mod slab;

pub use arena::{ArenaInitError, ArenaSlice, BoundedArena};
pub use control::ControlLease;
pub use domain::{AllocatorDomainId, AllocatorDomainInfo, AllocatorDomainKind};
#[allow(unused_imports)]
pub use error::{AllocError, AllocErrorKind};
pub use heap::HeapAllocator;
pub use policy::{AllocCapabilities, AllocHazards, AllocModeSet, AllocPolicy};
#[allow(unused_imports)]
pub use root::{Allocator, AllocatorBuilder};
pub use slab::Slab;

#[allow(unused_imports)]
pub use crate::mem::pool::{
    MemoryPool, MemoryPoolBuilder, MemoryPoolContributor, MemoryPoolContributorOrigin,
    MemoryPoolError, MemoryPoolErrorKind, MemoryPoolExtentRequest, MemoryPoolLease,
    MemoryPoolLeaseId, MemoryPoolLeaseView, MemoryPoolMemberId, MemoryPoolMemberInfo,
    MemoryPoolMetadataLayout, MemoryPoolPolicy, MemoryPoolProvisioningPolicy, MemoryPoolStats,
};
pub use crate::mem::provider::CriticalSafetyRequirements;
#[allow(unused_imports)]
pub use crate::mem::resource::{
    MemoryDomain, MemoryDomainSet, MemoryGeometry, ResourceAttrs, ResourceHazardSet,
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
    member_info:
        unsafe fn(NonNull<()>, MemoryPoolMemberId) -> Result<MemoryPoolMemberInfo, AllocError>,
    retain: unsafe fn(NonNull<()>) -> Result<(), AllocError>,
    release: unsafe fn(NonNull<()>),
}

struct PoolControlBlock<const MEMBERS: usize, const EXTENTS: usize> {
    refs: AtomicUsize,
    pool: MemoryPool<MEMBERS, EXTENTS>,
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
    pub(crate) fn new<const MEMBERS: usize, const EXTENTS: usize>(
        pool: MemoryPool<MEMBERS, EXTENTS>,
    ) -> Self {
        let block = Box::new(PoolControlBlock {
            refs: AtomicUsize::new(1),
            pool,
        });
        let ptr = NonNull::from(Box::leak(block)).cast::<()>();
        Self {
            ptr,
            vtable: PoolHandleVTable {
                acquire_extent: acquire_extent_impl::<MEMBERS, EXTENTS>,
                release_extent: release_extent_impl::<MEMBERS, EXTENTS>,
                lease_region: lease_region_impl::<MEMBERS, EXTENTS>,
                member_info: member_info_impl::<MEMBERS, EXTENTS>,
                retain: retain_impl::<MEMBERS, EXTENTS>,
                release: release_impl::<MEMBERS, EXTENTS>,
            },
        }
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

    pub(crate) fn member_info(
        &self,
        member: MemoryPoolMemberId,
    ) -> Result<MemoryPoolMemberInfo, AllocError> {
        unsafe { (self.vtable.member_info)(self.ptr, member) }
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
    // SAFETY: the pointer is created from a leaked `PoolControlBlock<MEMBERS, EXTENTS>` and the
    // vtable ensures each method uses the matching concrete instantiation.
    unsafe { &*ptr.cast::<PoolControlBlock<MEMBERS, EXTENTS>>().as_ptr() }
}

unsafe fn acquire_extent_impl<const MEMBERS: usize, const EXTENTS: usize>(
    ptr: NonNull<()>,
    request: &MemoryPoolExtentRequest,
) -> Result<MemoryPoolLease, AllocError> {
    pool_block::<MEMBERS, EXTENTS>(ptr)
        .pool
        .acquire_extent(request)
        .map_err(Into::into)
}

unsafe fn release_extent_impl<const MEMBERS: usize, const EXTENTS: usize>(
    ptr: NonNull<()>,
    lease: MemoryPoolLease,
) -> Result<(), AllocError> {
    pool_block::<MEMBERS, EXTENTS>(ptr)
        .pool
        .release_extent(lease)
        .map_err(Into::into)
}

unsafe fn lease_region_impl<const MEMBERS: usize, const EXTENTS: usize>(
    ptr: NonNull<()>,
    lease: &MemoryPoolLease,
) -> Result<Region, AllocError> {
    let view = pool_block::<MEMBERS, EXTENTS>(ptr).pool.lease_view(lease)?;
    // SAFETY: the lease remains live while the assigned extent exists.
    Ok(unsafe { view.as_range_view().raw_region() })
}

unsafe fn member_info_impl<const MEMBERS: usize, const EXTENTS: usize>(
    ptr: NonNull<()>,
    member: MemoryPoolMemberId,
) -> Result<MemoryPoolMemberInfo, AllocError> {
    pool_block::<MEMBERS, EXTENTS>(ptr)
        .pool
        .member_info(member)
        .map_err(Into::into)
}

unsafe fn retain_impl<const MEMBERS: usize, const EXTENTS: usize>(
    ptr: NonNull<()>,
) -> Result<(), AllocError> {
    let refs = &pool_block::<MEMBERS, EXTENTS>(ptr).refs;
    refs.fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
        current.checked_add(1)
    })
    .map(|_| ())
    .map_err(|_| AllocError::capacity_exhausted())
}

unsafe fn release_impl<const MEMBERS: usize, const EXTENTS: usize>(ptr: NonNull<()>) {
    let refs = &pool_block::<MEMBERS, EXTENTS>(ptr).refs;
    let Ok(previous) = refs.fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
        current.checked_sub(1)
    }) else {
        return;
    };
    if previous != 1 {
        return;
    }
    // SAFETY: the final reference owns the leaked control block and no further users remain.
    unsafe {
        drop(Box::from_raw(
            ptr.cast::<PoolControlBlock<MEMBERS, EXTENTS>>().as_ptr(),
        ));
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
