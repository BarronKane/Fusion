use core::fmt;
use core::mem::{align_of, size_of};
use core::ops::{Deref, DerefMut};
use core::ptr::{self, NonNull};
use core::slice;

use crate::sync::Mutex;

use super::{
    AllocCapabilities, AllocError, AllocHazards, AllocModeSet, AllocPolicy, AllocRequest,
    AllocResult, AllocationBacking, AllocationStrategy, AllocatorDomainId, AssignedPoolExtent,
    ControlLease, align_up,
};

#[derive(Debug)]
struct ArenaState {
    cursor: usize,
    live_slices: usize,
}

#[derive(Debug)]
struct ArenaControl {
    capacity: usize,
    domain: AllocatorDomainId,
    policy: AllocPolicy,
    extent: AssignedPoolExtent,
    state: Mutex<ArenaState>,
}

/// Fallible initializer failure for arena-backed typed allocations.
#[derive(Debug)]
pub enum ArenaInitError<E> {
    /// The arena could not reserve backing for the requested typed allocation.
    Alloc(AllocError),
    /// The caller-provided initializer failed after arena space had been reserved.
    Init(E),
}

/// Typed contiguous slice backed by arena-owned memory.
pub struct ArenaSlice<T> {
    control: ControlLease<ArenaControl>,
    ptr: NonNull<T>,
    len: usize,
}

unsafe impl<T: Send> Send for ArenaSlice<T> {}
unsafe impl<T: Sync> Sync for ArenaSlice<T> {}

impl<T> ArenaSlice<T> {
    const fn new(control: ControlLease<ArenaControl>, ptr: NonNull<T>, len: usize) -> Self {
        Self { control, ptr, len }
    }

    /// Returns the number of typed elements in this slice.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns whether this slice is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the immutable slice view.
    #[must_use]
    pub const fn as_slice(&self) -> &[T] {
        // SAFETY: the slice wrapper owns `len` initialized contiguous elements at `ptr`, and the
        // shared arena control guarantees the backing extent remains live while the slice exists.
        unsafe { slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    /// Returns the mutable slice view.
    #[must_use]
    pub const fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: this wrapper uniquely owns the initialized typed range it describes.
        unsafe { slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }
}

impl<T> fmt::Debug for ArenaSlice<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArenaSlice")
            .field("ptr", &self.ptr)
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}

impl<T> Deref for ArenaSlice<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<T> DerefMut for ArenaSlice<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl<'a, T> IntoIterator for &'a ArenaSlice<T> {
    type Item = &'a T;
    type IntoIter = slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

impl<'a, T> IntoIterator for &'a mut ArenaSlice<T> {
    type Item = &'a mut T;
    type IntoIter = slice::IterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.as_mut_slice().iter_mut()
    }
}

impl<T> Drop for ArenaSlice<T> {
    fn drop(&mut self) {
        // SAFETY: the slice owns `len` initialized elements at `ptr`.
        unsafe {
            ptr::drop_in_place(ptr::slice_from_raw_parts_mut(self.ptr.as_ptr(), self.len));
        }
        if let Ok(mut state) = self.control.state.lock() {
            state.live_slices = state.live_slices.saturating_sub(1);
        }
    }
}

/// Bounded lifetime allocator intended for bulk-free or reset-driven use.
pub struct BoundedArena {
    control: ControlLease<ArenaControl>,
}

impl fmt::Debug for BoundedArena {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoundedArena")
            .field("capacity", &self.control.capacity)
            .field("domain", &self.control.domain)
            .field("policy", &self.control.policy)
            .field("lease_id", &self.control.extent.lease_id())
            .finish_non_exhaustive()
    }
}

impl BoundedArena {
    pub(super) const fn extent_request(
        capacity: usize,
    ) -> Result<super::MemoryPoolExtentRequest, AllocError> {
        if capacity == 0 {
            return Err(AllocError::invalid_request());
        }
        Ok(super::MemoryPoolExtentRequest {
            len: capacity,
            align: 1,
        })
    }

