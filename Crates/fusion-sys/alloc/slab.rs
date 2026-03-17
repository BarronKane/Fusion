use core::array;
use core::fmt;
use core::ptr::NonNull;

use crate::sync::Mutex;

use super::{
    AllocCapabilities, AllocError, AllocHazards, AllocModeSet, AllocPolicy, AllocRequest,
    AllocResult, AllocationBacking, AllocationStrategy, AllocatorDomainId, AssignedPoolExtent,
};

#[derive(Debug)]
struct SlabState<const COUNT: usize> {
    free: [usize; COUNT],
    occupied: [bool; COUNT],
    len: usize,
}

impl<const COUNT: usize> SlabState<COUNT> {
    fn new() -> Self {
        Self {
            free: array::from_fn(|index| COUNT.saturating_sub(index + 1)),
            occupied: array::from_fn(|_| false),
            len: COUNT,
        }
    }

    fn allocate_slot(&mut self) -> Option<usize> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        let slot = self.free[self.len];
        let occupied = self.occupied.get_mut(slot)?;
        if *occupied {
            return None;
        }
        *occupied = true;
        Some(slot)
    }

    fn release_slot(&mut self, slot: usize) -> Result<(), AllocError> {
        let Some(occupied) = self.occupied.get_mut(slot) else {
            return Err(AllocError::invalid_request());
        };
        if !*occupied || self.len == COUNT {
            return Err(AllocError::invalid_request());
        }
        *occupied = false;
        self.free[self.len] = slot;
        self.len += 1;
        Ok(())
    }
}

/// Fixed-size, bounded allocator on top of one allocator-owned pool extent.
pub struct Slab<const SIZE: usize, const COUNT: usize> {
    domain: AllocatorDomainId,
    policy: AllocPolicy,
    extent: AssignedPoolExtent,
    slot_align: usize,
    state: Mutex<SlabState<COUNT>>,
}

impl<const SIZE: usize, const COUNT: usize> fmt::Debug for Slab<SIZE, COUNT> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Slab")
            .field("domain", &self.domain)
            .field("policy", &self.policy)
            .field("lease_id", &self.extent.lease_id())
            .field("slot_align", &self.slot_align)
            .finish_non_exhaustive()
    }
}

impl<const SIZE: usize, const COUNT: usize> Slab<SIZE, COUNT> {
    pub(super) const fn extent_request(
        slot_align: usize,
    ) -> Result<super::MemoryPoolExtentRequest, AllocError> {
        if SIZE == 0 || COUNT == 0 {
            return Err(AllocError::invalid_request());
        }
        let Some(total_len) = SIZE.checked_mul(COUNT) else {
            return Err(AllocError::invalid_request());
        };
        Ok(super::MemoryPoolExtentRequest {
            len: total_len,
            align: slot_align,
        })
    }

    pub(super) fn slot_align_for_domain() -> Result<usize, AllocError> {
        slab_slot_align::<SIZE>()
    }

    pub(super) fn from_assigned_extent(
        domain: AllocatorDomainId,
        policy: AllocPolicy,
        extent: AssignedPoolExtent,
    ) -> Result<Self, AllocError> {
        if SIZE == 0 || COUNT == 0 {
            return Err(AllocError::invalid_request());
        }
        if !policy.allows(AllocModeSet::SLAB) {
            return Err(AllocError::policy_denied());
        }
        let slot_align = slab_slot_align::<SIZE>()?;

        Ok(Self {
            domain,
            policy,
            extent,
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

        let slot = {
            let mut state = self
                .state
                .lock()
                .map_err(|error| AllocError::synchronization(error.kind))?;
            state
                .allocate_slot()
                .ok_or_else(AllocError::capacity_exhausted)?
        };

        let region = self.extent.region();
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
            self.extent.member().compatibility.domain,
            self.extent.member().compatibility.attrs,
            self.extent.member().compatibility.hazards,
            self.extent.member().compatibility.geometry,
            AllocationBacking::SlabSlot {
                pool_marker: self.extent.pool_marker(),
                lease_id: self.extent.lease_id(),
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
            } if pool_marker == self.extent.pool_marker() && lease_id == self.extent.lease_id() => {
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

fn slab_slot_align<const SIZE: usize>() -> Result<usize, AllocError> {
    if SIZE == 0 {
        return Err(AllocError::invalid_request());
    }
    Ok(1usize << SIZE.trailing_zeros())
}
