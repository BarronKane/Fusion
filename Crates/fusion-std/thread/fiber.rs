//! Domain 2: public green-thread and fiber orchestration surface.

use core::mem::{MaybeUninit, align_of, size_of};
use core::num::NonZeroUsize;

use std::boxed::Box;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::vec::Vec;

use crate::sync::{Mutex as SyncMutex, Semaphore, SyncError, SyncErrorKind};
use fusion_pal::sys::mem::{
    Backing, CachePolicy, MapFlags, MapRequest, MemBase, MemMap, MemProtect, Placement, Protect,
    Region, RegionAttrs, system_mem,
};
use fusion_sys::fiber::{
    ContextCaps, ContextStackDirection, Fiber, FiberError, FiberReturn, FiberStack, FiberSupport,
    FiberSystem, FiberYield, yield_now as system_yield_now,
};

use super::ThreadPool;

const INLINE_GREEN_JOB_BYTES: usize = 256;

/// Scheduling policy for green threads on top of carrier workers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GreenScheduling {
    /// Simple FIFO scheduling across carriers.
    Fifo,
    /// Priority-aware scheduling across carriers.
    Priority,
    /// Per-carrier deque scheduling with work stealing.
    WorkStealing,
}

/// Growth policy for the green-thread pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GreenGrowth {
    /// Fixed-capacity pool with explicit admission control.
    Fixed,
    /// Grow green-thread population on demand up to the configured cap.
    OnDemand,
}

/// Public green-thread pool configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GreenPoolConfig {
    /// Per-green-thread stack size.
    pub stack_size: NonZeroUsize,
    /// Guard size for each green-thread stack.
    pub guard_bytes: usize,
    /// Maximum live green threads admitted by the pool.
    pub max_green_threads: usize,
    /// Scheduling policy across carriers.
    pub scheduling: GreenScheduling,
    /// Population growth policy.
    pub growth: GreenGrowth,
}

impl GreenPoolConfig {
    /// Returns a small default green-pool configuration.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            stack_size: unsafe { NonZeroUsize::new_unchecked(64 * 1024) },
            guard_bytes: 0,
            max_green_threads: 64,
            scheduling: GreenScheduling::Fifo,
            growth: GreenGrowth::OnDemand,
        }
    }
}

impl Default for GreenPoolConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GreenTaskState {
    Queued,
    Running,
    Yielded,
    Completed,
    Failed(FiberError),
}

const fn is_terminal_task_state(state: GreenTaskState) -> bool {
    matches!(state, GreenTaskState::Completed | GreenTaskState::Failed(_))
}

#[derive(Debug)]
struct FixedIndexStack {
    entries: Box<[usize]>,
    len: usize,
}

impl FixedIndexStack {
    fn new(capacity: usize) -> Self {
        let entries = (0..capacity).collect::<Vec<_>>().into_boxed_slice();
        Self {
            len: entries.len(),
            entries,
        }
    }

    fn pop(&mut self) -> Option<usize> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        Some(self.entries[self.len])
    }

    fn push(&mut self, value: usize) -> Result<(), FiberError> {
        if self.len == self.entries.len() {
            return Err(FiberError::state_conflict());
        }
        self.entries[self.len] = value;
        self.len += 1;
        Ok(())
    }
}

#[derive(Debug)]
struct FixedIndexQueue {
    entries: Box<[usize]>,
    head: usize,
    tail: usize,
    len: usize,
}

impl FixedIndexQueue {
    fn new(capacity: usize) -> Result<Self, FiberError> {
        if capacity == 0 {
            return Err(FiberError::invalid());
        }
        Ok(Self {
            entries: vec![0_usize; capacity].into_boxed_slice(),
            head: 0,
            tail: 0,
            len: 0,
        })
    }

    fn enqueue(&mut self, value: usize) -> Result<(), FiberError> {
        if self.len == self.entries.len() {
            return Err(FiberError::resource_exhausted());
        }
        self.entries[self.tail] = value;
        self.tail = (self.tail + 1) % self.entries.len();
        self.len += 1;
        Ok(())
    }

    fn dequeue(&mut self) -> Option<usize> {
        if self.len == 0 {
            return None;
        }
        let value = self.entries[self.head];
        self.head = (self.head + 1) % self.entries.len();
        self.len -= 1;
        Some(value)
    }
}

#[repr(C, align(64))]
struct InlineGreenJobBytes {
    bytes: [u8; INLINE_GREEN_JOB_BYTES],
}

struct InlineGreenJobStorage {
    storage: MaybeUninit<InlineGreenJobBytes>,
    run: Option<unsafe fn(*mut u8)>,
    drop: Option<unsafe fn(*mut u8)>,
    occupied: bool,
}

impl fmt::Debug for InlineGreenJobStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InlineGreenJobStorage")
            .field("occupied", &self.occupied)
            .finish_non_exhaustive()
    }
}

impl InlineGreenJobStorage {
    const fn empty() -> Self {
        Self {
            storage: MaybeUninit::uninit(),
            run: None,
            drop: None,
            occupied: false,
        }
    }