    pub(super) const fn control_extent_request(
    ) -> Result<super::MemoryPoolExtentRequest, AllocError> {
        ControlLease::<ArenaControl>::extent_request()
    }

    pub(super) fn from_assigned_extents(
        domain: AllocatorDomainId,
        capacity: usize,
        policy: AllocPolicy,
        extent: AssignedPoolExtent,
        control_extent: AssignedPoolExtent,
    ) -> Result<Self, AllocError> {
        if capacity == 0 {
            return Err(AllocError::invalid_request());
        }
        if !policy.allows(AllocModeSet::ARENA) {
            return Err(AllocError::policy_denied());
        }

        let control = ControlLease::new(
            control_extent,
            ArenaControl {
                capacity,
                domain,
                policy,
                extent,
                state: Mutex::new(ArenaState {
                    cursor: 0,
                    live_slices: 0,
                }),
            },
        )?;

        Ok(Self { control })
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
    pub fn capacity(&self) -> usize {
        self.control.capacity
    }

    /// Returns the arena policy.
    #[must_use]
    pub fn policy(&self) -> AllocPolicy {
        self.control.policy
    }

    /// Returns the owning allocator domain.
    #[must_use]
    pub fn domain(&self) -> AllocatorDomainId {
        self.control.domain
    }

    /// Resets the arena cursor to the beginning of the reserved extent.
    ///
    /// # Errors
    ///
    /// Returns an error when the arena still has live typed leases or cannot synchronize its
    /// cursor state honestly.
    pub fn reset(&self) -> Result<(), AllocError> {
        let mut state = self
            .control
            .state
            .lock()
            .map_err(|error| AllocError::synchronization(error.kind))?;
        if state.live_slices != 0 {
            return Err(AllocError::busy());
        }
        state.cursor = 0;
        Ok(())
    }

    /// Allocates one typed value from the arena.
    ///
    /// The returned value lives inside arena-owned storage and is dropped when the wrapper
    /// is dropped, while the backing extent remains live until the last arena or slice lease
    /// goes away.
    ///
    /// # Errors
    ///
    /// Returns an error when the typed request cannot be represented or the arena is full.
    pub fn alloc_value<T>(&self, value: T) -> Result<ArenaSlice<T>, AllocError> {
        let request = typed_request::<T>(1)?;
        let region = self.control.extent.region();
        let base = region.base.as_ptr() as usize;
        let mut state = self
            .control
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
        if end > self.control.capacity {
            return Err(AllocError::capacity_exhausted());
        }

        let control = self.control.try_clone()?;
        state.cursor = end;
        state.live_slices = state
            .live_slices
            .checked_add(1)
            .ok_or_else(AllocError::capacity_exhausted)?;
        let ptr = NonNull::new(start as *mut T).ok_or_else(AllocError::invalid_request)?;
        // SAFETY: the typed request reserved one aligned `T` slot, and the live-slice count now
        // keeps reset from invalidating the region until the returned slice is dropped.
        unsafe {
            ptr.as_ptr().write(value);
        }
        drop(state);
        Ok(ArenaSlice::new(control, ptr, 1))
    }

    /// Allocates and initializes one typed slice from the arena.
    ///
    /// # Errors
    ///
    /// Returns an error when the typed request cannot be represented or the arena is full.
    pub fn alloc_array_with<T, F>(&self, len: usize, mut init: F) -> Result<ArenaSlice<T>, AllocError>
    where
        F: FnMut(usize) -> T,
    {
        match self.try_alloc_array_with(len, |index| Ok::<T, ()>(init(index))) {
            Ok(slice) => Ok(slice),
            Err(ArenaInitError::Alloc(error)) => Err(error),
            Err(ArenaInitError::Init(())) => Err(AllocError::invalid_request()),
        }
    }

    /// Allocates and initializes one typed slice from the arena with a fallible initializer.
    ///
    /// # Errors
    ///
    /// Returns an allocator error when the typed request cannot be represented or the arena is
    /// full, or the initializer's own error after reclaiming the just-reserved arena region.
    pub fn try_alloc_array_with<T, E, F>(
        &self,
        len: usize,
        mut init: F,
    ) -> Result<ArenaSlice<T>, ArenaInitError<E>>
    where
        F: FnMut(usize) -> Result<T, E>,
    {
        let request = typed_request::<T>(len).map_err(ArenaInitError::Alloc)?;
        let region = self.control.extent.region();
        let base = region.base.as_ptr() as usize;
        let mut state = self
            .control
            .state
            .lock()
            .map_err(|error| ArenaInitError::Alloc(AllocError::synchronization(error.kind)))?;
        let start = align_up(
            base.checked_add(state.cursor)
                .ok_or_else(|| ArenaInitError::Alloc(AllocError::invalid_request()))?,
            request.align,
        )
        .map_err(ArenaInitError::Alloc)?;
        let offset = start
            .checked_sub(base)
            .ok_or_else(|| ArenaInitError::Alloc(AllocError::invalid_request()))?;
        let end = offset
            .checked_add(request.len)
            .ok_or_else(|| ArenaInitError::Alloc(AllocError::invalid_request()))?;
        if end > self.control.capacity {
            return Err(ArenaInitError::Alloc(AllocError::capacity_exhausted()));
        }

        let control = self.control.try_clone().map_err(ArenaInitError::Alloc)?;
        state.cursor = end;
        state.live_slices = state
            .live_slices
            .checked_add(1)
            .ok_or_else(|| ArenaInitError::Alloc(AllocError::capacity_exhausted()))?;
        let ptr = NonNull::new(start as *mut T)
            .ok_or_else(|| ArenaInitError::Alloc(AllocError::invalid_request()))?;
        let mut initialized = 0usize;

        for index in 0..len {
            match init(index) {
                Ok(value) => {
                    // SAFETY: the typed request reserved enough aligned space for `len`
                    // contiguous `T` values, and each slot is written at most once here.
                    unsafe {
                        ptr.as_ptr().add(index).write(value);
                    }
                    initialized += 1;
                }
                Err(error) => {
                    for initialized_index in 0..initialized {
                        // SAFETY: only the prefix `[0, initialized)` has been written so far.
                        unsafe {
                            ptr.as_ptr().add(initialized_index).drop_in_place();
                        }
                    }
                    state.cursor = offset;
                    state.live_slices = state.live_slices.saturating_sub(1);
                    return Err(ArenaInitError::Init(error));
                }
            }
        }

        drop(state);
        Ok(ArenaSlice::new(control, ptr, len))
    }
}

impl AllocationStrategy for BoundedArena {
    fn policy(&self) -> AllocPolicy {
        self.control.policy
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

        let region = self.control.extent.region();
        let base = region.base.as_ptr() as usize;
        let mut state = self
            .control
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
        if end > self.control.capacity {
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
            self.control.extent.member().compatibility.domain,
            self.control.extent.member().compatibility.attrs,
            self.control.extent.member().compatibility.hazards,
            self.control.extent.member().compatibility.geometry,
            AllocationBacking::ArenaBlock {
                pool_marker: self.control.extent.pool_marker(),
                lease_id: self.control.extent.lease_id(),
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
        if pool_marker != self.control.extent.pool_marker()
            || lease_id != self.control.extent.lease_id()
        {
            return Err(AllocError::invalid_request());
        }

        let mut state = self
            .control
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

fn typed_request<T>(len: usize) -> Result<AllocRequest, AllocError> {
    if len == 0 || size_of::<T>() == 0 {
        return Err(AllocError::invalid_request());
    }
    let byte_len = size_of::<T>()
        .checked_mul(len)
        .ok_or_else(AllocError::invalid_request)?;
    Ok(AllocRequest {
        len: byte_len,
        align: align_of::<T>(),
        zeroed: false,
    })
}
