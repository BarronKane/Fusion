use core::array;
use core::fmt;
use core::marker::PhantomData;
use core::mem::{ManuallyDrop, align_of, size_of};
use core::ptr::{self, NonNull};

use crate::sync::{Mutex, RetainedHandle};

use super::{
    AllocCapabilities, AllocError, AllocHazards, AllocModeSet, AllocPolicy, AllocRequest,
    AllocResult, AllocSubsystemKind, AllocationBacking, AllocationStrategy, AllocatorDomainId,
    AssignedPoolExtent, Immortal, LifetimePolicy, MetadataPageHeader, Mortal,
    front_metadata_layout,
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

#[repr(C)]
struct SlabMetadata<const COUNT: usize> {
    header: MetadataPageHeader,
    state: Mutex<SlabState<COUNT>>,
}

impl<const COUNT: usize> fmt::Debug for SlabMetadata<COUNT> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SlabMetadata")
            .field("header", &self.header)
            .finish_non_exhaustive()
    }
}

/// Fixed-size, bounded allocator on top of one allocator-owned pool extent.
pub struct Slab<const SIZE: usize, const COUNT: usize, L: LifetimePolicy = Mortal> {
    domain: AllocatorDomainId,
    policy: AllocPolicy,
    extent: ManuallyDrop<AssignedPoolExtent>,
    slot_align: usize,
    metadata: NonNull<SlabMetadata<COUNT>>,
    _lifetime: PhantomData<L>,
}

impl<const SIZE: usize, const COUNT: usize, L: LifetimePolicy> fmt::Debug for Slab<SIZE, COUNT, L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Slab")
            .field("domain", &self.domain)
            .field("policy", &self.policy)
            .field("lease_id", &self.extent().lease_id())
            .field("slot_align", &self.slot_align)
            .field("immortal", &L::IMMORTAL)
            .finish_non_exhaustive()
    }
}