    fn store<F>(&mut self, job: F) -> Result<(), FiberError>
    where
        F: FnOnce() + Send + 'static,
    {
        if self.occupied {
            return Err(FiberError::state_conflict());
        }
        if size_of::<F>() > size_of::<InlineGreenJobBytes>()
            || align_of::<F>() > align_of::<InlineGreenJobBytes>()
        {
            return Err(FiberError::unsupported());
        }

        unsafe {
            self.storage.as_mut_ptr().cast::<F>().write(job);
        }
        self.run = Some(run_inline_green_job::<F>);
        self.drop = Some(drop_inline_green_job::<F>);
        self.occupied = true;
        Ok(())
    }

    fn take_runner(&mut self) -> Result<InlineGreenJobRunner, FiberError> {
        if !self.occupied {
            return Err(FiberError::state_conflict());
        }
        let run = self.run.take().ok_or_else(FiberError::state_conflict)?;
        self.drop = None;
        self.occupied = false;
        Ok(InlineGreenJobRunner {
            ptr: self.storage.as_mut_ptr().cast::<u8>(),
            run,
        })
    }

    fn clear(&mut self) {
        if !self.occupied {
            self.run = None;
            self.drop = None;
            return;
        }

        if let Some(drop) = self.drop.take() {
            unsafe {
                drop(self.storage.as_mut_ptr().cast::<u8>());
            }
        }
        self.run = None;
        self.occupied = false;
    }
}

impl Drop for InlineGreenJobStorage {
    fn drop(&mut self) {
        self.clear();
    }
}

struct InlineGreenJobRunner {
    ptr: *mut u8,
    run: unsafe fn(*mut u8),
}

impl InlineGreenJobRunner {
    fn run(self) {
        unsafe {
            (self.run)(self.ptr);
        }
    }
}

unsafe fn run_inline_green_job<F>(ptr: *mut u8)
where
    F: FnOnce(),
{
    unsafe {
        ptr.cast::<F>().read()();
    }
}

unsafe fn drop_inline_green_job<F>(ptr: *mut u8) {
    unsafe {
        ptr.cast::<F>().drop_in_place();
    }
}

#[derive(Debug)]
struct FiberStackLease {
    slot_index: usize,
    stack: FiberStack,
}

#[derive(Debug)]
struct FiberStackSlab {
    region: Region,
    stack_size: usize,
    guard_size: usize,
    slot_stride: usize,
    capacity: usize,
    stack_direction: ContextStackDirection,
    free: SyncMutex<FixedIndexStack>,
}

// SAFETY: the mapped region is immutable after construction and the free-list bookkeeping is
// serialized through `free`.
unsafe impl Send for FiberStackSlab {}
// SAFETY: the mapped region is immutable after construction and the free-list bookkeeping is
// serialized through `free`.
unsafe impl Sync for FiberStackSlab {}

impl FiberStackSlab {
    fn new(
        stack_size: usize,
        guard_bytes: usize,
        count: usize,
        alignment: usize,
        stack_direction: ContextStackDirection,
    ) -> Result<Self, FiberError> {
        if stack_size == 0 || count == 0 || alignment == 0 || !alignment.is_power_of_two() {
            return Err(FiberError::invalid());
        }
        if guard_bytes != 0 && matches!(stack_direction, ContextStackDirection::Unknown) {
            return Err(FiberError::unsupported());
        }

        let memory = system_mem();
        let page = memory.page_info().alloc_granule.get();
        let usable_alignment = alignment.max(page);
        let rounded_stack = stack_size
            .checked_next_multiple_of(usable_alignment)
            .ok_or_else(FiberError::resource_exhausted)?;
        let rounded_guard = if guard_bytes == 0 {
            0
        } else {
            guard_bytes
                .checked_next_multiple_of(page)
                .ok_or_else(FiberError::resource_exhausted)?
        };
        let slot_stride = rounded_stack
            .checked_add(rounded_guard)
            .ok_or_else(FiberError::resource_exhausted)?;
        let total = slot_stride
            .checked_mul(count)
            .ok_or_else(FiberError::resource_exhausted)?;

        let region = unsafe {
            memory.map(&MapRequest {
                len: total,
                align: page,
                protect: Protect::READ.union(Protect::WRITE),
                flags: MapFlags::PRIVATE,
                attrs: RegionAttrs::VIRTUAL_ONLY,
                cache: CachePolicy::Default,
                placement: Placement::Anywhere,
                backing: Backing::Anonymous,
            })
        }
        .map_err(fiber_error_from_mem)?;

        let slab = Self {
            region,
            stack_size: rounded_stack,
            guard_size: rounded_guard,
            slot_stride,
            capacity: count,
            stack_direction,
            free: SyncMutex::new(FixedIndexStack::new(count)),
        };

        if rounded_guard != 0 {
            for slot_index in 0..count {
                let guard = slab.guard_region_for(slot_index)?;
                unsafe { memory.protect(guard, Protect::NONE) }.map_err(fiber_error_from_mem)?;
            }
        }

        Ok(slab)
    }

