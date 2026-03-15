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

use core::ptr::NonNull;

mod arena;
mod domain;
mod error;
mod heap;
mod policy;
mod root;
mod slab;

pub use arena::BoundedArena;
pub use domain::{AllocatorDomainId, AllocatorDomainInfo, AllocatorDomainKind};
pub use error::{AllocError, AllocErrorKind};
pub use heap::HeapAllocator;
pub use policy::{AllocCapabilities, AllocHazards, AllocModeSet, AllocPolicy};
pub use root::{Allocator, AllocatorBuilder};
pub use slab::Slab;

pub use crate::mem::pool::{
    MemoryPool, MemoryPoolBuilder, MemoryPoolContributor, MemoryPoolContributorOrigin,
    MemoryPoolError, MemoryPoolErrorKind, MemoryPoolExtentRequest, MemoryPoolLease,
    MemoryPoolLeaseId, MemoryPoolLeaseView, MemoryPoolMemberId, MemoryPoolMemberInfo,
    MemoryPoolMetadataLayout, MemoryPoolPolicy, MemoryPoolProvisioningPolicy, MemoryPoolStats,
};
pub use crate::mem::provider::CriticalSafetyRequirements;
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

/// Successful allocator result together with the resource truth attached to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
