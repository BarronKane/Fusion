use core::fmt;
use core::ptr::NonNull;

use crate::sync::Mutex;

use super::{
    AllocCapabilities, AllocError, AllocHazards, AllocModeSet, AllocPolicy, AllocRequest,
    AllocResult, AllocationBacking, AllocationStrategy, AllocatorDomainId, AssignedPoolExtent,
    align_up,
};

#[derive(Debug)]
struct ArenaState {
    cursor: usize,
}

/// Bounded lifetime allocator intended for bulk-free or reset-driven use.
pub struct BoundedArena {
    capacity: usize,
    domain: AllocatorDomainId,
    policy: AllocPolicy,
    extent: AssignedPoolExtent,
    state: Mutex<ArenaState>,
}

impl fmt::Debug for BoundedArena {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoundedArena")
            .field("capacity", &self.capacity)
            .field("domain", &self.domain)
            .field("policy", &self.policy)
            .field("lease_id", &self.extent.lease_id())
            .finish_non_exhaustive()
    }
}

impl BoundedArena {
    pub(super) const fn extent_request(capacity: usize) -> Result<super::MemoryPoolExtentRequest, AllocError> {
        if capacity == 0 {
            return Err(AllocError::invalid_request());
        }
        Ok(super::MemoryPoolExtentRequest {
            len: capacity,
            align: 1,
        })
    }

    pub(super) fn from_assigned_extent(
        domain: AllocatorDomainId,
        capacity: usize,
        policy: AllocPolicy,
        extent: AssignedPoolExtent,
    ) -> Result<Self, AllocError> {
        if capacity == 0 {
            return Err(AllocError::invalid_request());
        }
        if !policy.allows(AllocModeSet::ARENA) {
            return Err(AllocError::policy_denied());
        }

        Ok(Self {
            capacity,
            domain,
            policy,
            extent,
            state: Mutex::new(ArenaState { cursor: 0 }),
        })
    }

    /// Returns the capability surface a bounded arena provides.
    #[must_use]
    pub const fn supported_capabilities() -> AllocCapabilities {
        AllocCapabilities::ARENA
            .union(AllocCapabilities::DETERMINISTIC)
            .union(AllocCapabilities::BOUNDED)
    }

    /// Returns the expected coarse arena hazards.
    #[must_use]
    pub const fn expected_hazards() -> AllocHazards {
        AllocHazards::empty()
    }

    /// Returns the configured bounded capacity in bytes.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the arena policy.
    #[must_use]
    pub const fn policy(&self) -> AllocPolicy {
        self.policy
    }

    /// Returns the owning allocator domain.
    #[must_use]
    pub const fn domain(&self) -> AllocatorDomainId {
        self.domain
    }

    /// Resets the arena cursor to the beginning of the reserved extent.
    ///
    /// # Errors
    ///
    /// Returns an error when the arena cannot synchronize its cursor state honestly.
    pub fn reset(&self) -> Result<(), AllocError> {
        let mut state = self
            .state
            .lock()
            .map_err(|error| AllocError::synchronization(error.kind))?;
        state.cursor = 0;
        Ok(())
    }
}

impl AllocationStrategy for BoundedArena {
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

        let region = self.extent.region();
        let base = region.base.as_ptr() as usize;
        let mut state = self
            .state
            .lock()
            .map_err(|error| AllocError::synchronization(error.kind))?;
        let start = align_up(
            base.checked_add(state.cursor)
                .ok_or_else(AllocError::invalid_request)?,
            request.align,
        )?;
        let offset = start
            .checked_sub(base)
            .ok_or_else(AllocError::invalid_request)?;
        let end = offset
            .checked_add(request.len)
            .ok_or_else(AllocError::invalid_request)?;
        if end > self.capacity {
            return Err(AllocError::capacity_exhausted());
        }

        state.cursor = end;
        let ptr = NonNull::new(start as *mut u8).ok_or_else(AllocError::invalid_request)?;
        if request.zeroed {
            // SAFETY: the arena owns the reserved extent and the cursor discipline grants
            // exclusive access to the newly allocated range until the caller releases or resets it.
            unsafe {
                ptr.as_ptr().write_bytes(0, request.len);
            }
        }

        Ok(AllocResult::from_parts(
            ptr,
            request.len,
            request.align,
            self.extent.member().compatibility.domain,
            self.extent.member().compatibility.attrs,
            self.extent.member().compatibility.hazards,
            self.extent.member().compatibility.geometry,
            AllocationBacking::ArenaBlock {
                pool_marker: self.extent.pool_marker(),
                lease_id: self.extent.lease_id(),
                offset,
                len: request.len,
            },
        ))
    }

    fn deallocate(&self, allocation: AllocResult) -> Result<(), AllocError> {
        let AllocationBacking::ArenaBlock {
            pool_marker,
            lease_id,
            offset,
            len,
        } = allocation.backing
        else {
            return Err(AllocError::invalid_request());
        };
        if pool_marker != self.extent.pool_marker() || lease_id != self.extent.lease_id() {
            return Err(AllocError::invalid_request());
        }

        let mut state = self
            .state
            .lock()
            .map_err(|error| AllocError::synchronization(error.kind))?;
        let end = offset
            .checked_add(len)
            .ok_or_else(AllocError::invalid_request)?;
        if end != state.cursor {
            return Err(AllocError::invalid_request());
        }

        state.cursor = offset;
        Ok(())
    }
}