    fn guard_region_for(&self, slot_index: usize) -> Result<Region, FiberError> {
        if self.guard_size == 0 {
            return Err(FiberError::invalid());
        }

        let slot = self.slot_region(slot_index)?;
        match self.stack_direction {
            ContextStackDirection::Down => slot
                .subrange(0, self.guard_size)
                .map_err(fiber_error_from_mem),
            ContextStackDirection::Up => slot
                .subrange(self.stack_size, self.guard_size)
                .map_err(fiber_error_from_mem),
            ContextStackDirection::Unknown => Err(FiberError::unsupported()),
        }
    }

    fn slot_region(&self, slot_index: usize) -> Result<Region, FiberError> {
        self.region
            .subrange(slot_index * self.slot_stride, self.slot_stride)
            .map_err(fiber_error_from_mem)
    }

    fn acquire(&self) -> Result<FiberStackLease, FiberError> {
        let slot_index = self
            .free
            .lock()
            .map_err(fiber_error_from_sync)?
            .pop()
            .ok_or_else(FiberError::resource_exhausted)?;
        let slot = self.slot_region(slot_index)?;
        let usable = if self.guard_size == 0 {
            slot.subrange(0, self.stack_size)
        } else {
            match self.stack_direction {
                ContextStackDirection::Down => slot.subrange(self.guard_size, self.stack_size),
                ContextStackDirection::Up => slot.subrange(0, self.stack_size),
                ContextStackDirection::Unknown => {
                    Err(fusion_pal::sys::mem::MemError::unsupported())
                }
            }
        }
        .map_err(fiber_error_from_mem)?;

        Ok(FiberStackLease {
            slot_index,
            stack: FiberStack::new(usable.base, usable.len)?,
        })
    }

    fn release(&self, slot_index: usize) -> Result<(), FiberError> {
        self.free
            .lock()
            .map_err(fiber_error_from_sync)?
            .push(slot_index)
    }
}

impl Drop for FiberStackSlab {
    fn drop(&mut self) {
        let _ = unsafe { system_mem().unmap(self.region) };
    }
}

#[derive(Debug)]
struct CarrierQueue {
    queue: SyncMutex<FixedIndexQueue>,
    ready: Semaphore,
}

impl CarrierQueue {
    fn new(capacity: usize) -> Result<Self, FiberError> {
        Ok(Self {
            queue: SyncMutex::new(FixedIndexQueue::new(capacity)?),
            ready: Semaphore::new(
                0,
                u32::try_from(capacity).map_err(|_| FiberError::resource_exhausted())?,
            )
            .map_err(fiber_error_from_sync)?,
        })
    }
}

#[derive(Debug)]
struct GreenTaskRecord {
    allocated: bool,
    id: u64,
    carrier: usize,
    slab_slot: usize,
    fiber: Option<Fiber>,
    job: InlineGreenJobStorage,
    state: GreenTaskState,
}

impl GreenTaskRecord {
    const fn empty() -> Self {
        Self {
            allocated: false,
            id: 0,
            carrier: 0,
            slab_slot: 0,
            fiber: None,
            job: InlineGreenJobStorage::empty(),
            state: GreenTaskState::Completed,
        }
    }
}

#[derive(Debug)]
struct GreenTaskSlot {
    record: SyncMutex<GreenTaskRecord>,
    completed: Semaphore,
    handle_refs: AtomicUsize,
}

impl GreenTaskSlot {
    fn new() -> Result<Self, FiberError> {
        Ok(Self {
            record: SyncMutex::new(GreenTaskRecord::empty()),
            completed: Semaphore::new(0, 1).map_err(fiber_error_from_sync)?,
            handle_refs: AtomicUsize::new(0),
        })
    }

    const fn context_ptr(&self) -> *mut () {
        core::ptr::from_ref(self).cast_mut().cast()
    }

    fn assign<F>(&self, id: u64, carrier: usize, slab_slot: usize, job: F) -> Result<(), FiberError>
    where
        F: FnOnce() + Send + 'static,
    {
        while self
            .completed
            .try_acquire()
            .map_err(fiber_error_from_sync)?
        {}

        let mut record = self.record.lock().map_err(fiber_error_from_sync)?;
        if record.allocated {
            return Err(FiberError::state_conflict());
        }

        record.job.clear();
        record.job.store(job)?;
        record.allocated = true;
        record.id = id;
        record.carrier = carrier;
        record.slab_slot = slab_slot;
        record.fiber = None;
        record.state = GreenTaskState::Queued;
        self.handle_refs.store(1, Ordering::Release);
        Ok(())
    }

    fn clone_handle(&self) {
        self.handle_refs.fetch_add(1, Ordering::AcqRel);
    }

    fn current_id(&self) -> Result<u64, FiberError> {
        let record = self.record.lock().map_err(fiber_error_from_sync)?;
        if !record.allocated {
            return Err(FiberError::state_conflict());
        }
        Ok(record.id)
    }

