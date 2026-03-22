use core::fmt;
use core::marker::PhantomData;
use core::mem::{ManuallyDrop, align_of, size_of};
use core::ops::{Deref, DerefMut};
use core::ptr::{self, NonNull};
use core::slice;

use crate::sync::{Mutex, RetainedHandle};

use super::{
    AllocCapabilities,
    AllocError,
    AllocHazards,
    AllocModeSet,
    AllocPolicy,
    AllocRequest,
    AllocResult,
    AllocSubsystemKind,
    AllocationBacking,
    AllocationStrategy,
    AllocatorDomainId,
    AssignedPoolExtent,
    ControlLease,
    Immortal,
    LifetimePolicy,
    MetadataPageHeader,
    Mortal,
    align_up,
    front_metadata_layout,
};

#[derive(Debug)]
struct ArenaState {
    cursor: usize,
    live_slices: usize,
}

#[derive(Debug)]
struct ArenaControl {
    header: MetadataPageHeader,
    max_align: usize,
    domain: AllocatorDomainId,
    policy: AllocPolicy,
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
///
/// The described element range has a stable address for the lifetime of this wrapper. A
/// `BoundedArena` never relocates previously allocated regions; it only advances one cursor and,
/// for mortal arenas, refuses `reset()` while any live typed slice still exists.
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
///
/// Allocation from a `BoundedArena` is monotonic and non-relocating: each successful request
/// carves a new range from the reserved extent, and later requests never move earlier ones.
/// That makes typed results such as [`ArenaSlice`] suitable backing for pin-sensitive storage as
/// long as the returned lease remains live. Mortal arenas additionally refuse [`reset`](Self::reset)
/// while any typed slice lease still exists, so a live lease continues to pin its backing bytes in
/// place instead of letting the arena reuse them behind its back.
pub struct BoundedArena<L: LifetimePolicy = Mortal> {
    control: ManuallyDrop<ControlLease<ArenaControl>>,
    _lifetime: PhantomData<L>,
}

impl<L: LifetimePolicy> fmt::Debug for BoundedArena<L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoundedArena")
            .field("capacity", &self.control().header.payload_len)
            .field("max_align", &self.control().max_align)
            .field("domain", &self.control().domain)
            .field("policy", &self.control().policy)
            .field("lease_id", &self.control().lease_id())
            .field("immortal", &L::IMMORTAL)
            .finish_non_exhaustive()
    }
}

impl<L: LifetimePolicy> BoundedArena<L> {
    pub(super) fn extent_request(
        capacity: usize,
        max_align: usize,
    ) -> Result<super::MemoryPoolExtentRequest, AllocError> {
        if capacity == 0 || max_align == 0 || !max_align.is_power_of_two() {
            return Err(AllocError::invalid_request());
        }
        let layout = front_metadata_layout(
            ControlLease::<ArenaControl>::backing_size(),
            ControlLease::<ArenaControl>::backing_align(),
            capacity,
            max_align,
        )?;
        Ok(super::MemoryPoolExtentRequest {
            len: layout.total_len,
            align: layout.request_align,
        })
    }