impl<const SIZE: usize, const COUNT: usize, L: LifetimePolicy> Slab<SIZE, COUNT, L> {
    pub(super) fn extent_request(
        slot_align: usize,
    ) -> Result<super::MemoryPoolExtentRequest, AllocError> {
        if SIZE == 0 || COUNT == 0 {
            return Err(AllocError::invalid_request());
        }
        let Some(payload_len) = SIZE.checked_mul(COUNT) else {
            return Err(AllocError::invalid_request());
        };
        let layout = front_metadata_layout(
            size_of::<SlabMetadata<COUNT>>(),
            align_of::<SlabMetadata<COUNT>>(),
            payload_len,
            slot_align,
        )?;
        Ok(super::MemoryPoolExtentRequest {
            len: layout.total_len,
            align: layout.request_align,
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
        let layout = front_metadata_layout(
            size_of::<SlabMetadata<COUNT>>(),
            align_of::<SlabMetadata<COUNT>>(),
            SIZE.checked_mul(COUNT)
                .ok_or_else(AllocError::invalid_request)?,
            slot_align,
        )?;
        let region = extent.region();
        if region.len < layout.total_len
            || !(region.base.as_ptr() as usize).is_multiple_of(align_of::<SlabMetadata<COUNT>>())
        {
            return Err(AllocError::invalid_request());
        }
        let metadata = region.base.cast::<SlabMetadata<COUNT>>();
        // SAFETY: the assigned extent is uniquely owned here, properly aligned for the slab
        // metadata, and reserves front-loaded metadata space ahead of the slab payload region.
        unsafe {
            metadata.as_ptr().write(SlabMetadata {
                header: MetadataPageHeader::new(
                    AllocSubsystemKind::Slab,
                    layout.metadata_len,
                    layout.payload_offset,
                    layout.payload_len,
                ),
                state: Mutex::new(SlabState::new()),
            });
        }

        Ok(Self {
            domain,
            policy,
            extent: ManuallyDrop::new(extent),
            slot_align,
            metadata,
            _lifetime: PhantomData,
        })
    }

    const fn extent(&self) -> &AssignedPoolExtent {
        // SAFETY: the slab owns the assigned extent and only skips dropping it for immortal
        // typestates, where leaking the backing is intentional.
        unsafe { &*((&raw const self.extent).cast::<AssignedPoolExtent>()) }
    }

    const fn metadata(&self) -> &SlabMetadata<COUNT> {
        // SAFETY: the slab owns the assigned extent and initializes the metadata exactly once.
        unsafe { self.metadata.as_ref() }
    }

    fn payload_base(&self) -> Result<usize, AllocError> {
        ((self.extent().region().base.as_ptr().cast::<u8>()) as usize)
            .checked_add(self.metadata().header.payload_offset)
            .ok_or_else(AllocError::invalid_request)
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

impl<const SIZE: usize, const COUNT: usize> Slab<SIZE, COUNT, Immortal> {
    /// Allocates one process-lifetime typed value from this immortal slab.
    ///
    /// The returned handle never drops `value`; the slot remains permanently occupied until
    /// process teardown.
    ///
    /// # Errors
    ///
    /// Returns an error when `T` cannot fit honestly inside one slab slot or the slab is full.
    pub fn alloc_retained_value<T: 'static>(
        &self,
        value: T,
    ) -> Result<RetainedHandle<T>, AllocError> {
        if size_of::<T>() == 0 || size_of::<T>() > SIZE || align_of::<T>() > self.slot_align {
            return Err(AllocError::invalid_request());
        }

        let slot = {
            let mut state = self
                .metadata()
                .state
                .lock()
                .map_err(|error| AllocError::synchronization(error.kind))?;
            state
                .allocate_slot()
                .ok_or_else(AllocError::capacity_exhausted)?
        };

        let addr = self
            .payload_base()?
            .checked_add(
                slot.checked_mul(SIZE)
                    .ok_or_else(AllocError::invalid_request)?,
            )
            .ok_or_else(AllocError::invalid_request)?;
        let ptr = NonNull::new(addr as *mut T).ok_or_else(AllocError::invalid_request)?;
        // SAFETY: the slab slot is exclusively owned after allocation and satisfies `T`'s size
        // and alignment requirements checked above.
        unsafe {
            ptr.as_ptr().write(value);
        }
        Ok(RetainedHandle::from_nonnull(ptr))
    }
}

impl<const SIZE: usize, const COUNT: usize, L: LifetimePolicy> AllocationStrategy
    for Slab<SIZE, COUNT, L>
{
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
                .metadata()
                .state
                .lock()
                .map_err(|error| AllocError::synchronization(error.kind))?;
            state
                .allocate_slot()
                .ok_or_else(AllocError::capacity_exhausted)?
        };

        let addr = self
            .payload_base()?
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
            self.extent().member().compatibility.domain,
            self.extent().member().compatibility.attrs,
            self.extent().member().compatibility.hazards,
            self.extent().member().compatibility.geometry,
            AllocationBacking::SlabSlot {
                pool_marker: self.extent().pool_marker(),
                lease_id: self.extent().lease_id(),
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
            } if pool_marker == self.extent().pool_marker()
                && lease_id == self.extent().lease_id() =>
            {
                let mut state = self
                    .metadata()
                    .state
                    .lock()
                    .map_err(|error| AllocError::synchronization(error.kind))?;
                state.release_slot(slot)
            }
            _ => Err(AllocError::invalid_request()),
        }
    }
}

impl<const SIZE: usize, const COUNT: usize, L: LifetimePolicy> Drop for Slab<SIZE, COUNT, L> {
    fn drop(&mut self) {
        if L::IMMORTAL {
            return;
        }
        // SAFETY: the slab is the unique owner of this metadata region and must drop the in-place
        // metadata before the backing extent is released.
        unsafe {
            ptr::drop_in_place(self.metadata.as_ptr());
            ManuallyDrop::drop(&mut self.extent);
        }
    }
}

// SAFETY: the slab metadata lives inside the owned extent and all mutable state is serialized by
// the embedded mutex.
unsafe impl<const SIZE: usize, const COUNT: usize, L: LifetimePolicy> Send
    for Slab<SIZE, COUNT, L>
{
}
// SAFETY: the slab metadata lives inside the owned extent and all mutable state is serialized by
// the embedded mutex.
unsafe impl<const SIZE: usize, const COUNT: usize, L: LifetimePolicy> Sync
    for Slab<SIZE, COUNT, L>
{
}

fn slab_slot_align<const SIZE: usize>() -> Result<usize, AllocError> {
    if SIZE == 0 {
        return Err(AllocError::invalid_request());
    }
    Ok(1usize << SIZE.trailing_zeros())
}
