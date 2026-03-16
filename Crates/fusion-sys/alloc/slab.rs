use core::array;
use core::fmt;
use core::ptr::NonNull;

use crate::sync::Mutex;

use super::{
    AllocCapabilities, AllocError, AllocHazards, AllocModeSet, AllocPolicy, AllocRequest,
    AllocResult, AllocationBacking, AllocationStrategy, AllocatorDomainId, MemoryPoolLeaseId,
    SharedDomainPool, shared_pool_marker,
};

#[derive(Debug)]
struct SlabState<const COUNT: usize> {
    slots: [bool; COUNT],
}

impl<const COUNT: usize> SlabState<COUNT> {
    fn new() -> Self {
        Self {
            slots: array::from_fn(|_| true),
        }
    }

    fn allocate_slot(&mut self) -> Option<usize> {
        let slot = self.slots.iter().position(|free| *free)?;
        self.slots[slot] = false;
        Some(slot)
    }

    fn release_slot(&mut self, slot: usize) -> Result<(), AllocError> {
        let Some(free) = self.slots.get_mut(slot) else {
            return Err(AllocError::invalid_request());
        };
        if *free {
            return Err(AllocError::invalid_request());
        }
        *free = true;
        Ok(())
    }
}

/// Fixed-size, bounded allocator on top of one allocator-owned pool extent.
pub struct Slab<const SIZE: usize, const COUNT: usize> {
    domain: AllocatorDomainId,
    policy: AllocPolicy,
    pool: SharedDomainPool,
    lease: Option<super::MemoryPoolLease>,
    lease_id: MemoryPoolLeaseId,
    pool_marker: usize,
    member: super::MemoryPoolMemberInfo,
    slot_align: usize,
    state: Mutex<SlabState<COUNT>>,
}

impl<const SIZE: usize, const COUNT: usize> fmt::Debug for Slab<SIZE, COUNT> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Slab")
            .field("domain", &self.domain)
            .field("policy", &self.policy)
            .field("lease_id", &self.lease_id)
            .field("slot_align", &self.slot_align)
            .finish_non_exhaustive()
    }
}

impl<const SIZE: usize, const COUNT: usize> Slab<SIZE, COUNT> {
    pub(super) fn for_domain(
        domain: AllocatorDomainId,
        policy: AllocPolicy,
        pool: Option<SharedDomainPool>,
    ) -> Result<Self, AllocError> {
        if SIZE == 0 || COUNT == 0 {
            return Err(AllocError::invalid_request());
        }
        if !policy.allows(AllocModeSet::SLAB) {
            return Err(AllocError::policy_denied());
        }
        let pool = pool.ok_or_else(AllocError::capacity_exhausted)?;
        let slot_align = slab_slot_align::<SIZE>()?;
        let total_len = SIZE
            .checked_mul(COUNT)
            .ok_or_else(AllocError::invalid_request)?;
        let lease = pool.acquire_extent(&super::MemoryPoolExtentRequest {
            len: total_len,
            align: slot_align,
        })?;
        let lease_id = lease.id();
        let member = pool.member_info(lease.member())?;

        Ok(Self {
            domain,
            policy,
            pool_marker: shared_pool_marker(&pool),
            pool,
            lease: Some(lease),
            lease_id,
            member,
            slot_align,
            state: Mutex::new(SlabState::new()),
        })
    }

    /// Returns the capability surface a slab allocator provides.
    #[must_use]
    pub const fn supported_capabilities() -> AllocCapabilities {
        AllocCapabilities::SLAB
            .union(AllocCapabilities::ZEROED_ALLOC)
            .union(AllocCapabilities::DETERMINISTIC)
            .union(AllocCapabilities::BOUNDED)
    }

    /// Returns the expected coarse slab hazards.
    #[must_use]
    pub const fn expected_hazards() -> AllocHazards {
        AllocHazards::empty()
    }

    /// Returns the slab policy.
    #[must_use]
    pub const fn policy(&self) -> AllocPolicy {
        self.policy
    }

    /// Returns the owning allocator domain.
    #[must_use]
    pub const fn domain(&self) -> AllocatorDomainId {
        self.domain
    }

    /// Returns the fixed slot alignment guaranteed by this slab.
    #[must_use]
    pub const fn slot_align(&self) -> usize {
        self.slot_align
    }
}

impl<const SIZE: usize, const COUNT: usize> AllocationStrategy for Slab<SIZE, COUNT> {
    fn policy(&self) -> AllocPolicy {
        self.policy
    }

    fn capabilities(&self) -> AllocCapabilities {
        Self::supported_capabilities()
    }

    fn hazards(&self) -> AllocHazards {
        Self::expected_hazards()
    }

    fn allocate(&self, request: &AllocRequest) -> Result<AllocResult, AllocError> {
        if request.len == 0 || request.align == 0 || !request.align.is_power_of_two() {
            return Err(AllocError::invalid_request());
        }
        if request.len > SIZE || request.align > self.slot_align {
            return Err(AllocError::invalid_request());
        }

        let lease = self
            .lease
            .as_ref()
            .ok_or_else(AllocError::invalid_request)?;
        let slot = {
            let mut state = self
                .state
                .lock()
                .map_err(|error| AllocError::synchronization(error.kind))?;
            state
                .allocate_slot()
                .ok_or_else(AllocError::capacity_exhausted)?
        };

        let region = self.pool.lease_region(lease)?;
        let base = region.base.as_ptr() as usize;
        let addr = base
            .checked_add(
                slot.checked_mul(SIZE)
                    .ok_or_else(AllocError::invalid_request)?,
            )
            .ok_or_else(AllocError::invalid_request)?;
        let ptr = NonNull::new(addr as *mut u8).ok_or_else(AllocError::invalid_request)?;

        if request.zeroed {
            // SAFETY: the slab owns the reserved extent and slot allocation guarantees exclusive
            // access to this slot until the returned token is released.
            unsafe {
                ptr.as_ptr().write_bytes(0, SIZE);
            }
        }

        Ok(AllocResult::from_parts(
            ptr,
            SIZE,
            self.slot_align,
            self.member.compatibility.domain,
            self.member.compatibility.attrs,
            self.member.compatibility.hazards,
            self.member.compatibility.geometry,
            AllocationBacking::SlabSlot {
                pool_marker: self.pool_marker,
                lease_id: self.lease_id,
                slot,
            },
        ))
    }

    fn deallocate(&self, allocation: AllocResult) -> Result<(), AllocError> {
        match allocation.backing {
            AllocationBacking::SlabSlot {
                pool_marker,
                lease_id,
                slot,
            } if pool_marker == self.pool_marker && lease_id == self.lease_id => {
                let mut state = self
                    .state
                    .lock()
                    .map_err(|error| AllocError::synchronization(error.kind))?;
                state.release_slot(slot)
            }
            _ => Err(AllocError::invalid_request()),
        }
    }
}

impl<const SIZE: usize, const COUNT: usize> Drop for Slab<SIZE, COUNT> {
    fn drop(&mut self) {
        if let Some(lease) = self.lease.take() {
            let _ = self.pool.release_extent(lease);
        }
    }
}

fn slab_slot_align<const SIZE: usize>() -> Result<usize, AllocError> {
    if SIZE == 0 {
        return Err(AllocError::invalid_request());
    }
    Ok(1usize << SIZE.trailing_zeros())
}