    const fn matches_id(record: &GreenTaskRecord, id: u64) -> bool {
        record.allocated && record.id == id
    }

    fn install_fiber(&self, id: u64, fiber: Fiber) -> Result<(), FiberError> {
        let mut record = self.record.lock().map_err(fiber_error_from_sync)?;
        if !Self::matches_id(&record, id) {
            return Err(FiberError::state_conflict());
        }
        record.fiber = Some(fiber);
        Ok(())
    }

    fn clear_fiber(&self, id: u64) -> Result<(), FiberError> {
        let mut record = self.record.lock().map_err(fiber_error_from_sync)?;
        if !Self::matches_id(&record, id) {
            return Err(FiberError::state_conflict());
        }
        record.fiber = None;
        Ok(())
    }

    fn slab_slot(&self, id: u64) -> Result<usize, FiberError> {
        let record = self.record.lock().map_err(fiber_error_from_sync)?;
        if !Self::matches_id(&record, id) {
            return Err(FiberError::state_conflict());
        }
        Ok(record.slab_slot)
    }

    fn carrier(&self, id: u64) -> Result<usize, FiberError> {
        let record = self.record.lock().map_err(fiber_error_from_sync)?;
        if !Self::matches_id(&record, id) {
            return Err(FiberError::state_conflict());
        }
        Ok(record.carrier)
    }

    fn state(&self, id: u64) -> Result<GreenTaskState, FiberError> {
        let record = self.record.lock().map_err(fiber_error_from_sync)?;
        if !Self::matches_id(&record, id) {
            return Err(FiberError::state_conflict());
        }
        Ok(record.state)
    }

    fn is_finished(&self, id: u64) -> Result<bool, FiberError> {
        Ok(is_terminal_task_state(self.state(id)?))
    }

    fn join(&self, id: u64) -> Result<(), FiberError> {
        let waited = if self.is_finished(id)? {
            false
        } else {
            self.completed.acquire().map_err(fiber_error_from_sync)?;
            true
        };

        let state = self.state(id)?;
        if waited && is_terminal_task_state(state) {
            self.completed.release(1).map_err(fiber_error_from_sync)?;
        }

        match state {
            GreenTaskState::Completed => Ok(()),
            GreenTaskState::Failed(error) => Err(error),
            GreenTaskState::Queued | GreenTaskState::Running | GreenTaskState::Yielded => {
                Err(FiberError::state_conflict())
            }
        }
    }

    fn set_state(&self, id: u64, state: GreenTaskState) -> Result<(), FiberError> {
        let mut release_completion = false;
        {
            let mut record = self.record.lock().map_err(fiber_error_from_sync)?;
            if !Self::matches_id(&record, id) {
                return Err(FiberError::state_conflict());
            }
            let previous = record.state;
            record.state = state;
            if !is_terminal_task_state(previous) && is_terminal_task_state(state) {
                release_completion = true;
            }
        }
        if release_completion {
            self.completed.release(1).map_err(fiber_error_from_sync)?;
        }
        Ok(())
    }

    fn resume(&self, id: u64) -> Result<FiberYield, FiberError> {
        let mut fiber = {
            let mut record = self.record.lock().map_err(fiber_error_from_sync)?;
            if !Self::matches_id(&record, id) {
                return Err(FiberError::state_conflict());
            }
            record.fiber.take().ok_or_else(FiberError::state_conflict)?
        };

        match fiber.resume() {
            Ok(FiberYield::Yielded) => {
                let mut record = self.record.lock().map_err(fiber_error_from_sync)?;
                if !Self::matches_id(&record, id) {
                    return Err(FiberError::state_conflict());
                }
                record.fiber = Some(fiber);
                Ok(FiberYield::Yielded)
            }
            Ok(FiberYield::Completed(result)) => Ok(FiberYield::Completed(result)),
            Err(error) => Err(error),
        }
    }

    fn take_job_runner(&self, id: u64) -> Result<InlineGreenJobRunner, FiberError> {
        let mut record = self.record.lock().map_err(fiber_error_from_sync)?;
        if !Self::matches_id(&record, id) {
            return Err(FiberError::state_conflict());
        }
        record.job.take_runner()
    }

    fn force_recycle(&self, id: u64) -> Result<bool, FiberError> {
        let mut record = self.record.lock().map_err(fiber_error_from_sync)?;
        if !Self::matches_id(&record, id) {
            return Ok(false);
        }
        record.job.clear();
        record.fiber = None;
        record.allocated = false;
        record.id = 0;
        record.carrier = 0;
        record.slab_slot = 0;
        record.state = GreenTaskState::Completed;
        self.handle_refs.store(0, Ordering::Release);
        Ok(true)
    }

    fn try_recycle(&self, id: u64) -> Result<bool, FiberError> {
        let mut record = self.record.lock().map_err(fiber_error_from_sync)?;
        if !Self::matches_id(&record, id) {
            return Ok(false);
        }
        if !is_terminal_task_state(record.state) || self.handle_refs.load(Ordering::Acquire) != 0 {
            return Ok(false);
        }
        record.job.clear();
        record.fiber = None;
        record.allocated = false;
        record.id = 0;
        record.carrier = 0;
        record.slab_slot = 0;
        record.state = GreenTaskState::Completed;
        Ok(true)
    }
}