    pub(super) fn from_assigned_extent(
        domain: AllocatorDomainId,
        capacity: usize,
        max_align: usize,
        policy: AllocPolicy,
        extent: AssignedPoolExtent,
    ) -> Result<Self, AllocError> {
        if capacity == 0 || max_align == 0 || !max_align.is_power_of_two() {
            return Err(AllocError::invalid_request());
        }
        if !policy.allows(AllocModeSet::ARENA) {
            return Err(AllocError::policy_denied());
        }
        let layout = front_metadata_layout(
            ControlLease::<ArenaControl>::backing_size(),
            ControlLease::<ArenaControl>::backing_align(),
            capacity,
            max_align,
        )?;
        let region = extent.region();
        let usable_base = region
            .base
            .get()
            .checked_add(layout.payload_offset)
            .ok_or_else(AllocError::invalid_request)?;
        if region.len < layout.total_len
            || !usable_base.is_multiple_of(max_align)
            || !region
                .base
                .get()
                .is_multiple_of(ControlLease::<ArenaControl>::backing_align())
        {
            return Err(AllocError::invalid_request());
        }

        let control = ControlLease::new(
            extent,
            ArenaControl {
                header: MetadataPageHeader::new(
                    AllocSubsystemKind::BoundedArena,
                    layout.metadata_len,
                    layout.payload_offset,
                    layout.payload_len,
                ),
                max_align,
                domain,
                policy,
                state: Mutex::new(ArenaState {
                    cursor: 0,
                    live_slices: 0,
                }),
            },
        )?;

        Ok(Self {
            control: ManuallyDrop::new(control),
            _lifetime: PhantomData,
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
    pub fn capacity(&self) -> usize {
        self.control().header.payload_len
    }

    /// Returns the arena policy.
    #[must_use]
    pub fn policy(&self) -> AllocPolicy {
        self.control().policy
    }

    /// Returns the owning allocator domain.
    #[must_use]
    pub fn domain(&self) -> AllocatorDomainId {
        self.control().domain
    }

    const fn control(&self) -> &ControlLease<ArenaControl> {
        // SAFETY: the arena owns the control lease and only leaks it intentionally for immortal
        // typestates.
        unsafe { &*((&raw const self.control).cast::<ControlLease<ArenaControl>>()) }
    }

    fn payload_base(&self) -> Result<usize, AllocError> {
        (self.control().region().base.get())
            .checked_add(self.control().header.payload_offset)
            .ok_or_else(AllocError::invalid_request)
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
        if request.align > self.control().max_align {
            return Err(AllocError::invalid_request());
        }
        let base = self.payload_base()?;
        let mut state = self
            .control()
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
        if end > self.control().header.payload_len {
            return Err(AllocError::capacity_exhausted());
        }

        let control = self.control().try_clone()?;
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
    pub fn alloc_array_with<T, F>(
        &self,
        len: usize,
        mut init: F,
    ) -> Result<ArenaSlice<T>, AllocError>
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
        if request.align > self.control().max_align {
            return Err(ArenaInitError::Alloc(AllocError::invalid_request()));
        }
        let base = self.payload_base().map_err(ArenaInitError::Alloc)?;
        let mut state = self
            .control()
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
        if end > self.control().header.payload_len {
            return Err(ArenaInitError::Alloc(AllocError::capacity_exhausted()));
        }

        let control = self.control().try_clone().map_err(ArenaInitError::Alloc)?;
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

impl BoundedArena<Mortal> {
    /// Resets the arena cursor to the beginning of the reserved extent.
    ///
    /// # Errors
    ///
    /// Returns an error when the arena still has live typed leases or cannot synchronize its
    /// cursor state honestly.
    pub fn reset(&self) -> Result<(), AllocError> {
        let mut state = self
            .control()
            .state
            .lock()
            .map_err(|error| AllocError::synchronization(error.kind))?;
        if state.live_slices != 0 {
            return Err(AllocError::busy());
        }
        state.cursor = 0;
        Ok(())
    }
}

impl BoundedArena<Immortal> {
    /// Allocates one process-lifetime typed value from this immortal arena.
    ///
    /// The returned handle never drops `value`; the bytes remain permanently occupied until
    /// process teardown.
    ///
    /// # Errors
    ///
    /// Returns an error when the typed request cannot be represented honestly or the arena is full.
    pub fn alloc_retained_value<T: 'static>(
        &self,
        value: T,
    ) -> Result<RetainedHandle<T>, AllocError> {
        let request = typed_request::<T>(1)?;
        if request.align > self.control().max_align {
            return Err(AllocError::invalid_request());
        }

        let base = self.payload_base()?;
        let mut state = self
            .control()
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
        if end > self.control().header.payload_len {
            return Err(AllocError::capacity_exhausted());
        }

        state.cursor = end;
        let ptr = NonNull::new(start as *mut T).ok_or_else(AllocError::invalid_request)?;
        // SAFETY: the typed request reserved one aligned `T` slot in immortal backing.
        unsafe {
            ptr.as_ptr().write(value);
        }
        drop(state);
        Ok(RetainedHandle::from_nonnull(ptr))
    }
}

impl<L: LifetimePolicy> BoundedArena<L> {
    const fn control_for_drop(&mut self) -> &mut ManuallyDrop<ControlLease<ArenaControl>> {
        &mut self.control
    }
}

impl<L: LifetimePolicy> Drop for BoundedArena<L> {
    fn drop(&mut self) {
        if !L::IMMORTAL {
            // SAFETY: mortal arenas own their control lease and must release it on drop.
            unsafe {
                ManuallyDrop::drop(self.control_for_drop());
            }
        }
    }
}
impl<L: LifetimePolicy> AllocationStrategy for BoundedArena<L> {
    fn policy(&self) -> AllocPolicy {
        self.control().policy
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
        if request.align > self.control().max_align {
            return Err(AllocError::invalid_request());
        }

        let base = self.payload_base()?;
        let mut state = self
            .control()
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
        if end > self.control().header.payload_len {
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
            self.control().member().compatibility.domain,
            self.control().member().compatibility.attrs,
            self.control().member().compatibility.hazards,
            self.control().member().compatibility.geometry,
            AllocationBacking::ArenaBlock {
                pool_marker: self.control().pool_marker(),
                lease_id: self.control().lease_id(),
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
        if pool_marker != self.control().pool_marker() || lease_id != self.control().lease_id() {
            return Err(AllocError::invalid_request());
        }

        let mut state = self
            .control()
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