#[derive(Debug)]
struct GreenTaskRegistry {
    slots: Box<[GreenTaskSlot]>,
    free: SyncMutex<FixedIndexStack>,
}

impl GreenTaskRegistry {
    fn new(capacity: usize) -> Result<Self, FiberError> {
        if capacity == 0 {
            return Err(FiberError::invalid());
        }

        let mut slots = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            slots.push(GreenTaskSlot::new()?);
        }

        Ok(Self {
            free: SyncMutex::new(FixedIndexStack::new(capacity)),
            slots: slots.into_boxed_slice(),
        })
    }

    fn allocate<F>(
        &self,
        id: u64,
        carrier: usize,
        slab_slot: usize,
        job: F,
    ) -> Result<usize, FiberError>
    where
        F: FnOnce() + Send + 'static,
    {
        let slot_index = self
            .free
            .lock()
            .map_err(fiber_error_from_sync)?
            .pop()
            .ok_or_else(FiberError::resource_exhausted)?;
        let slot = &self.slots[slot_index];
        if let Err(error) = slot.assign(id, carrier, slab_slot, job) {
            self.free
                .lock()
                .map_err(fiber_error_from_sync)?
                .push(slot_index)?;
            return Err(error);
        }
        Ok(slot_index)
    }

    fn recycle_slot(&self, slot_index: usize) -> Result<(), FiberError> {
        self.free
            .lock()
            .map_err(fiber_error_from_sync)?
            .push(slot_index)
    }

    fn slot(&self, slot_index: usize) -> Result<&GreenTaskSlot, FiberError> {
        self.slots.get(slot_index).ok_or_else(FiberError::invalid)
    }

    fn slot_context(&self, slot_index: usize) -> Result<*mut (), FiberError> {
        Ok(self.slot(slot_index)?.context_ptr())
    }

    fn clone_handle(&self, slot_index: usize) -> Result<(), FiberError> {
        self.slot(slot_index)?.clone_handle();
        Ok(())
    }

    fn current_id(&self, slot_index: usize) -> Result<u64, FiberError> {
        self.slot(slot_index)?.current_id()
    }

    fn install_fiber(&self, slot_index: usize, id: u64, fiber: Fiber) -> Result<(), FiberError> {
        self.slot(slot_index)?.install_fiber(id, fiber)
    }

    fn clear_fiber(&self, slot_index: usize, id: u64) -> Result<(), FiberError> {
        self.slot(slot_index)?.clear_fiber(id)
    }

    fn carrier(&self, slot_index: usize, id: u64) -> Result<usize, FiberError> {
        self.slot(slot_index)?.carrier(id)
    }

    fn slab_slot(&self, slot_index: usize, id: u64) -> Result<usize, FiberError> {
        self.slot(slot_index)?.slab_slot(id)
    }

    fn state(&self, slot_index: usize, id: u64) -> Result<GreenTaskState, FiberError> {
        self.slot(slot_index)?.state(id)
    }

    fn is_finished(&self, slot_index: usize, id: u64) -> Result<bool, FiberError> {
        self.slot(slot_index)?.is_finished(id)
    }

    fn join(&self, slot_index: usize, id: u64) -> Result<(), FiberError> {
        self.slot(slot_index)?.join(id)
    }

    fn set_state(
        &self,
        slot_index: usize,
        id: u64,
        state: GreenTaskState,
    ) -> Result<(), FiberError> {
        self.slot(slot_index)?.set_state(id, state)
    }

    fn resume(&self, slot_index: usize, id: u64) -> Result<FiberYield, FiberError> {
        self.slot(slot_index)?.resume(id)
    }

    fn release_handle(&self, slot_index: usize, id: u64) -> Result<(), FiberError> {
        let slot = self.slot(slot_index)?;
        let previous = slot.handle_refs.fetch_sub(1, Ordering::AcqRel);
        if previous == 0 {
            slot.handle_refs.store(0, Ordering::Release);
            return Err(FiberError::state_conflict());
        }
        if previous == 1 && slot.try_recycle(id)? {
            self.recycle_slot(slot_index)?;
        }
        Ok(())
    }

    fn try_reclaim(&self, slot_index: usize, id: u64) -> Result<(), FiberError> {
        if self.slot(slot_index)?.try_recycle(id)? {
            self.recycle_slot(slot_index)?;
        }
        Ok(())
    }

    fn abandon(&self, slot_index: usize, id: u64) -> Result<(), FiberError> {
        if self.slot(slot_index)?.force_recycle(id)? {
            self.recycle_slot(slot_index)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct GreenPoolInner {
    support: FiberSupport,
    shutdown: AtomicBool,
    client_refs: AtomicUsize,
    active: AtomicUsize,
    next_id: AtomicU64,
    next_carrier: AtomicUsize,
    carriers: Box<[CarrierQueue]>,
    tasks: GreenTaskRegistry,
    stack_slab: FiberStackSlab,
}

impl GreenPoolInner {
    fn enqueue(&self, carrier: usize, slot_index: usize) -> Result<(), FiberError> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(FiberError::state_conflict());
        }

        let queue = self.carriers.get(carrier).ok_or_else(FiberError::invalid)?;
        let mut guard = queue.queue.lock().map_err(fiber_error_from_sync)?;
        guard.enqueue(slot_index)?;
        drop(guard);
        queue.ready.release(1).map_err(fiber_error_from_sync)
    }

    fn request_shutdown(&self) -> Result<(), FiberError> {
        if self.shutdown.swap(true, Ordering::AcqRel) {
            return Ok(());
        }

        for carrier in &*self.carriers {
            carrier.ready.release(1).map_err(fiber_error_from_sync)?;
        }
        Ok(())
    }

    fn finish_task(&self, slot_index: usize, id: u64) -> Result<(), FiberError> {
        let mut first_error = None;

        let slab_slot = match self.tasks.slab_slot(slot_index, id) {
            Ok(slab_slot) => Some(slab_slot),
            Err(error) => {
                first_error = Some(error);
                None
            }
        };

        if let Err(error) = self.tasks.clear_fiber(slot_index, id)
            && first_error.is_none()
        {
            first_error = Some(error);
        }

        if let Some(slab_slot) = slab_slot
            && let Err(error) = self.stack_slab.release(slab_slot)
            && first_error.is_none()
        {
            first_error = Some(error);
        }

        self.active.fetch_sub(1, Ordering::AcqRel);

        if let Err(error) = self.tasks.try_reclaim(slot_index, id)
            && first_error.is_none()
        {
            first_error = Some(error);
        }

        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(())
    }
}

/// Opaque public green-thread handle.
#[derive(Debug)]
pub struct GreenHandle {
    id: u64,
    slot_index: usize,
    inner: Arc<GreenPoolInner>,
}

impl GreenHandle {
    /// Returns the stable green-thread identifier.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    /// Returns whether the green thread has completed.
    ///
    /// # Errors
    ///
    /// Returns an error if the green-thread state cannot be observed honestly.
    pub fn is_finished(&self) -> Result<bool, FiberError> {
        self.inner.tasks.is_finished(self.slot_index, self.id)
    }

    /// Waits for the green thread to complete.
    ///
    /// # Errors
    ///
    /// Returns the fiber failure that stopped execution, if any.
    pub fn join(self) -> Result<(), FiberError> {
        self.inner.tasks.join(self.slot_index, self.id)
    }
}

impl Clone for GreenHandle {
    fn clone(&self) -> Self {
        if self.inner.tasks.clone_handle(self.slot_index).is_err() {
            // The underlying task registry should stay live while a handle exists. If it
            // doesn't, the only honest fallback left to `Clone` is to preserve the stale handle
            // shape and let later observation report the state conflict.
        }
        Self {
            id: self.id,
            slot_index: self.slot_index,
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Drop for GreenHandle {
    fn drop(&mut self) {
        let _ = self.inner.tasks.release_handle(self.slot_index, self.id);
    }
}

/// Public green-thread pool wrapper.
#[derive(Debug)]
pub struct GreenPool {
    inner: Arc<GreenPoolInner>,
}

impl GreenPool {
    /// Returns the low-level fiber support available on the current backend.
    #[must_use]
    pub fn support() -> FiberSupport {
        FiberSystem::new().support()
    }

    /// Creates a green-thread pool on top of the supplied carrier pool.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the selected fiber backend cannot support same-carrier
    /// green threads or the configured slab-backed stack pool cannot be realized.
    pub fn new(config: &GreenPoolConfig, carrier: &ThreadPool) -> Result<Self, FiberError> {
        let support = Self::support();
        if !support.context.caps.contains(ContextCaps::MAKE)
            || !support.context.caps.contains(ContextCaps::SWAP)
        {
            return Err(FiberError::unsupported());
        }
        if support.context.guard_required && config.guard_bytes == 0 {
            return Err(FiberError::invalid());
        }

        let carrier_workers = carrier
            .worker_count()
            .map_err(fiber_error_from_thread_pool)?;
        if config.max_green_threads == 0 || carrier_workers == 0 {
            return Err(FiberError::invalid());
        }
        if !matches!(config.scheduling, GreenScheduling::Fifo) {
            return Err(FiberError::unsupported());
        }

        let alignment = support.context.min_stack_alignment.max(16);
        let stack_slab = FiberStackSlab::new(
            config.stack_size.get(),
            config.guard_bytes,
            config.max_green_threads,
            alignment,
            support.context.stack_direction,
        )?;
        let mut carriers = Vec::with_capacity(carrier_workers);
        for _ in 0..carrier_workers {
            carriers.push(CarrierQueue::new(config.max_green_threads)?);
        }
        let carriers = carriers.into_boxed_slice();

        let inner = Arc::new(GreenPoolInner {
            support,
            shutdown: AtomicBool::new(false),
            client_refs: AtomicUsize::new(1),
            active: AtomicUsize::new(0),
            next_id: AtomicU64::new(1),
            next_carrier: AtomicUsize::new(0),
            carriers,
            tasks: GreenTaskRegistry::new(config.max_green_threads)?,
            stack_slab,
        });

        for carrier_index in 0..inner.carriers.len() {
            let inner = Arc::clone(&inner);
            carrier
                .submit(move || {
                    if run_carrier_loop(&inner, carrier_index).is_err() {
                        let _ = inner.request_shutdown();
                    }
                })
                .map_err(fiber_error_from_thread_pool)?;
        }

        Ok(Self { inner })
    }

    /// Returns the currently configured low-level support surface.
    #[must_use]
    pub fn fiber_support(&self) -> FiberSupport {
        self.inner.support
    }

    /// Returns the number of active green threads currently admitted.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.inner.active.load(Ordering::Acquire)
    }

    /// Spawns one green-thread job onto the carrier-backed scheduler.
    ///
    /// # Errors
    ///
    /// Returns an error when the pool is shut down, capacity is exhausted, the inline task
    /// storage cannot contain the submitted closure, or a new fiber cannot be constructed on the
    /// slab-backed stack store.
    pub fn spawn<F>(&self, job: F) -> Result<GreenHandle, FiberError>
    where
        F: FnOnce() + Send + 'static,
    {
        loop {
            let active = self.inner.active.load(Ordering::Acquire);
            if active >= self.inner.stack_slab.capacity {
                return Err(FiberError::resource_exhausted());
            }
            if self
                .inner
                .active
                .compare_exchange(active, active + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                break;
            }
        }

        let lease = match self.inner.stack_slab.acquire() {
            Ok(lease) => lease,
            Err(error) => {
                self.inner.active.fetch_sub(1, Ordering::AcqRel);
                return Err(error);
            }
        };

        let id = self.inner.next_id.fetch_add(1, Ordering::AcqRel);
        let carrier =
            self.inner.next_carrier.fetch_add(1, Ordering::AcqRel) % self.inner.carriers.len();
        let slot_index = match self
            .inner
            .tasks
            .allocate(id, carrier, lease.slot_index, job)
        {
            Ok(slot_index) => slot_index,
            Err(error) => {
                let _ = self.inner.stack_slab.release(lease.slot_index);
                self.inner.active.fetch_sub(1, Ordering::AcqRel);
                return Err(error);
            }
        };

        let context = match self.inner.tasks.slot_context(slot_index) {
            Ok(context) => context,
            Err(error) => {
                let _ = self.inner.tasks.abandon(slot_index, id);
                let _ = self.inner.stack_slab.release(lease.slot_index);
                self.inner.active.fetch_sub(1, Ordering::AcqRel);
                return Err(error);
            }
        };

        let fiber = match Fiber::new(lease.stack, green_task_entry, context) {
            Ok(fiber) => fiber,
            Err(error) => {
                let _ = self.inner.tasks.abandon(slot_index, id);
                let _ = self.inner.stack_slab.release(lease.slot_index);
                self.inner.active.fetch_sub(1, Ordering::AcqRel);
                return Err(error);
            }
        };

        if let Err(error) = self.inner.tasks.install_fiber(slot_index, id, fiber) {
            let _ = self.inner.tasks.abandon(slot_index, id);
            let _ = self.inner.stack_slab.release(lease.slot_index);
            self.inner.active.fetch_sub(1, Ordering::AcqRel);
            return Err(error);
        }

        if let Err(error) = self.inner.enqueue(carrier, slot_index) {
            let _ = self.inner.tasks.abandon(slot_index, id);
            let _ = self.inner.stack_slab.release(lease.slot_index);
            self.inner.active.fetch_sub(1, Ordering::AcqRel);
            return Err(error);
        }

        Ok(GreenHandle {
            id,
            slot_index,
            inner: Arc::clone(&self.inner),
        })
    }

    /// Requests scheduler shutdown and wakes every carrier loop.
    ///
    /// # Errors
    ///
    /// Returns an error if the wakeup path cannot be signaled honestly.
    pub fn shutdown(&self) -> Result<(), FiberError> {
        self.inner.request_shutdown()
    }
}

impl Clone for GreenPool {
    fn clone(&self) -> Self {
        self.inner.client_refs.fetch_add(1, Ordering::AcqRel);
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Drop for GreenPool {
    fn drop(&mut self) {
        if self.inner.client_refs.fetch_sub(1, Ordering::AcqRel) == 1 {
            let _ = self.inner.request_shutdown();
        }
    }
}

unsafe fn green_task_entry(context: *mut ()) -> FiberReturn {
    let slot = unsafe { &*context.cast::<GreenTaskSlot>() };
    let Ok(id) = slot.current_id() else {
        return FiberReturn::new(usize::MAX);
    };

    let runner = match slot.take_job_runner(id) {
        Ok(runner) => runner,
        Err(error) => {
            let _ = slot.set_state(id, GreenTaskState::Failed(error));
            return FiberReturn::new(usize::MAX);
        }
    };

    if catch_unwind(AssertUnwindSafe(|| runner.run())).is_err() {
        let _ = slot.set_state(id, GreenTaskState::Failed(FiberError::state_conflict()));
        return FiberReturn::new(usize::MAX);
    }

    FiberReturn::new(0)
}

fn run_carrier_loop(inner: &Arc<GreenPoolInner>, carrier_index: usize) -> Result<(), FiberError> {
    loop {
        let carrier = &inner.carriers[carrier_index];
        carrier.ready.acquire().map_err(fiber_error_from_sync)?;
        let slot_index = {
            let mut queue = carrier.queue.lock().map_err(fiber_error_from_sync)?;
            queue.dequeue()
        };

        let Some(slot_index) = slot_index else {
            if inner.shutdown.load(Ordering::Acquire) {
                break;
            }
            continue;
        };

        let task_id = inner.tasks.current_id(slot_index)?;
        inner
            .tasks
            .set_state(slot_index, task_id, GreenTaskState::Running)?;
        let resume = inner.tasks.resume(slot_index, task_id);

        match resume {
            Ok(FiberYield::Yielded) => {
                inner
                    .tasks
                    .set_state(slot_index, task_id, GreenTaskState::Yielded)?;
                let carrier_index = inner.tasks.carrier(slot_index, task_id)?;
                if let Err(error) = inner.enqueue(carrier_index, slot_index) {
                    let _ =
                        inner
                            .tasks
                            .set_state(slot_index, task_id, GreenTaskState::Failed(error));
                    let _ = inner.finish_task(slot_index, task_id);
                }
            }
            Ok(FiberYield::Completed(_)) => {
                if !matches!(
                    inner.tasks.state(slot_index, task_id)?,
                    GreenTaskState::Failed(_)
                ) {
                    inner
                        .tasks
                        .set_state(slot_index, task_id, GreenTaskState::Completed)?;
                }
                inner.finish_task(slot_index, task_id)?;
            }
            Err(error) => {
                inner
                    .tasks
                    .set_state(slot_index, task_id, GreenTaskState::Failed(error))?;
                inner.finish_task(slot_index, task_id)?;
            }
        }
    }
    Ok(())
}

/// Yields the current green thread cooperatively.
///
/// # Errors
///
/// Returns an honest error when no active green fiber exists on the current carrier.
pub fn yield_now() -> Result<(), FiberError> {
    system_yield_now()
}

const fn fiber_error_from_thread_pool(error: super::ThreadPoolError) -> FiberError {
    match error.kind() {
        fusion_sys::thread::ThreadErrorKind::Unsupported => FiberError::unsupported(),
        fusion_sys::thread::ThreadErrorKind::ResourceExhausted => FiberError::resource_exhausted(),
        fusion_sys::thread::ThreadErrorKind::Busy
        | fusion_sys::thread::ThreadErrorKind::Timeout
        | fusion_sys::thread::ThreadErrorKind::StateConflict => FiberError::state_conflict(),
        fusion_sys::thread::ThreadErrorKind::Invalid
        | fusion_sys::thread::ThreadErrorKind::PermissionDenied
        | fusion_sys::thread::ThreadErrorKind::PlacementDenied
        | fusion_sys::thread::ThreadErrorKind::SchedulerDenied
        | fusion_sys::thread::ThreadErrorKind::StackDenied
        | fusion_sys::thread::ThreadErrorKind::Platform(_) => FiberError::invalid(),
    }
}

const fn fiber_error_from_sync(error: SyncError) -> FiberError {
    match error.kind {
        SyncErrorKind::Unsupported => FiberError::unsupported(),
        SyncErrorKind::Invalid | SyncErrorKind::Overflow => FiberError::invalid(),
        SyncErrorKind::Busy | SyncErrorKind::PermissionDenied | SyncErrorKind::Platform(_) => {
            FiberError::state_conflict()
        }
    }
}

const fn fiber_error_from_mem(error: fusion_pal::sys::mem::MemError) -> FiberError {
    match error.kind {
        fusion_pal::sys::mem::MemErrorKind::Unsupported => FiberError::unsupported(),
        fusion_pal::sys::mem::MemErrorKind::InvalidInput
        | fusion_pal::sys::mem::MemErrorKind::InvalidAddress
        | fusion_pal::sys::mem::MemErrorKind::Misaligned
        | fusion_pal::sys::mem::MemErrorKind::OutOfBounds
        | fusion_pal::sys::mem::MemErrorKind::PermissionDenied
        | fusion_pal::sys::mem::MemErrorKind::Overflow => FiberError::invalid(),
        fusion_pal::sys::mem::MemErrorKind::OutOfMemory => FiberError::resource_exhausted(),
        fusion_pal::sys::mem::MemErrorKind::Busy
        | fusion_pal::sys::mem::MemErrorKind::Platform(_) => FiberError::state_conflict(),
    }
}
