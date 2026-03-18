//! Domain 2: public green-thread and fiber orchestration surface.

use core::any::TypeId;
use core::fmt;
use core::marker::PhantomData;
use core::mem::{ManuallyDrop, MaybeUninit, align_of, size_of};
use core::num::NonZeroUsize;
use core::ops::{Deref, DerefMut};
use core::ptr::{self, NonNull, addr_of_mut};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use core::time::Duration;

use crate::sync::{
    Mutex as SyncMutex, OnceLock, Semaphore, SharedHeader, SharedRelease, SyncError, SyncErrorKind,
};
use fusion_pal::sys::fiber::{
    FiberHostError, FiberHostErrorKind, PlatformFiberSignalStack, PlatformFiberWakeSignal,
    PlatformWakeToken, system_fiber_host,
};
use fusion_pal::sys::mem::{
    Advise, Backing, CachePolicy, MapFlags, MapRequest, MemAdviceCaps, MemAdvise, MemBase, MemMap,
    MemProtect, Placement, Protect, Region, RegionAttrs, system_mem,
};
use fusion_sys::event::{
    EventCaps, EventInterest, EventKey, EventNotification, EventPoller, EventReadiness,
    EventRecord, EventSourceHandle, EventSystem,
};
use fusion_sys::fiber::{
    ContextCaps, ContextMigrationSupport, ContextStackDirection, Fiber, FiberError, FiberReturn,
    FiberStack, FiberSupport, FiberSystem, FiberYield, current_context as system_fiber_context,
    yield_now as system_yield_now,
};

use super::ThreadPool;
#[cfg(feature = "std")]
use super::{PoolPlacement, ThreadPoolConfig};
#[cfg(feature = "std")]
use fusion_pal::hal::{HardwareTopologyQuery as _, system_hardware};

const INLINE_GREEN_JOB_BYTES: usize = 256;
const INLINE_GREEN_RESULT_BYTES: usize = 256;
const CARRIER_EVENT_BATCH: usize = 64;
const STEAL_SEED_MIX: u64 = 0x9e37_79b9_7f4a_7c15;

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

/// Signal-path stack telemetry policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FiberTelemetry {
    /// No per-fiber growth counters.
    Disabled,
    /// Count growth events only.
    GrowthCount,
    /// Count growth events and track committed-page high-water marks.
    Full,
}

/// Response policy when an elastic fiber stack reaches its reservation ceiling.
#[derive(Debug, Clone, Copy)]
pub enum CapacityPolicy {
    /// Hard-fault semantics only. No advisory callback.
    Abort,
    /// Invoke the callback after the running fiber yields or completes.
    Notify(fn(FiberCapacityEvent)),
}

impl PartialEq for CapacityPolicy {
    fn eq(&self, other: &Self) -> bool {
        match (*self, *other) {
            (Self::Abort, Self::Abort) => true,
            (Self::Notify(lhs), Self::Notify(rhs)) => core::ptr::fn_addr_eq(lhs, rhs),
            _ => false,
        }
    }
}

impl Eq for CapacityPolicy {}

impl core::hash::Hash for CapacityPolicy {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        match self {
            Self::Abort => core::hash::Hash::hash(&0_u8, state),
            Self::Notify(_) => core::hash::Hash::hash(&1_u8, state),
        }
    }
}

/// Advisory event emitted when a fiber reaches stack capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberCapacityEvent {
    /// Stable fiber identifier.
    pub fiber_id: u64,
    /// Carrier worker that was executing the fiber.
    pub carrier_id: usize,
    /// Currently committed usable pages.
    pub committed_pages: u32,
    /// Maximum usable pages allowed by the reservation.
    pub reservation_pages: u32,
}

/// Approximate pool-level stack telemetry snapshot.
#[derive(Debug, PartialEq, Eq)]
pub struct FiberStackStats {
    /// Total growth events across live fibers in the pool.
    pub total_growth_events: u64,
    /// Maximum committed-page count observed across live fibers.
    pub peak_committed_pages: u32,
    /// Distribution of live fibers by committed-page count.
    pub committed_distribution: FiberStackDistribution,
    /// Number of live fibers currently at reservation capacity.
    pub at_capacity_count: usize,
}

/// Huge-page preference for large fiber stack reservations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HugePagePolicy {
    /// Small-page treatment only.
    Disabled,
    /// Prefer huge-page treatment for large reservations when the backend supports advice.
    Enabled {
        /// Target huge-page granule used as the advisory threshold.
        size: HugePageSize,
    },
}

/// Huge-page granule used for advisory thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HugePageSize {
    /// 2 MiB huge pages.
    TwoMiB,
    /// 1 GiB huge pages.
    OneGiB,
}

impl HugePageSize {
    const fn bytes(self) -> usize {
        match self {
            Self::TwoMiB => 2 * 1024 * 1024,
            Self::OneGiB => 1024 * 1024 * 1024,
        }
    }
}

/// Stack-backing strategy for one fiber reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FiberStackBacking {
    /// Fully committed fixed-capacity stacks with hardware guard pages only.
    Fixed {
        /// Total usable stack size per fiber.
        stack_size: NonZeroUsize,
    },
    /// Reservation-backed elastic stacks with MMU-driven page promotion.
    Elastic {
        /// Initially committed usable bytes at fiber creation.
        initial_size: NonZeroUsize,
        /// Maximum usable bytes the fiber may grow to.
        max_size: NonZeroUsize,
    },
}

/// Public fiber-pool configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberPoolConfig {
    /// Stack backing and growth model.
    pub stack_backing: FiberStackBacking,
    /// Hardware guard pages per fiber.
    pub guard_pages: usize,
    /// Number of fiber reservations committed together when the pool grows.
    pub growth_chunk: usize,
    /// Maximum live fibers admitted per carrier worker.
    pub max_fibers_per_carrier: usize,
    /// Scheduling policy across carriers.
    pub scheduling: GreenScheduling,
    /// Pool population growth policy.
    pub growth: GreenGrowth,
    /// Signal-path stack telemetry policy.
    pub telemetry: FiberTelemetry,
    /// Action to take when an elastic stack reaches capacity.
    pub capacity_policy: CapacityPolicy,
    /// Huge-page preference for large reservations.
    pub huge_pages: HugePagePolicy,
}

impl FiberPoolConfig {
    /// Returns an automatic hosted default.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            stack_backing: FiberStackBacking::Elastic {
                initial_size: unsafe { NonZeroUsize::new_unchecked(4 * 1024) },
                max_size: unsafe { NonZeroUsize::new_unchecked(256 * 1024) },
            },
            guard_pages: 1,
            growth_chunk: 32,
            max_fibers_per_carrier: 64,
            scheduling: GreenScheduling::Fifo,
            growth: GreenGrowth::OnDemand,
            telemetry: FiberTelemetry::Disabled,
            capacity_policy: CapacityPolicy::Abort,
            huge_pages: HugePagePolicy::Disabled,
        }
    }

    /// Returns an explicit fixed-capacity deterministic configuration.
    #[must_use]
    pub const fn fixed(stack_size: NonZeroUsize, max_fibers_per_carrier: usize) -> Self {
        Self {
            stack_backing: FiberStackBacking::Fixed { stack_size },
            guard_pages: 1,
            growth_chunk: max_fibers_per_carrier,
            max_fibers_per_carrier,
            scheduling: GreenScheduling::Fifo,
            growth: GreenGrowth::Fixed,
            telemetry: FiberTelemetry::Disabled,
            capacity_policy: CapacityPolicy::Abort,
            huge_pages: HugePagePolicy::Disabled,
        }
    }
}

impl Default for FiberPoolConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Backward-compatible alias for the older green-pool naming.
pub type GreenPoolConfig = FiberPoolConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GreenTaskState {
    Queued,
    Running,
    Yielded,
    Waiting,
    Completed,
    Failed(FiberError),
}

const fn is_terminal_task_state(state: GreenTaskState) -> bool {
    matches!(state, GreenTaskState::Completed | GreenTaskState::Failed(_))
}

const EMPTY_EVENT_RECORD: EventRecord = EventRecord {
    key: EventKey(0),
    notification: EventNotification::Readiness(EventReadiness::empty()),
};

struct MetadataSlice<T> {
    ptr: core::ptr::NonNull<T>,
    len: usize,
}

impl<T> Copy for MetadataSlice<T> {}

impl<T> Clone for MetadataSlice<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> MetadataSlice<T> {
    const fn empty() -> Self {
        Self {
            ptr: core::ptr::NonNull::dangling(),
            len: 0,
        }
    }

    const fn len(&self) -> usize {
        self.len
    }

    const fn is_empty(&self) -> bool {
        self.len == 0
    }

    unsafe fn write(&self, index: usize, value: T) -> Result<(), FiberError> {
        if index >= self.len {
            return Err(FiberError::invalid());
        }
        // SAFETY: the metadata slice owner reserved a contiguous region for `len` elements and is
        // responsible for initialization discipline before exposing shared references.
        unsafe {
            self.ptr.as_ptr().add(index).write(value);
        }
        Ok(())
    }

    const fn as_slice(&self) -> &[T] {
        // SAFETY: callers construct `MetadataSlice<T>` only after reserving enough contiguous
        // space, and all public readers are used only after initialization is complete.
        unsafe { core::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    const fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: the owner provides unique mutable access before any aliasing references are
        // handed out.
        unsafe { core::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }

    fn get(&self, index: usize) -> Option<&T> {
        self.as_slice().get(index)
    }
}

impl<T> fmt::Debug for MetadataSlice<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MetadataSlice")
            .field("ptr", &self.ptr)
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}

impl<T> Deref for MetadataSlice<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<T> DerefMut for MetadataSlice<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

// SAFETY: `MetadataSlice<T>` is just a pointer/length view over allocator-owned memory. Sending
// or sharing it is sound when the underlying element type already satisfies the corresponding
// thread-safety contract.
unsafe impl<T: Send> Send for MetadataSlice<T> {}
// SAFETY: see above.
unsafe impl<T: Sync> Sync for MetadataSlice<T> {}

struct MappedVec<T> {
    region: Option<Region>,
    ptr: core::ptr::NonNull<T>,
    len: usize,
    capacity: usize,
}

impl<T: Copy> MappedVec<T> {
    const fn new() -> Self {
        Self {
            region: None,
            ptr: core::ptr::NonNull::dangling(),
            len: 0,
            capacity: 0,
        }
    }

    const fn len(&self) -> usize {
        self.len
    }

    const fn is_empty(&self) -> bool {
        self.len == 0
    }

    const fn as_slice(&self) -> &[T] {
        // SAFETY: `ptr` references `len` initialized elements while the owned mapping stays live.
        unsafe { core::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    const fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: the owned mapping stays live and mutable access is unique through `&mut self`.
        unsafe { core::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }

    fn truncate(&mut self, len: usize) {
        self.len = self.len.min(len);
    }

    fn grow_for(&mut self, min_capacity: usize) -> Result<(), FiberError> {
        let mut target = self.capacity.max(4);
        while target < min_capacity {
            target = target
                .checked_mul(2)
                .ok_or_else(FiberError::resource_exhausted)?;
        }

        let mut next = Self::with_capacity(target)?;
        for item in self.as_slice() {
            next.push_copy(item)?;
        }
        *self = next;
        Ok(())
    }

    fn with_capacity(capacity: usize) -> Result<Self, FiberError> {
        if capacity == 0 {
            return Ok(Self::new());
        }
        if size_of::<T>() == 0 {
            return Err(FiberError::unsupported());
        }

        let memory = system_mem();
        let page = memory.page_info().alloc_granule.get();
        let align = page.max(align_of::<T>());
        let bytes = size_of::<T>()
            .checked_mul(capacity)
            .ok_or_else(FiberError::resource_exhausted)?;
        let len = fiber_align_up(bytes, page)?;
        let region = unsafe {
            memory.map(&MapRequest {
                len,
                align,
                protect: Protect::NONE,
                flags: MapFlags::PRIVATE,
                attrs: RegionAttrs::VIRTUAL_ONLY,
                cache: CachePolicy::Default,
                placement: Placement::Anywhere,
                backing: Backing::Anonymous,
            })
        }
        .map_err(fiber_error_from_mem)?;
        unsafe { memory.protect(region, Protect::READ | Protect::WRITE) }
            .map_err(fiber_error_from_mem)?;

        Ok(Self {
            region: Some(region),
            ptr: region.base.cast::<T>(),
            len: 0,
            capacity,
        })
    }

    fn push_copy(&mut self, value: &T) -> Result<(), FiberError> {
        if self.len == self.capacity {
            self.grow_for(
                self.len
                    .checked_add(1)
                    .ok_or_else(FiberError::resource_exhausted)?,
            )?;
        }
        // SAFETY: growth above guarantees spare initialized storage for exactly one `T`.
        unsafe {
            self.ptr.as_ptr().add(self.len).write(*value);
        }
        self.len += 1;
        Ok(())
    }

    fn push(&mut self, value: T) -> Result<(), FiberError> {
        if self.len == self.capacity {
            self.grow_for(
                self.len
                    .checked_add(1)
                    .ok_or_else(FiberError::resource_exhausted)?,
            )?;
        }
        // SAFETY: growth above guarantees spare initialized storage for exactly one `T`.
        unsafe {
            self.ptr.as_ptr().add(self.len).write(value);
        }
        self.len += 1;
        Ok(())
    }

    fn retain<F>(&mut self, mut keep: F)
    where
        F: FnMut(&T) -> bool,
    {
        let mut write = 0;
        for read in 0..self.len {
            let value = self.as_slice()[read];
            if keep(&value) {
                // SAFETY: `write <= read < len` always addresses initialized storage.
                unsafe {
                    self.ptr.as_ptr().add(write).write(value);
                }
                write += 1;
            }
        }
        self.len = write;
    }

    fn sort_by_key<K, F>(&mut self, mut f: F)
    where
        K: Ord,
        F: FnMut(&T) -> K,
    {
        let slice = self.as_mut_slice();
        for i in 1..slice.len() {
            let key = f(&slice[i]);
            let value = slice[i];
            let mut j = i;
            while j > 0 && f(&slice[j - 1]) > key {
                slice[j] = slice[j - 1];
                j -= 1;
            }
            slice[j] = value;
        }
    }
}

impl<T> Drop for MappedVec<T> {
    fn drop(&mut self) {
        if let Some(region) = self.region.take() {
            let _ = unsafe { system_mem().unmap(region) };
        }
    }
}

impl<T: Copy> Deref for MappedVec<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<T: Copy> DerefMut for MappedVec<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl<T: Copy + fmt::Debug> fmt::Debug for MappedVec<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_slice().fmt(f)
    }
}

impl<T: Copy + PartialEq> PartialEq for MappedVec<T> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: Copy + Eq> Eq for MappedVec<T> {}

// SAFETY: `MappedVec<T>` owns its mapping and only exposes shared/mutable access according to `T`.
unsafe impl<T: Copy + Send> Send for MappedVec<T> {}
// SAFETY: see above.
unsafe impl<T: Copy + Sync> Sync for MappedVec<T> {}

#[derive(Debug, PartialEq, Eq)]
pub struct FiberStackDistribution {
    entries: MappedVec<(u32, usize)>,
}

impl FiberStackDistribution {
    const fn new() -> Self {
        Self {
            entries: MappedVec::new(),
        }
    }

    fn increment(&mut self, committed_pages: u32) -> Result<(), FiberError> {
        if let Some((_, count)) = self
            .entries
            .as_mut_slice()
            .iter_mut()
            .find(|(pages, _)| *pages == committed_pages)
        {
            *count += 1;
            return Ok(());
        }
        self.entries.push((committed_pages, 1))
    }

    fn sort(&mut self) {
        self.entries
            .sort_by_key(|(committed_pages, _)| *committed_pages);
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[must_use]
    pub const fn as_slice(&self) -> &[(u32, usize)] {
        self.entries.as_slice()
    }
}

impl Default for FiberStackDistribution {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for FiberStackDistribution {
    type Target = [(u32, usize)];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

struct MetadataCursor {
    region: Region,
    offset: usize,
}

impl MetadataCursor {
    const fn new(region: Region) -> Self {
        Self { region, offset: 0 }
    }

    fn reserve_slice<T>(&mut self, len: usize) -> Result<MetadataSlice<T>, FiberError> {
        if len == 0 || size_of::<T>() == 0 {
            return Err(FiberError::invalid());
        }

        let base = self.region.base.as_ptr() as usize;
        let start = fiber_align_up(
            base.checked_add(self.offset)
                .ok_or_else(FiberError::resource_exhausted)?,
            align_of::<T>(),
        )?;
        let offset = start
            .checked_sub(base)
            .ok_or_else(FiberError::resource_exhausted)?;
        let bytes = size_of::<T>()
            .checked_mul(len)
            .ok_or_else(FiberError::resource_exhausted)?;
        let end = offset
            .checked_add(bytes)
            .ok_or_else(FiberError::resource_exhausted)?;
        if end > self.region.len {
            return Err(FiberError::resource_exhausted());
        }

        self.offset = end;
        Ok(MetadataSlice {
            ptr: core::ptr::NonNull::new(start as *mut T).ok_or_else(FiberError::invalid)?,
            len,
        })
    }
}

fn fiber_align_up(value: usize, align: usize) -> Result<usize, FiberError> {
    if align == 0 || !align.is_power_of_two() {
        return Err(FiberError::invalid());
    }
    let mask = align - 1;
    value
        .checked_add(mask)
        .map(|rounded| rounded & !mask)
        .ok_or_else(FiberError::resource_exhausted)
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct FiberStackSlabHeader {
    metadata_len: usize,
    payload_offset: usize,
    capacity: usize,
    slot_stride: usize,
    elastic: bool,
}

#[derive(Debug)]
struct MetadataIndexStack {
    entries: MetadataSlice<usize>,
    len: usize,
}

impl MetadataIndexStack {
    fn with_prefix(entries: MetadataSlice<usize>, len: usize) -> Result<Self, FiberError> {
        if len > entries.len() {
            return Err(FiberError::invalid());
        }
        for index in 0..entries.len() {
            unsafe {
                entries.write(index, 0)?;
            }
        }
        for index in 0..len {
            unsafe {
                entries.write(index, index)?;
            }
        }
        Ok(Self { entries, len })
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

    fn retain_less_than(&mut self, limit: usize) {
        let mut write = 0;
        for read in 0..self.len {
            let value = self.entries[read];
            if value < limit {
                self.entries[write] = value;
                write += 1;
            }
        }
        self.len = write;
    }
}

#[derive(Debug)]
struct MetadataIndexQueue {
    entries: MetadataSlice<usize>,
    head: usize,
    tail: usize,
    len: usize,
}

impl MetadataIndexQueue {
    fn new(entries: MetadataSlice<usize>) -> Result<Self, FiberError> {
        if entries.is_empty() {
            return Err(FiberError::invalid());
        }
        for index in 0..entries.len() {
            unsafe {
                entries.write(index, 0)?;
            }
        }
        Ok(Self {
            entries,
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

    fn steal(&mut self) -> Option<usize> {
        if self.len == 0 {
            return None;
        }
        self.tail = if self.tail == 0 {
            self.entries.len() - 1
        } else {
            self.tail - 1
        };
        let value = self.entries[self.tail];
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

#[repr(C, align(64))]
struct InlineGreenResultBytes {
    bytes: [u8; INLINE_GREEN_RESULT_BYTES],
}

struct InlineGreenResultStorage {
    storage: MaybeUninit<InlineGreenResultBytes>,
    drop: Option<unsafe fn(*mut u8)>,
    type_id: Option<TypeId>,
    occupied: bool,
}

impl fmt::Debug for InlineGreenResultStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InlineGreenResultStorage")
            .field("occupied", &self.occupied)
            .finish_non_exhaustive()
    }
}

impl InlineGreenResultStorage {
    const fn empty() -> Self {
        Self {
            storage: MaybeUninit::uninit(),
            drop: None,
            type_id: None,
            occupied: false,
        }
    }

    const fn supports<T: 'static>() -> bool {
        size_of::<T>() <= size_of::<InlineGreenResultBytes>()
            && align_of::<T>() <= align_of::<InlineGreenResultBytes>()
    }

    fn store<T: 'static>(&mut self, value: T) -> Result<(), FiberError> {
        if self.occupied {
            return Err(FiberError::state_conflict());
        }
        if !Self::supports::<T>() {
            return Err(FiberError::unsupported());
        }

        unsafe {
            self.storage.as_mut_ptr().cast::<T>().write(value);
        }
        self.drop = Some(drop_inline_green_job::<T>);
        self.type_id = Some(TypeId::of::<T>());
        self.occupied = true;
        Ok(())
    }

    fn take<T: 'static>(&mut self) -> Result<T, FiberError> {
        if !self.occupied || self.type_id != Some(TypeId::of::<T>()) {
            return Err(FiberError::state_conflict());
        }

        self.drop = None;
        self.type_id = None;
        self.occupied = false;
        Ok(unsafe { self.storage.as_ptr().cast::<T>().read() })
    }

    fn clear(&mut self) {
        if !self.occupied {
            self.drop = None;
            self.type_id = None;
            return;
        }

        if let Some(drop) = self.drop.take() {
            unsafe {
                drop(self.storage.as_mut_ptr().cast::<u8>());
            }
        }
        self.type_id = None;
        self.occupied = false;
    }
}

impl Drop for InlineGreenResultStorage {
    fn drop(&mut self) {
        self.clear();
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FixedStackLayout {
    usable_size: usize,
    guard: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ElasticStackLayout {
    initial: usize,
    max: usize,
    guard: usize,
    detector: usize,
}

struct ElasticStackMeta {
    reservation_base: usize,
    reservation_end: usize,
    page_size: usize,
    telemetry: FiberTelemetry,
    initial_committed_pages: u32,
    max_committed_pages: u32,
    fiber_id: AtomicU64,
    carrier_id: AtomicUsize,
    capacity_token: AtomicU64,
    initial_detector_page: usize,
    initial_guard_page: usize,
    detector_page: AtomicUsize,
    guard_page: AtomicUsize,
    at_capacity: AtomicBool,
    capacity_pending: AtomicBool,
    occupied: AtomicBool,
    growth_events: AtomicU32,
    committed_pages: AtomicU32,
    active: AtomicBool,
}

impl fmt::Debug for ElasticStackMeta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ElasticStackMeta")
            .field("reservation_base", &self.reservation_base)
            .field("reservation_end", &self.reservation_end)
            .field("page_size", &self.page_size)
            .field("telemetry", &self.telemetry)
            .field("initial_committed_pages", &self.initial_committed_pages)
            .field("max_committed_pages", &self.max_committed_pages)
            .field("fiber_id", &self.fiber_id.load(Ordering::Acquire))
            .field("carrier_id", &self.carrier_id.load(Ordering::Acquire))
            .field(
                "capacity_token",
                &self.capacity_token.load(Ordering::Acquire),
            )
            .field("initial_detector_page", &self.initial_detector_page)
            .field("initial_guard_page", &self.initial_guard_page)
            .field("detector_page", &self.detector_page.load(Ordering::Acquire))
            .field("guard_page", &self.guard_page.load(Ordering::Acquire))
            .field("at_capacity", &self.at_capacity.load(Ordering::Acquire))
            .field(
                "capacity_pending",
                &self.capacity_pending.load(Ordering::Acquire),
            )
            .field("occupied", &self.occupied.load(Ordering::Acquire))
            .field("growth_events", &self.growth_events.load(Ordering::Acquire))
            .field(
                "committed_pages",
                &self.committed_pages.load(Ordering::Acquire),
            )
            .field("active", &self.active.load(Ordering::Acquire))
            .finish()
    }
}

#[derive(Debug)]
enum FiberStackBackingState {
    Fixed(FixedStackLayout),
    Elastic {
        layout: ElasticStackLayout,
        metadata: MetadataSlice<ElasticStackMeta>,
    },
}

#[derive(Debug)]
struct FiberStackSlab {
    mapping: Region,
    region: Region,
    slot_stride: usize,
    capacity: usize,
    initial_slots: usize,
    chunk_size: usize,
    growth: GreenGrowth,
    telemetry: FiberTelemetry,
    huge_pages: HugePagePolicy,
    stack_direction: ContextStackDirection,
    backing: FiberStackBackingState,
    state: SyncMutex<FiberStackSlabState>,
}

#[derive(Debug)]
struct FiberStackSlabState {
    free: MetadataIndexStack,
    allocated: MetadataSlice<bool>,
    committed_slots: usize,
}

#[derive(Debug, Clone, Copy)]
struct FiberStackRegionLayout {
    region: Region,
    slot_stride: usize,
    capacity: usize,
    stack_direction: ContextStackDirection,
}

impl FiberStackSlabState {
    fn new(
        free_entries: MetadataSlice<usize>,
        allocated: MetadataSlice<bool>,
        initial_slots: usize,
    ) -> Result<Self, FiberError> {
        for index in 0..allocated.len() {
            unsafe {
                allocated.write(index, false)?;
            }
        }
        Ok(Self {
            free: MetadataIndexStack::with_prefix(free_entries, initial_slots)?,
            allocated,
            committed_slots: initial_slots,
        })
    }
}

// SAFETY: the mapped region is immutable after construction and slot bookkeeping is serialized
// through `state`.
unsafe impl Send for FiberStackSlab {}
// SAFETY: the mapped region is immutable after construction and slot bookkeeping is serialized
// through `state`.
unsafe impl Sync for FiberStackSlab {}

impl FiberStackSlab {
    fn new(
        config: &FiberPoolConfig,
        alignment: usize,
        stack_direction: ContextStackDirection,
    ) -> Result<Self, FiberError> {
        let backing = config.stack_backing;
        let guard_pages = config.guard_pages;
        let count = config.max_fibers_per_carrier;
        let growth_chunk = config.growth_chunk;
        let growth = config.growth;
        let telemetry = config.telemetry;
        let huge_pages = config.huge_pages;
        if count == 0
            || growth_chunk == 0
            || growth_chunk > count
            || alignment == 0
            || !alignment.is_power_of_two()
        {
            return Err(FiberError::invalid());
        }
        if guard_pages != 0 && matches!(stack_direction, ContextStackDirection::Unknown) {
            return Err(FiberError::unsupported());
        }

        let memory = system_mem();
        Self::validate_huge_page_policy(memory.support().advice, huge_pages)?;
        let page = memory.page_info().alloc_granule.get();
        let rounded_guard = guard_pages
            .checked_mul(page)
            .ok_or_else(FiberError::resource_exhausted)?;
        let (slot_stride, backing) =
            Self::build_backing(backing, rounded_guard, page, alignment, stack_direction)?;
        let total = slot_stride
            .checked_mul(count)
            .ok_or_else(FiberError::resource_exhausted)?;
        let elastic = matches!(backing, FiberStackBackingState::Elastic { .. });
        let metadata_len = Self::metadata_bytes(count, elastic, page)?;
        let mapping_len = metadata_len
            .checked_add(total)
            .ok_or_else(FiberError::resource_exhausted)?;

        let mapping = unsafe {
            memory.map(&MapRequest {
                len: mapping_len,
                align: page,
                protect: Protect::NONE,
                flags: MapFlags::PRIVATE,
                attrs: RegionAttrs::VIRTUAL_ONLY,
                cache: CachePolicy::Default,
                placement: Placement::Anywhere,
                backing: Backing::Anonymous,
            })
        }
        .map_err(fiber_error_from_mem)?;
        let metadata_region = mapping
            .subrange(0, metadata_len)
            .map_err(fiber_error_from_mem)?;
        unsafe { memory.protect(metadata_region, Protect::READ | Protect::WRITE) }
            .map_err(fiber_error_from_mem)?;
        let region = mapping
            .subrange(metadata_len, total)
            .map_err(fiber_error_from_mem)?;

        let initial_slots = match growth {
            GreenGrowth::Fixed => count,
            GreenGrowth::OnDemand => count.min(growth_chunk),
        };
        let (header, state, elastic_metadata) =
            Self::initialize_metadata(metadata_region, count, slot_stride, initial_slots, elastic)?;

        let mut slab = Self {
            mapping,
            region,
            slot_stride,
            capacity: count,
            initial_slots,
            chunk_size: growth_chunk,
            growth,
            telemetry,
            huge_pages,
            stack_direction,
            backing: match backing {
                FiberStackBackingState::Fixed(layout) => FiberStackBackingState::Fixed(layout),
                FiberStackBackingState::Elastic { layout, .. } => FiberStackBackingState::Elastic {
                    layout,
                    metadata: elastic_metadata.ok_or_else(FiberError::invalid)?,
                },
            },
            state: SyncMutex::new(state),
        };
        debug_assert_eq!(header.capacity, count);
        debug_assert_eq!(header.slot_stride, slot_stride);

        slab.initialize_slots(initial_slots)?;
        slab.apply_huge_page_policy()?;

        Ok(slab)
    }

    fn metadata_bytes(capacity: usize, elastic: bool, page: usize) -> Result<usize, FiberError> {
        let mut bytes = size_of::<FiberStackSlabHeader>();
        bytes = fiber_align_up(bytes, align_of::<usize>())?;
        bytes = bytes
            .checked_add(
                size_of::<usize>()
                    .checked_mul(capacity)
                    .ok_or_else(FiberError::resource_exhausted)?,
            )
            .ok_or_else(FiberError::resource_exhausted)?;
        bytes = fiber_align_up(bytes, align_of::<bool>())?;
        bytes = bytes
            .checked_add(
                size_of::<bool>()
                    .checked_mul(capacity)
                    .ok_or_else(FiberError::resource_exhausted)?,
            )
            .ok_or_else(FiberError::resource_exhausted)?;
        if elastic {
            bytes = fiber_align_up(bytes, align_of::<ElasticStackMeta>())?;
            bytes = bytes
                .checked_add(
                    size_of::<ElasticStackMeta>()
                        .checked_mul(capacity)
                        .ok_or_else(FiberError::resource_exhausted)?,
                )
                .ok_or_else(FiberError::resource_exhausted)?;
        }
        fiber_align_up(bytes, page)
    }

    fn initialize_metadata(
        metadata_region: Region,
        capacity: usize,
        slot_stride: usize,
        initial_slots: usize,
        elastic: bool,
    ) -> Result<
        (
            FiberStackSlabHeader,
            FiberStackSlabState,
            Option<MetadataSlice<ElasticStackMeta>>,
        ),
        FiberError,
    > {
        let mut cursor = MetadataCursor::new(metadata_region);
        let header_slice = cursor.reserve_slice::<FiberStackSlabHeader>(1)?;
        let free_entries = cursor.reserve_slice::<usize>(capacity)?;
        let allocated = cursor.reserve_slice::<bool>(capacity)?;
        let elastic_metadata = if elastic {
            Some(cursor.reserve_slice::<ElasticStackMeta>(capacity)?)
        } else {
            None
        };

        let header = FiberStackSlabHeader {
            metadata_len: metadata_region.len,
            payload_offset: metadata_region.len,
            capacity,
            slot_stride,
            elastic,
        };
        unsafe {
            header_slice.write(0, header)?;
        }

        let state = FiberStackSlabState::new(free_entries, allocated, initial_slots)?;
        Ok((header, state, elastic_metadata))
    }

    const fn validate_huge_page_policy(
        advice_caps: MemAdviceCaps,
        policy: HugePagePolicy,
    ) -> Result<(), FiberError> {
        match policy {
            HugePagePolicy::Disabled => Ok(()),
            HugePagePolicy::Enabled { size } => {
                if !advice_caps.contains(MemAdviceCaps::HUGE_PAGE) {
                    return Err(FiberError::unsupported());
                }
                if matches!(size, HugePageSize::OneGiB) && !cfg!(target_arch = "x86_64") {
                    return Err(FiberError::unsupported());
                }
                Ok(())
            }
        }
    }

    fn build_backing(
        backing: FiberStackBacking,
        rounded_guard: usize,
        page: usize,
        alignment: usize,
        stack_direction: ContextStackDirection,
    ) -> Result<(usize, FiberStackBackingState), FiberError> {
        let usable_alignment = alignment.max(page);
        match backing {
            FiberStackBacking::Fixed { stack_size } => {
                let rounded_stack = stack_size
                    .get()
                    .checked_next_multiple_of(usable_alignment)
                    .ok_or_else(FiberError::resource_exhausted)?;
                let slot_stride = rounded_stack
                    .checked_add(rounded_guard)
                    .ok_or_else(FiberError::resource_exhausted)?;
                Ok((
                    slot_stride,
                    FiberStackBackingState::Fixed(FixedStackLayout {
                        usable_size: rounded_stack,
                        guard: rounded_guard,
                    }),
                ))
            }
            FiberStackBacking::Elastic {
                initial_size,
                max_size,
            } => {
                if !system_fiber_host().support().elastic_stack_faults
                    || stack_direction != ContextStackDirection::Down
                    || rounded_guard != page
                {
                    return Err(FiberError::unsupported());
                }
                let rounded_initial = initial_size
                    .get()
                    .checked_next_multiple_of(page)
                    .ok_or_else(FiberError::resource_exhausted)?;
                let rounded_max = max_size
                    .get()
                    .checked_next_multiple_of(page)
                    .ok_or_else(FiberError::resource_exhausted)?;
                if rounded_initial == 0 || rounded_initial > rounded_max {
                    return Err(FiberError::invalid());
                }
                let slot_stride = rounded_max
                    .checked_add(rounded_guard)
                    .and_then(|total| total.checked_add(page))
                    .ok_or_else(FiberError::resource_exhausted)?;
                Ok((
                    slot_stride,
                    FiberStackBackingState::Elastic {
                        layout: ElasticStackLayout {
                            initial: rounded_initial,
                            max: rounded_max,
                            guard: rounded_guard,
                            detector: page,
                        },
                        metadata: MetadataSlice::empty(),
                    },
                ))
            }
        }
    }

    fn initialize_slots(&mut self, committed_slots: usize) -> Result<(), FiberError> {
        let region_layout = FiberStackRegionLayout {
            region: self.region,
            slot_stride: self.slot_stride,
            capacity: self.capacity,
            stack_direction: self.stack_direction,
        };
        let telemetry = self.telemetry;
        match &mut self.backing {
            FiberStackBackingState::Fixed(layout) => {
                Self::initialize_fixed_slots(region_layout, *layout, committed_slots)
            }
            FiberStackBackingState::Elastic { layout, metadata } => Self::initialize_elastic_slots(
                region_layout,
                telemetry,
                *layout,
                committed_slots,
                metadata,
            ),
        }
    }

    fn apply_huge_page_policy(&self) -> Result<(), FiberError> {
        let HugePagePolicy::Enabled { size } = self.huge_pages else {
            return Ok(());
        };

        let memory = system_mem();
        let advice_caps = memory.support().advice;
        if !advice_caps.contains(MemAdviceCaps::HUGE_PAGE) {
            return Err(FiberError::unsupported());
        }

        for slot_index in 0..self.capacity {
            let (huge_region, no_huge_region) = self.huge_page_regions(slot_index, size)?;
            if let Some(region) = huge_region {
                unsafe { memory.advise(region, Advise::HugePage) }.map_err(fiber_error_from_mem)?;
            }
            if let Some(region) = no_huge_region
                && advice_caps.contains(MemAdviceCaps::NO_HUGE_PAGE)
            {
                unsafe { memory.advise(region, Advise::NoHugePage) }
                    .map_err(fiber_error_from_mem)?;
            }
        }

        Ok(())
    }

    fn initialize_fixed_slots(
        region_layout: FiberStackRegionLayout,
        layout: FixedStackLayout,
        committed_slots: usize,
    ) -> Result<(), FiberError> {
        let memory = system_mem();
        for slot_index in 0..region_layout.capacity.min(committed_slots) {
            let slot = Self::slot_region_from(
                region_layout.region,
                region_layout.slot_stride,
                slot_index,
            )?;
            let usable = if layout.guard == 0 {
                slot.subrange(0, layout.usable_size)
            } else {
                match region_layout.stack_direction {
                    ContextStackDirection::Down => slot.subrange(layout.guard, layout.usable_size),
                    ContextStackDirection::Up => slot.subrange(0, layout.usable_size),
                    ContextStackDirection::Unknown => {
                        Err(fusion_pal::sys::mem::MemError::unsupported())
                    }
                }
            }
            .map_err(fiber_error_from_mem)?;
            unsafe { memory.protect(usable, Protect::READ | Protect::WRITE) }
                .map_err(fiber_error_from_mem)?;
        }
        Ok(())
    }

    fn initialize_elastic_slots(
        region_layout: FiberStackRegionLayout,
        telemetry: FiberTelemetry,
        layout: ElasticStackLayout,
        committed_slots: usize,
        metadata: &MetadataSlice<ElasticStackMeta>,
    ) -> Result<(), FiberError> {
        let memory = system_mem();
        for slot_index in 0..region_layout.capacity {
            let slot = Self::slot_region_from(
                region_layout.region,
                region_layout.slot_stride,
                slot_index,
            )?;
            if slot_index < committed_slots {
                let usable = Self::elastic_initial_usable_region_from(
                    region_layout.region,
                    region_layout.slot_stride,
                    region_layout.stack_direction,
                    slot_index,
                    layout,
                )?;
                unsafe { memory.protect(usable, Protect::READ | Protect::WRITE) }
                    .map_err(fiber_error_from_mem)?;
            }
            let detector_offset = slot
                .len
                .checked_sub(layout.initial + layout.detector)
                .ok_or_else(FiberError::invalid)?;
            let detector = slot
                .subrange(detector_offset, layout.detector)
                .map_err(fiber_error_from_mem)?;
            let guard_offset = slot
                .len
                .checked_sub(layout.initial + layout.detector + layout.guard)
                .ok_or_else(FiberError::invalid)?;
            let guard = slot
                .subrange(guard_offset, layout.guard)
                .map_err(fiber_error_from_mem)?;
            unsafe {
                metadata.write(
                    slot_index,
                    ElasticStackMeta {
                        reservation_base: slot.base.addr().get(),
                        reservation_end: slot.end_addr().ok_or_else(FiberError::invalid)?,
                        page_size: layout.detector,
                        telemetry,
                        initial_committed_pages: u32::try_from(layout.initial / layout.detector)
                            .map_err(|_| FiberError::resource_exhausted())?,
                        max_committed_pages: u32::try_from(layout.max / layout.detector)
                            .map_err(|_| FiberError::resource_exhausted())?,
                        fiber_id: AtomicU64::new(0),
                        carrier_id: AtomicUsize::new(0),
                        capacity_token: AtomicU64::new(PlatformWakeToken::invalid().into_raw()),
                        initial_detector_page: detector.base.addr().get(),
                        initial_guard_page: guard.base.addr().get(),
                        detector_page: AtomicUsize::new(detector.base.addr().get()),
                        guard_page: AtomicUsize::new(guard.base.addr().get()),
                        at_capacity: AtomicBool::new(false),
                        capacity_pending: AtomicBool::new(false),
                        occupied: AtomicBool::new(false),
                        growth_events: AtomicU32::new(0),
                        committed_pages: AtomicU32::new(0),
                        active: AtomicBool::new(true),
                    },
                )?;
            }
        }
        register_elastic_stack_metadata(metadata.as_slice())?;
        Ok(())
    }

    fn slot_region(&self, slot_index: usize) -> Result<Region, FiberError> {
        Self::slot_region_from(self.region, self.slot_stride, slot_index)
    }

    fn slot_region_from(
        region: Region,
        slot_stride: usize,
        slot_index: usize,
    ) -> Result<Region, FiberError> {
        region
            .subrange(slot_index * slot_stride, slot_stride)
            .map_err(fiber_error_from_mem)
    }

    fn fixed_usable_region(
        &self,
        slot_index: usize,
        layout: FixedStackLayout,
    ) -> Result<Region, FiberError> {
        let slot = self.slot_region(slot_index)?;
        if layout.guard == 0 {
            return slot
                .subrange(0, layout.usable_size)
                .map_err(fiber_error_from_mem);
        }
        match self.stack_direction {
            ContextStackDirection::Down => slot
                .subrange(layout.guard, layout.usable_size)
                .map_err(fiber_error_from_mem),
            ContextStackDirection::Up => slot
                .subrange(0, layout.usable_size)
                .map_err(fiber_error_from_mem),
            ContextStackDirection::Unknown => Err(FiberError::unsupported()),
        }
    }

    fn elastic_initial_usable_region(
        &self,
        slot_index: usize,
        layout: ElasticStackLayout,
    ) -> Result<Region, FiberError> {
        Self::elastic_initial_usable_region_from(
            self.region,
            self.slot_stride,
            self.stack_direction,
            slot_index,
            layout,
        )
    }

    fn elastic_initial_usable_region_from(
        region: Region,
        slot_stride: usize,
        stack_direction: ContextStackDirection,
        slot_index: usize,
        layout: ElasticStackLayout,
    ) -> Result<Region, FiberError> {
        let slot = Self::slot_region_from(region, slot_stride, slot_index)?;
        match stack_direction {
            ContextStackDirection::Down => slot
                .subrange(
                    slot.len
                        .checked_sub(layout.initial)
                        .ok_or_else(FiberError::invalid)?,
                    layout.initial,
                )
                .map_err(fiber_error_from_mem),
            ContextStackDirection::Up | ContextStackDirection::Unknown => {
                Err(FiberError::unsupported())
            }
        }
    }

    fn elastic_max_usable_region(
        &self,
        slot_index: usize,
        layout: ElasticStackLayout,
    ) -> Result<Region, FiberError> {
        let slot = self.slot_region(slot_index)?;
        match self.stack_direction {
            ContextStackDirection::Down => slot
                .subrange(
                    slot.len
                        .checked_sub(layout.max)
                        .ok_or_else(FiberError::invalid)?,
                    layout.max,
                )
                .map_err(fiber_error_from_mem),
            ContextStackDirection::Up | ContextStackDirection::Unknown => {
                Err(FiberError::unsupported())
            }
        }
    }

    fn huge_page_regions(
        &self,
        slot_index: usize,
        huge_size: HugePageSize,
    ) -> Result<(Option<Region>, Option<Region>), FiberError> {
        let threshold = huge_size.bytes();
        match &self.backing {
            FiberStackBackingState::Fixed(layout) => {
                let usable = self.fixed_usable_region(slot_index, *layout)?;
                if usable.len < threshold {
                    return Ok((None, None));
                }
                Ok((Some(usable), None))
            }
            FiberStackBackingState::Elastic { layout, .. } => {
                let usable = self.elastic_max_usable_region(slot_index, *layout)?;
                if usable.len < threshold {
                    return Ok((None, None));
                }

                let lower_small_window = layout.initial + layout.guard + layout.detector;
                let lower_window = lower_small_window
                    .checked_next_multiple_of(layout.detector)
                    .ok_or_else(FiberError::resource_exhausted)?;
                if usable.len <= lower_window {
                    return Ok((None, None));
                }

                let huge_offset = lower_window;
                let huge_len = usable.len - huge_offset;
                if huge_len < threshold {
                    return Ok((None, None));
                }

                let huge_region = usable
                    .subrange(huge_offset, huge_len)
                    .map_err(fiber_error_from_mem)?;
                let no_huge_region = if huge_offset == 0 {
                    None
                } else {
                    Some(
                        usable
                            .subrange(0, huge_offset)
                            .map_err(fiber_error_from_mem)?,
                    )
                };
                Ok((Some(huge_region), no_huge_region))
            }
        }
    }

    fn acquire(&self) -> Result<FiberStackLease, FiberError> {
        let slot_index = self.acquire_slot_index()?;
        let stack = match &self.backing {
            FiberStackBackingState::Fixed(layout) => {
                let usable = self.fixed_usable_region(slot_index, *layout)?;
                FiberStack::new(usable.base, usable.len)?
            }
            FiberStackBackingState::Elastic { .. } => {
                let slot = self.slot_region(slot_index)?;
                FiberStack::new(slot.base, slot.len)?
            }
        };

        Ok(FiberStackLease { slot_index, stack })
    }

    fn release(&self, slot_index: usize) -> Result<(), FiberError> {
        self.reset_slot(slot_index)?;

        let mut state = self.state.lock().map_err(fiber_error_from_sync)?;
        if slot_index >= state.committed_slots || !state.allocated[slot_index] {
            return Err(FiberError::state_conflict());
        }
        state.allocated[slot_index] = false;
        state.free.push(slot_index)?;
        self.try_shrink_locked(&mut state)
    }

    const fn requires_signal_handler(&self) -> bool {
        matches!(self.backing, FiberStackBackingState::Elastic { .. })
    }

    fn stack_stats(&self) -> Option<FiberStackStats> {
        if matches!(self.telemetry, FiberTelemetry::Disabled) {
            return None;
        }

        let FiberStackBackingState::Elastic { metadata, .. } = &self.backing else {
            return Some(FiberStackStats {
                total_growth_events: 0,
                peak_committed_pages: 0,
                committed_distribution: FiberStackDistribution::new(),
                at_capacity_count: 0,
            });
        };

        let mut stats = FiberStackStats {
            total_growth_events: 0,
            peak_committed_pages: 0,
            committed_distribution: FiberStackDistribution::new(),
            at_capacity_count: 0,
        };
        for meta in &**metadata {
            if !meta.occupied.load(Ordering::Acquire) {
                continue;
            }

            let growth_events = meta.growth_events.load(Ordering::Acquire);
            let committed_pages = Self::current_committed_pages(meta);
            stats.total_growth_events += u64::from(growth_events);
            stats.peak_committed_pages = stats.peak_committed_pages.max(committed_pages);
            if meta.at_capacity.load(Ordering::Acquire) {
                stats.at_capacity_count += 1;
            }

            if stats
                .committed_distribution
                .increment(committed_pages)
                .is_err()
            {
                return None;
            }
        }
        stats.committed_distribution.sort();
        Some(stats)
    }

    fn current_committed_pages(meta: &ElasticStackMeta) -> u32 {
        if !meta.occupied.load(Ordering::Acquire) {
            return 0;
        }
        if meta.at_capacity.load(Ordering::Acquire) {
            return meta.max_committed_pages;
        }
        let detector = meta.detector_page.load(Ordering::Acquire);
        if detector == 0 {
            return meta.max_committed_pages;
        }

        let committed_with_detector = (meta.reservation_end - detector) / meta.page_size;
        let usable_pages = committed_with_detector.saturating_sub(1);
        u32::try_from(usable_pages).unwrap_or(meta.max_committed_pages)
    }

    fn acquire_slot_index(&self) -> Result<usize, FiberError> {
        let mut state = self.state.lock().map_err(fiber_error_from_sync)?;
        if state.free.len == 0 && matches!(self.growth, GreenGrowth::OnDemand) {
            self.grow_locked(&mut state)?;
        }
        let slot_index = state
            .free
            .pop()
            .ok_or_else(FiberError::resource_exhausted)?;
        state.allocated[slot_index] = true;
        self.mark_slot_allocated(slot_index)?;
        Ok(slot_index)
    }

    fn grow_locked(&self, state: &mut FiberStackSlabState) -> Result<(), FiberError> {
        if state.committed_slots >= self.capacity {
            return Err(FiberError::resource_exhausted());
        }

        let start = state.committed_slots;
        let end = self.capacity.min(
            start
                .checked_add(self.chunk_size)
                .ok_or_else(FiberError::resource_exhausted)?,
        );
        self.initialize_slot_range(start, end)?;
        for slot_index in start..end {
            state.free.push(slot_index)?;
        }
        state.committed_slots = end;
        Ok(())
    }

    fn try_shrink_locked(&self, state: &mut FiberStackSlabState) -> Result<(), FiberError> {
        if !matches!(self.growth, GreenGrowth::OnDemand) {
            return Ok(());
        }

        while state.committed_slots > self.initial_slots {
            let Some((tail_start, tail_end)) = self.chunk_range_ending_at(state.committed_slots)
            else {
                return Err(FiberError::state_conflict());
            };
            let Some((prev_start, prev_end)) = self.chunk_range_ending_at(tail_start) else {
                break;
            };
            if !Self::chunk_is_free(state, tail_start, tail_end)
                || !Self::chunk_is_free(state, prev_start, prev_end)
            {
                break;
            }

            self.deinitialize_slot_range(tail_start, tail_end)?;
            state.committed_slots = tail_start;
            state.free.retain_less_than(tail_start);
        }

        Ok(())
    }

    fn chunk_is_free(state: &FiberStackSlabState, start: usize, end: usize) -> bool {
        !state.allocated[start..end]
            .iter()
            .any(|allocated| *allocated)
    }

    fn chunk_range_ending_at(&self, end: usize) -> Option<(usize, usize)> {
        if end == 0 || end > self.capacity {
            return None;
        }
        let chunk_len = match end % self.chunk_size {
            0 => self.chunk_size,
            remainder => remainder,
        };
        Some((end.checked_sub(chunk_len)?, end))
    }

    fn initialize_slot_range(&self, start: usize, end: usize) -> Result<(), FiberError> {
        for slot_index in start..end {
            self.initialize_slot(slot_index)?;
        }
        Ok(())
    }

    fn deinitialize_slot_range(&self, start: usize, end: usize) -> Result<(), FiberError> {
        for slot_index in start..end {
            self.deinitialize_slot(slot_index)?;
        }
        Ok(())
    }

    fn initialize_slot(&self, slot_index: usize) -> Result<(), FiberError> {
        match &self.backing {
            FiberStackBackingState::Fixed(layout) => {
                let memory = system_mem();
                let usable = self.fixed_usable_region(slot_index, *layout)?;
                unsafe { memory.protect(usable, Protect::READ | Protect::WRITE) }
                    .map_err(fiber_error_from_mem)
            }
            FiberStackBackingState::Elastic { layout, metadata } => {
                let memory = system_mem();
                let usable = self.elastic_initial_usable_region(slot_index, *layout)?;
                unsafe { memory.protect(usable, Protect::READ | Protect::WRITE) }
                    .map_err(fiber_error_from_mem)?;
                Self::reset_elastic_metadata(slot_index, metadata)
            }
        }
    }

    fn deinitialize_slot(&self, slot_index: usize) -> Result<(), FiberError> {
        match &self.backing {
            FiberStackBackingState::Fixed(_) => Ok(()),
            FiberStackBackingState::Elastic { metadata, .. } => {
                let memory = system_mem();
                let slot = self.slot_region(slot_index)?;
                unsafe { memory.protect(slot, Protect::NONE) }.map_err(fiber_error_from_mem)?;
                Self::reset_elastic_metadata(slot_index, metadata)
            }
        }
    }

    fn reset_slot(&self, slot_index: usize) -> Result<(), FiberError> {
        match &self.backing {
            FiberStackBackingState::Fixed(_) => Ok(()),
            FiberStackBackingState::Elastic { layout, metadata } => {
                let memory = system_mem();
                let slot = self.slot_region(slot_index)?;
                unsafe { memory.protect(slot, Protect::NONE) }.map_err(fiber_error_from_mem)?;
                let usable = self.elastic_initial_usable_region(slot_index, *layout)?;
                unsafe { memory.protect(usable, Protect::READ | Protect::WRITE) }
                    .map_err(fiber_error_from_mem)?;
                Self::reset_elastic_metadata(slot_index, metadata)
            }
        }
    }

    fn reset_elastic_metadata(
        slot_index: usize,
        metadata: &MetadataSlice<ElasticStackMeta>,
    ) -> Result<(), FiberError> {
        let meta = metadata.get(slot_index).ok_or_else(FiberError::invalid)?;
        meta.detector_page
            .store(meta.initial_detector_page, Ordering::Release);
        meta.guard_page
            .store(meta.initial_guard_page, Ordering::Release);
        meta.at_capacity.store(false, Ordering::Release);
        meta.capacity_pending.store(false, Ordering::Release);
        meta.fiber_id.store(0, Ordering::Release);
        meta.carrier_id.store(0, Ordering::Release);
        meta.capacity_token
            .store(PlatformWakeToken::invalid().into_raw(), Ordering::Release);
        meta.occupied.store(false, Ordering::Release);
        meta.growth_events.store(0, Ordering::Release);
        meta.committed_pages.store(0, Ordering::Release);
        Ok(())
    }

    fn mark_slot_allocated(&self, slot_index: usize) -> Result<(), FiberError> {
        let FiberStackBackingState::Elastic { metadata, .. } = &self.backing else {
            return Ok(());
        };
        let meta = metadata.get(slot_index).ok_or_else(FiberError::invalid)?;
        meta.occupied.store(true, Ordering::Release);
        meta.growth_events.store(0, Ordering::Release);
        meta.committed_pages
            .store(meta.initial_committed_pages, Ordering::Release);
        meta.at_capacity.store(false, Ordering::Release);
        meta.capacity_pending.store(false, Ordering::Release);
        Ok(())
    }

    fn attach_slot_identity(
        &self,
        slot_index: usize,
        fiber_id: u64,
        carrier_id: usize,
        capacity_token: PlatformWakeToken,
    ) -> Result<(), FiberError> {
        let FiberStackBackingState::Elastic { metadata, .. } = &self.backing else {
            return Ok(());
        };
        let meta = metadata.get(slot_index).ok_or_else(FiberError::invalid)?;
        meta.fiber_id.store(fiber_id, Ordering::Release);
        meta.carrier_id.store(carrier_id, Ordering::Release);
        meta.capacity_token
            .store(capacity_token.into_raw(), Ordering::Release);
        Ok(())
    }

    fn take_capacity_event(
        &self,
        slot_index: usize,
    ) -> Result<Option<FiberCapacityEvent>, FiberError> {
        let FiberStackBackingState::Elastic { metadata, .. } = &self.backing else {
            return Ok(None);
        };
        let meta = metadata.get(slot_index).ok_or_else(FiberError::invalid)?;
        if !meta.capacity_pending.swap(false, Ordering::AcqRel) {
            return Ok(None);
        }

        Ok(Some(FiberCapacityEvent {
            fiber_id: meta.fiber_id.load(Ordering::Acquire),
            carrier_id: meta.carrier_id.load(Ordering::Acquire),
            committed_pages: Self::current_committed_pages(meta),
            reservation_pages: meta.max_committed_pages,
        }))
    }

    fn dispatch_capacity_event(
        &self,
        slot_index: usize,
        policy: CapacityPolicy,
    ) -> Result<(), FiberError> {
        let CapacityPolicy::Notify(callback) = policy else {
            return Ok(());
        };
        if let Some(event) = self.take_capacity_event(slot_index)? {
            run_capacity_callback_contained(callback, event);
        }
        Ok(())
    }
}

impl Drop for FiberStackSlab {
    fn drop(&mut self) {
        if let FiberStackBackingState::Elastic { metadata, .. } = &self.backing {
            for meta in metadata.as_slice() {
                meta.active.store(false, Ordering::Release);
            }
            let _ = unregister_elastic_stack_metadata(metadata.as_slice());
        }
        let _ = unsafe { system_mem().unmap(self.mapping) };
    }
}

#[derive(Debug, Clone, Copy)]
struct ElasticRegistryEntry {
    reservation_base: usize,
    reservation_end: usize,
    meta: usize,
}

impl ElasticRegistryEntry {
    fn new(meta: &ElasticStackMeta) -> Self {
        Self {
            reservation_base: meta.reservation_base,
            reservation_end: meta.reservation_end,
            meta: core::ptr::from_ref(meta) as usize,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct ElasticRegistrySnapshotHeader {
    len: usize,
    entries_offset: usize,
}

#[derive(Debug)]
struct ElasticRegistrySnapshot {
    region: Region,
    header: core::ptr::NonNull<ElasticRegistrySnapshotHeader>,
}

impl ElasticRegistrySnapshot {
    fn new(entries: &[ElasticRegistryEntry]) -> Result<Option<Self>, FiberError> {
        if entries.is_empty() {
            return Ok(None);
        }

        let memory = system_mem();
        let page = memory.page_info().alloc_granule.get();
        let entries_offset = fiber_align_up(
            size_of::<ElasticRegistrySnapshotHeader>(),
            align_of::<ElasticRegistryEntry>(),
        )?;
        let entries_bytes = size_of::<ElasticRegistryEntry>()
            .checked_mul(entries.len())
            .ok_or_else(FiberError::resource_exhausted)?;
        let mapping_len = fiber_align_up(
            entries_offset
                .checked_add(entries_bytes)
                .ok_or_else(FiberError::resource_exhausted)?,
            page,
        )?;

        let region = unsafe {
            memory.map(&MapRequest {
                len: mapping_len,
                align: page.max(align_of::<ElasticRegistrySnapshotHeader>()),
                protect: Protect::NONE,
                flags: MapFlags::PRIVATE,
                attrs: RegionAttrs::VIRTUAL_ONLY,
                cache: CachePolicy::Default,
                placement: Placement::Anywhere,
                backing: Backing::Anonymous,
            })
        }
        .map_err(fiber_error_from_mem)?;
        unsafe { memory.protect(region, Protect::READ | Protect::WRITE) }
            .map_err(fiber_error_from_mem)?;

        let header = region.base.cast::<ElasticRegistrySnapshotHeader>();
        let entries_ptr = (region.base.as_ptr() as usize)
            .checked_add(entries_offset)
            .ok_or_else(FiberError::resource_exhausted)?
            as *mut ElasticRegistryEntry;
        debug_assert_eq!(
            entries_ptr.align_offset(align_of::<ElasticRegistryEntry>()),
            0
        );
        unsafe {
            header.as_ptr().write(ElasticRegistrySnapshotHeader {
                len: entries.len(),
                entries_offset,
            });
            core::ptr::copy_nonoverlapping(entries.as_ptr(), entries_ptr, entries.len());
        }

        Ok(Some(Self { region, header }))
    }

    const fn header_ptr(&self) -> *const ElasticRegistrySnapshotHeader {
        self.header.as_ptr()
    }
}

impl Drop for ElasticRegistrySnapshot {
    fn drop(&mut self) {
        let _ = unsafe { system_mem().unmap(self.region) };
    }
}

// SAFETY: snapshots are immutable after publication and keep their backing mapping alive until
// dropped after the reader drain barrier.
unsafe impl Send for ElasticRegistrySnapshot {}
// SAFETY: see above.
unsafe impl Sync for ElasticRegistrySnapshot {}

#[derive(Debug)]
struct ElasticRegistryState {
    pointers: MappedVec<usize>,
    snapshot: Option<ElasticRegistrySnapshot>,
}

static ELASTIC_STACK_REGISTRY: OnceLock<SyncMutex<ElasticRegistryState>> = OnceLock::new();
static ELASTIC_STACK_SNAPSHOT: AtomicUsize = AtomicUsize::new(0);
static ELASTIC_STACK_READERS: AtomicUsize = AtomicUsize::new(0);

fn elastic_registry() -> Result<&'static SyncMutex<ElasticRegistryState>, FiberError> {
    ELASTIC_STACK_REGISTRY
        .get_or_init(|| {
            SyncMutex::new(ElasticRegistryState {
                pointers: MappedVec::new(),
                snapshot: None,
            })
        })
        .map_err(fiber_error_from_sync)
}

fn register_elastic_stack_metadata(metadata: &[ElasticStackMeta]) -> Result<(), FiberError> {
    let registry = elastic_registry()?;
    let mut state = registry.lock().map_err(fiber_error_from_sync)?;
    let previous_len = state.pointers.len();
    for meta in metadata {
        if let Err(error) = state.pointers.push(core::ptr::from_ref(meta) as usize) {
            state.pointers.truncate(previous_len);
            return Err(error);
        }
    }
    let next_snapshot = build_elastic_snapshot(state.pointers.as_slice())?;
    commit_elastic_snapshot(&mut state, next_snapshot);
    Ok(())
}

fn unregister_elastic_stack_metadata(metadata: &[ElasticStackMeta]) -> Result<(), FiberError> {
    let registry = elastic_registry()?;
    let mut state = registry.lock().map_err(fiber_error_from_sync)?;
    state.pointers.retain(|meta_ptr| {
        !metadata
            .iter()
            .any(|meta| core::ptr::from_ref(meta) as usize == *meta_ptr)
    });
    let next_snapshot = build_elastic_snapshot(state.pointers.as_slice())?;
    commit_elastic_snapshot(&mut state, next_snapshot);
    Ok(())
}

fn build_elastic_snapshot(
    pointers: &[usize],
) -> Result<Option<ElasticRegistrySnapshot>, FiberError> {
    let mut entries = MappedVec::with_capacity(pointers.len())?;
    for meta_ptr in pointers {
        let meta = unsafe { &*(*meta_ptr as *const ElasticStackMeta) };
        entries.push(ElasticRegistryEntry::new(meta))?;
    }
    entries.sort_by_key(|entry| entry.reservation_base);
    ElasticRegistrySnapshot::new(entries.as_slice())
}

fn commit_elastic_snapshot(
    state: &mut ElasticRegistryState,
    next_snapshot: Option<ElasticRegistrySnapshot>,
) {
    let next_ptr = next_snapshot
        .as_ref()
        .map_or(0, |snapshot| snapshot.header_ptr() as usize);
    ELASTIC_STACK_SNAPSHOT.store(next_ptr, Ordering::Release);
    let previous = core::mem::replace(&mut state.snapshot, next_snapshot);
    wait_for_elastic_readers_to_drain();
    drop(previous);
}

#[allow(clippy::missing_const_for_fn)]
fn snapshot_entries(snapshot: &ElasticRegistrySnapshotHeader) -> &[ElasticRegistryEntry] {
    // SAFETY: published snapshots point at a live immutable header inside a mapped snapshot
    // region, and the entry payload immediately follows at `entries_offset`.
    let entries_ptr = (core::ptr::from_ref(snapshot).addr() + snapshot.entries_offset)
        as *const ElasticRegistryEntry;
    unsafe { core::slice::from_raw_parts(entries_ptr, snapshot.len) }
}

fn wait_for_elastic_readers_to_drain() {
    while ELASTIC_STACK_READERS.load(Ordering::Acquire) != 0 {
        core::hint::spin_loop();
    }
}

fn find_snapshot_elastic_entry(
    snapshot: &ElasticRegistrySnapshotHeader,
    fault_addr: usize,
) -> Option<ElasticRegistryEntry> {
    let entries = snapshot_entries(snapshot);
    let mut low = 0;
    let mut high = entries.len();
    while low < high {
        let mid = low + ((high - low) / 2);
        let entry = entries[mid];
        if fault_addr < entry.reservation_base {
            high = mid;
        } else if fault_addr >= entry.reservation_end {
            low = mid + 1;
        } else {
            return Some(entry);
        }
    }
    None
}

fn try_promote_elastic_stack_meta(meta: &ElasticStackMeta, fault_addr: usize) -> bool {
    if !meta.active.load(Ordering::Acquire) {
        return false;
    }

    let detector = meta.detector_page.load(Ordering::Acquire);
    let guard = meta.guard_page.load(Ordering::Acquire);
    if fault_addr >= guard && fault_addr < guard.saturating_add(meta.page_size) {
        // Guard-page faults are true stack overflow and must chain to the previous handler.
        return false;
    }
    if fault_addr < detector || fault_addr >= detector.saturating_add(meta.page_size) {
        return false;
    }
    if meta.at_capacity.load(Ordering::Acquire) {
        return false;
    }

    if system_fiber_host()
        .promote_elastic_page(detector, meta.page_size)
        .is_err()
    {
        return false;
    }

    let committed_pages =
        u32::try_from((meta.reservation_end - detector) / meta.page_size).unwrap_or(u32::MAX);
    let next_detector = guard;
    let next_guard = guard.saturating_sub(meta.page_size);
    let previously_at_capacity = meta.at_capacity.load(Ordering::Acquire);
    let at_capacity = next_guard <= meta.reservation_base;
    meta.detector_page.store(next_detector, Ordering::Release);
    meta.guard_page.store(next_guard, Ordering::Release);
    meta.at_capacity.store(at_capacity, Ordering::Release);
    if at_capacity && !previously_at_capacity {
        meta.capacity_pending.store(true, Ordering::Release);
        let token = PlatformWakeToken::from_raw(meta.capacity_token.load(Ordering::Acquire));
        let _ = system_fiber_host().notify_wake_token(token);
    }
    if !matches!(meta.telemetry, FiberTelemetry::Disabled) {
        meta.growth_events.fetch_add(1, Ordering::Relaxed);
        if matches!(meta.telemetry, FiberTelemetry::Full) {
            let _ = meta
                .committed_pages
                .fetch_max(committed_pages, Ordering::Relaxed);
        }
    }
    true
}

fn elastic_stack_fault_handler(fault_addr: usize) -> bool {
    if fault_addr == 0 {
        return false;
    }
    try_promote_elastic_stack_fault(fault_addr)
}

fn try_promote_elastic_stack_fault(fault_addr: usize) -> bool {
    ELASTIC_STACK_READERS.fetch_add(1, Ordering::Acquire);
    let snapshot_ptr =
        ELASTIC_STACK_SNAPSHOT.load(Ordering::Acquire) as *const ElasticRegistrySnapshotHeader;
    let promoted = if snapshot_ptr.is_null() {
        false
    } else {
        let snapshot = unsafe { &*snapshot_ptr };
        let Some(entry) = find_snapshot_elastic_entry(snapshot, fault_addr) else {
            ELASTIC_STACK_READERS.fetch_sub(1, Ordering::Release);
            return false;
        };
        let meta = unsafe { &*(entry.meta as *const ElasticStackMeta) };
        try_promote_elastic_stack_meta(meta, fault_addr)
    };
    ELASTIC_STACK_READERS.fetch_sub(1, Ordering::Release);
    promoted
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CurrentGreenYieldAction {
    Requeue,
    WaitReadiness {
        source: EventSourceHandle,
        interest: EventInterest,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CarrierWaiterRecord {
    key: EventKey,
    source: EventSourceHandle,
    slot_index: usize,
    task_id: u64,
}

#[derive(Debug)]
struct CarrierReactorState {
    reactor: EventSystem,
    poller: SyncMutex<EventPoller>,
    waiters: SyncMutex<MetadataSlice<Option<CarrierWaiterRecord>>>,
    wake: PlatformFiberWakeSignal,
    wake_key: EventKey,
    capacity: PlatformFiberWakeSignal,
    capacity_key: EventKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct CarrierPollResult {
    ready_count: usize,
    capacity_signaled: bool,
}

impl CarrierReactorState {
    fn new(waiters: MetadataSlice<Option<CarrierWaiterRecord>>) -> Result<Self, FiberError> {
        for index in 0..waiters.len() {
            unsafe {
                waiters.write(index, None)?;
            }
        }

        let reactor = EventSystem::new();
        let host = system_fiber_host();
        let mut poller = reactor.create().map_err(fiber_error_from_event)?;
        let wake = host.create_wake_signal().map_err(fiber_error_from_host)?;
        let wake_key = reactor
            .register(
                &mut poller,
                EventSourceHandle(wake.source_handle().map_err(fiber_error_from_host)?),
                EventInterest::READABLE | EventInterest::ERROR | EventInterest::HANGUP,
            )
            .map_err(fiber_error_from_event)?;
        let capacity_signal = host.create_wake_signal().map_err(fiber_error_from_host)?;
        let capacity_key = reactor
            .register(
                &mut poller,
                EventSourceHandle(
                    capacity_signal
                        .source_handle()
                        .map_err(fiber_error_from_host)?,
                ),
                EventInterest::READABLE | EventInterest::ERROR | EventInterest::HANGUP,
            )
            .map_err(fiber_error_from_event)?;
        Ok(Self {
            reactor,
            poller: SyncMutex::new(poller),
            waiters: SyncMutex::new(waiters),
            wake,
            wake_key,
            capacity: capacity_signal,
            capacity_key,
        })
    }

    fn signal(&self) -> Result<(), FiberError> {
        self.wake.signal().map_err(fiber_error_from_host)
    }

    fn capacity_token(&self) -> PlatformWakeToken {
        self.capacity.token()
    }

    fn register_wait(
        &self,
        slot_index: usize,
        task_id: u64,
        source: EventSourceHandle,
        interest: EventInterest,
    ) -> Result<(), FiberError> {
        let mut poller = self.poller.lock().map_err(fiber_error_from_sync)?;
        let mut waiters = self.waiters.lock().map_err(fiber_error_from_sync)?;
        if waiters
            .iter()
            .flatten()
            .any(|waiter| waiter.source == source || waiter.slot_index == slot_index)
        {
            return Err(FiberError::state_conflict());
        }

        let slot = waiters
            .iter_mut()
            .find(|entry| entry.is_none())
            .ok_or_else(FiberError::resource_exhausted)?;
        let key = self
            .reactor
            .register(
                &mut poller,
                source,
                interest | EventInterest::ERROR | EventInterest::HANGUP,
            )
            .map_err(fiber_error_from_event)?;
        *slot = Some(CarrierWaiterRecord {
            key,
            source,
            slot_index,
            task_id,
        });
        Ok(())
    }

    fn waiter_count(&self) -> Result<usize, FiberError> {
        Ok(self
            .waiters
            .lock()
            .map_err(fiber_error_from_sync)?
            .iter()
            .flatten()
            .count())
    }

    fn poll_ready(
        &self,
        timeout: Option<Duration>,
        ready: &mut [Option<CarrierWaiterRecord>; CARRIER_EVENT_BATCH],
    ) -> Result<CarrierPollResult, FiberError> {
        let mut poller = self.poller.lock().map_err(fiber_error_from_sync)?;
        let mut events = [EMPTY_EVENT_RECORD; CARRIER_EVENT_BATCH];
        let count = self
            .reactor
            .poll(&mut poller, &mut events, timeout)
            .map_err(fiber_error_from_event)?;
        let mut result = CarrierPollResult::default();
        for event in events.into_iter().take(count) {
            if event.key == self.wake_key {
                self.wake.drain().map_err(fiber_error_from_host)?;
                continue;
            }
            if event.key == self.capacity_key {
                self.capacity.drain().map_err(fiber_error_from_host)?;
                result.capacity_signaled = true;
                continue;
            }

            let waiter = {
                let mut waiters = self.waiters.lock().map_err(fiber_error_from_sync)?;
                let slot = waiters
                    .iter_mut()
                    .find(|entry| entry.as_ref().is_some_and(|waiter| waiter.key == event.key));
                slot.and_then(Option::take)
            };

            if let Some(waiter) = waiter {
                self.reactor
                    .deregister(&mut poller, waiter.key)
                    .map_err(fiber_error_from_event)?;
                if result.ready_count < ready.len() {
                    ready[result.ready_count] = Some(waiter);
                    result.ready_count += 1;
                }
            }
        }
        Ok(result)
    }

    fn cancel_one_waiter(&self) -> Result<Option<CarrierWaiterRecord>, FiberError> {
        let mut poller = self.poller.lock().map_err(fiber_error_from_sync)?;
        let mut waiters = self.waiters.lock().map_err(fiber_error_from_sync)?;
        let Some(slot) = waiters.iter_mut().find(|entry| entry.is_some()) else {
            return Ok(None);
        };
        let waiter = slot.take().ok_or_else(FiberError::state_conflict)?;
        self.reactor
            .deregister(&mut poller, waiter.key)
            .map_err(fiber_error_from_event)?;
        Ok(Some(waiter))
    }
}

#[derive(Debug)]
struct CarrierQueue {
    queue: SyncMutex<MetadataIndexQueue>,
    ready: Semaphore,
    reactor: Option<CarrierReactorState>,
    steal_state: AtomicU64,
}

impl CarrierQueue {
    fn new(
        queue_entries: MetadataSlice<usize>,
        waiters: Option<MetadataSlice<Option<CarrierWaiterRecord>>>,
        seed: u64,
    ) -> Result<Self, FiberError> {
        let capacity = queue_entries.len();
        Ok(Self {
            queue: SyncMutex::new(MetadataIndexQueue::new(queue_entries)?),
            ready: Semaphore::new(
                0,
                u32::try_from(capacity).map_err(|_| FiberError::resource_exhausted())?,
            )
            .map_err(fiber_error_from_sync)?,
            reactor: match waiters {
                Some(waiters) => Some(CarrierReactorState::new(waiters)?),
                None => None,
            },
            steal_state: AtomicU64::new(seed.max(1)),
        })
    }

    fn signal(&self) -> Result<(), FiberError> {
        if let Some(reactor) = &self.reactor {
            return reactor.signal();
        }
        self.ready.release(1).map_err(fiber_error_from_sync)
    }

    fn capacity_token(&self) -> PlatformWakeToken {
        self.reactor.as_ref().map_or(
            PlatformWakeToken::invalid(),
            CarrierReactorState::capacity_token,
        )
    }

    fn next_steal_start(&self, carrier_count: usize) -> usize {
        if carrier_count <= 1 {
            return 0;
        }

        let mut current = self.steal_state.load(Ordering::Acquire).max(1);
        loop {
            let next = xorshift64(current);
            match self.steal_state.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    let peers = u64::try_from(carrier_count - 1).unwrap_or(u64::MAX);
                    let offset = usize::try_from(next % peers).unwrap_or(0);
                    return offset + 1;
                }
                Err(observed) => current = observed.max(1),
            }
        }
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
    result: InlineGreenResultStorage,
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
            result: InlineGreenResultStorage::empty(),
            state: GreenTaskState::Completed,
        }
    }
}

#[derive(Debug)]
struct GreenTaskSlot {
    owner: AtomicUsize,
    slot_index: usize,
    yield_action: SyncMutex<CurrentGreenYieldAction>,
    record: SyncMutex<GreenTaskRecord>,
    completed: Semaphore,
    handle_refs: AtomicUsize,
}

impl GreenTaskSlot {
    fn new(slot_index: usize) -> Result<Self, FiberError> {
        Ok(Self {
            owner: AtomicUsize::new(0),
            slot_index,
            yield_action: SyncMutex::new(CurrentGreenYieldAction::Requeue),
            record: SyncMutex::new(GreenTaskRecord::empty()),
            completed: Semaphore::new(0, 1).map_err(fiber_error_from_sync)?,
            handle_refs: AtomicUsize::new(0),
        })
    }

    const fn context_ptr(&self) -> *mut () {
        core::ptr::from_ref(self).cast_mut().cast()
    }

    fn set_owner(&self, inner: *const GreenPoolInner) {
        self.owner.store(inner as usize, Ordering::Release);
    }

    fn current_context(&self) -> Result<CurrentGreenContext, FiberError> {
        let inner = self.owner.load(Ordering::Acquire) as *const GreenPoolInner;
        if inner.is_null() {
            return Err(FiberError::state_conflict());
        }

        Ok(CurrentGreenContext {
            inner,
            slot_index: self.slot_index,
            id: self.current_id()?,
        })
    }

    fn set_yield_action(&self, action: CurrentGreenYieldAction) -> Result<(), FiberError> {
        *self.yield_action.lock().map_err(fiber_error_from_sync)? = action;
        Ok(())
    }

    fn take_yield_action(&self) -> Result<CurrentGreenYieldAction, FiberError> {
        let mut guard = self.yield_action.lock().map_err(fiber_error_from_sync)?;
        Ok(core::mem::replace(
            &mut *guard,
            CurrentGreenYieldAction::Requeue,
        ))
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
        record.result.clear();
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

    fn assignment(&self) -> Result<Option<(u64, usize)>, FiberError> {
        let record = self.record.lock().map_err(fiber_error_from_sync)?;
        if !record.allocated {
            return Ok(None);
        }
        Ok(Some((record.id, record.carrier)))
    }

    fn reassign_carrier(&self, id: u64, carrier: usize) -> Result<(), FiberError> {
        let mut record = self.record.lock().map_err(fiber_error_from_sync)?;
        if !Self::matches_id(&record, id) {
            return Err(FiberError::state_conflict());
        }
        if matches!(
            record.state,
            GreenTaskState::Running | GreenTaskState::Waiting
        ) {
            return Err(FiberError::state_conflict());
        }
        record.carrier = carrier;
        Ok(())
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

    fn wait_until_terminal(&self, id: u64) -> Result<GreenTaskState, FiberError> {
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
        Ok(state)
    }

    fn set_state(&self, id: u64, state: GreenTaskState) -> Result<(), FiberError> {
        let mut record = self.record.lock().map_err(fiber_error_from_sync)?;
        if !Self::matches_id(&record, id) {
            return Err(FiberError::state_conflict());
        }
        record.state = state;
        Ok(())
    }

    fn signal_completed(&self, id: u64) -> Result<(), FiberError> {
        {
            let record = self.record.lock().map_err(fiber_error_from_sync)?;
            if !Self::matches_id(&record, id) || !is_terminal_task_state(record.state) {
                return Err(FiberError::state_conflict());
            }
        }
        self.completed.release(1).map_err(fiber_error_from_sync)
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

    fn store_output<T: 'static>(&self, id: u64, value: T) -> Result<(), FiberError> {
        let mut record = self.record.lock().map_err(fiber_error_from_sync)?;
        if !Self::matches_id(&record, id) {
            return Err(FiberError::state_conflict());
        }
        record.result.store(value)
    }

    fn take_output<T: 'static>(&self, id: u64) -> Result<T, FiberError> {
        let mut record = self.record.lock().map_err(fiber_error_from_sync)?;
        if !Self::matches_id(&record, id) {
            return Err(FiberError::state_conflict());
        }
        record.result.take::<T>()
    }

    fn force_recycle(&self, id: u64) -> Result<bool, FiberError> {
        let mut record = self.record.lock().map_err(fiber_error_from_sync)?;
        if !Self::matches_id(&record, id) {
            return Ok(false);
        }
        record.job.clear();
        record.result.clear();
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
        record.result.clear();
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
    slots: MetadataSlice<GreenTaskSlot>,
    free: SyncMutex<MetadataIndexStack>,
}

impl GreenTaskRegistry {
    fn new(
        slots: MetadataSlice<GreenTaskSlot>,
        free_entries: MetadataSlice<usize>,
    ) -> Result<Self, FiberError> {
        if slots.is_empty() || slots.len() != free_entries.len() {
            return Err(FiberError::invalid());
        }

        for slot_index in 0..slots.len() {
            unsafe {
                slots.write(slot_index, GreenTaskSlot::new(slot_index)?)?;
            }
        }

        Ok(Self {
            free: SyncMutex::new(MetadataIndexStack::with_prefix(free_entries, slots.len())?),
            slots,
        })
    }

    fn reserve_slot(&self) -> Result<usize, FiberError> {
        self.free
            .lock()
            .map_err(fiber_error_from_sync)?
            .pop()
            .ok_or_else(FiberError::resource_exhausted)
    }

    fn initialize_owner(&self, inner: *const GreenPoolInner) {
        for slot in &*self.slots {
            slot.set_owner(inner);
        }
    }

    fn assign_job<F>(
        &self,
        slot_index: usize,
        id: u64,
        carrier: usize,
        slab_slot: usize,
        job: F,
    ) -> Result<(), FiberError>
    where
        F: FnOnce() + Send + 'static,
    {
        let slot = &self.slots[slot_index];
        slot.assign(id, carrier, slab_slot, job)
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

    fn slab_slot(&self, slot_index: usize, id: u64) -> Result<usize, FiberError> {
        self.slot(slot_index)?.slab_slot(id)
    }

    fn assignment(&self, slot_index: usize) -> Result<Option<(u64, usize)>, FiberError> {
        self.slot(slot_index)?.assignment()
    }

    fn reassign_carrier(
        &self,
        slot_index: usize,
        id: u64,
        carrier: usize,
    ) -> Result<(), FiberError> {
        self.slot(slot_index)?.reassign_carrier(id, carrier)
    }

    fn state(&self, slot_index: usize, id: u64) -> Result<GreenTaskState, FiberError> {
        self.slot(slot_index)?.state(id)
    }

    fn is_finished(&self, slot_index: usize, id: u64) -> Result<bool, FiberError> {
        self.slot(slot_index)?.is_finished(id)
    }

    fn wait_until_terminal(
        &self,
        slot_index: usize,
        id: u64,
    ) -> Result<GreenTaskState, FiberError> {
        self.slot(slot_index)?.wait_until_terminal(id)
    }

    fn set_state(
        &self,
        slot_index: usize,
        id: u64,
        state: GreenTaskState,
    ) -> Result<(), FiberError> {
        self.slot(slot_index)?.set_state(id, state)
    }

    fn signal_completed(&self, slot_index: usize, id: u64) -> Result<(), FiberError> {
        self.slot(slot_index)?.signal_completed(id)
    }

    fn resume(&self, slot_index: usize, id: u64) -> Result<FiberYield, FiberError> {
        self.slot(slot_index)?.resume(id)
    }

    fn take_output<T: 'static>(&self, slot_index: usize, id: u64) -> Result<T, FiberError> {
        self.slot(slot_index)?.take_output::<T>(id)
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

#[derive(Clone, Copy)]
struct CurrentGreenContext {
    inner: *const GreenPoolInner,
    slot_index: usize,
    id: u64,
}

fn current_green_slot() -> Option<&'static GreenTaskSlot> {
    let context = system_fiber_context().ok()?;
    let slot = context.cast::<GreenTaskSlot>();
    if slot.is_null() {
        return None;
    }
    Some(unsafe { &*slot })
}

fn current_green_context() -> Option<CurrentGreenContext> {
    current_green_slot()?.current_context().ok()
}

#[doc(hidden)]
#[must_use]
pub fn is_in_green_context() -> bool {
    current_green_context().is_some()
}

fn set_current_green_yield_action(action: CurrentGreenYieldAction) {
    if let Some(slot) = current_green_slot() {
        let _ = slot.set_yield_action(action);
    }
}

fn take_current_green_yield_action(
    inner: &GreenPoolInner,
    slot_index: usize,
) -> Result<CurrentGreenYieldAction, FiberError> {
    inner.tasks.slot(slot_index)?.take_yield_action()
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct GreenPoolMetadataHeader {
    metadata_len: usize,
    carrier_count: usize,
    task_capacity: usize,
    reactor_enabled: bool,
}

#[derive(Debug)]
struct GreenPoolMetadata {
    mapping: Region,
    tasks: MetadataSlice<GreenTaskSlot>,
    initialized_tasks: usize,
    carriers: MetadataSlice<CarrierQueue>,
    initialized_carriers: usize,
}

impl GreenPoolMetadata {
    fn new(
        carrier_count: usize,
        task_capacity: usize,
        reactor_enabled: bool,
    ) -> Result<(Self, GreenTaskRegistry, MetadataSlice<CarrierQueue>), FiberError> {
        if carrier_count == 0 || task_capacity == 0 {
            return Err(FiberError::invalid());
        }

        let memory = system_mem();
        let page = memory.page_info().alloc_granule.get();
        let metadata_len =
            Self::metadata_bytes(carrier_count, task_capacity, reactor_enabled, page)?;
        let mapping = unsafe {
            memory.map(&MapRequest {
                len: metadata_len,
                align: page,
                protect: Protect::NONE,
                flags: MapFlags::PRIVATE,
                attrs: RegionAttrs::VIRTUAL_ONLY,
                cache: CachePolicy::Default,
                placement: Placement::Anywhere,
                backing: Backing::Anonymous,
            })
        }
        .map_err(fiber_error_from_mem)?;
        unsafe { memory.protect(mapping, Protect::READ | Protect::WRITE) }
            .map_err(fiber_error_from_mem)?;

        let mut metadata = Self {
            mapping,
            tasks: MetadataSlice::empty(),
            initialized_tasks: 0,
            carriers: MetadataSlice::empty(),
            initialized_carriers: 0,
        };
        let result =
            Self::initialize_into(&mut metadata, carrier_count, task_capacity, reactor_enabled);
        match result {
            Ok((tasks, carriers)) => Ok((metadata, tasks, carriers)),
            Err(error) => Err(error),
        }
    }

    fn metadata_bytes(
        carrier_count: usize,
        task_capacity: usize,
        reactor_enabled: bool,
        page: usize,
    ) -> Result<usize, FiberError> {
        let mut bytes = size_of::<GreenPoolMetadataHeader>();
        bytes = fiber_align_up(bytes, align_of::<GreenTaskSlot>())?;
        bytes = bytes
            .checked_add(
                size_of::<GreenTaskSlot>()
                    .checked_mul(task_capacity)
                    .ok_or_else(FiberError::resource_exhausted)?,
            )
            .ok_or_else(FiberError::resource_exhausted)?;
        bytes = fiber_align_up(bytes, align_of::<usize>())?;
        bytes = bytes
            .checked_add(
                size_of::<usize>()
                    .checked_mul(task_capacity)
                    .ok_or_else(FiberError::resource_exhausted)?,
            )
            .ok_or_else(FiberError::resource_exhausted)?;
        bytes = fiber_align_up(bytes, align_of::<CarrierQueue>())?;
        bytes = bytes
            .checked_add(
                size_of::<CarrierQueue>()
                    .checked_mul(carrier_count)
                    .ok_or_else(FiberError::resource_exhausted)?,
            )
            .ok_or_else(FiberError::resource_exhausted)?;

        for _ in 0..carrier_count {
            bytes = fiber_align_up(bytes, align_of::<usize>())?;
            bytes = bytes
                .checked_add(
                    size_of::<usize>()
                        .checked_mul(task_capacity)
                        .ok_or_else(FiberError::resource_exhausted)?,
                )
                .ok_or_else(FiberError::resource_exhausted)?;
            if reactor_enabled {
                bytes = fiber_align_up(bytes, align_of::<Option<CarrierWaiterRecord>>())?;
                bytes = bytes
                    .checked_add(
                        size_of::<Option<CarrierWaiterRecord>>()
                            .checked_mul(task_capacity)
                            .ok_or_else(FiberError::resource_exhausted)?,
                    )
                    .ok_or_else(FiberError::resource_exhausted)?;
            }
        }

        fiber_align_up(bytes, page)
    }

    fn initialize_into(
        metadata: &mut Self,
        carrier_count: usize,
        task_capacity: usize,
        reactor_enabled: bool,
    ) -> Result<(GreenTaskRegistry, MetadataSlice<CarrierQueue>), FiberError> {
        let mut cursor = MetadataCursor::new(metadata.mapping);
        let header_slice = cursor.reserve_slice::<GreenPoolMetadataHeader>(1)?;
        let task_slots = cursor.reserve_slice::<GreenTaskSlot>(task_capacity)?;
        let free_entries = cursor.reserve_slice::<usize>(task_capacity)?;
        let carriers = cursor.reserve_slice::<CarrierQueue>(carrier_count)?;
        metadata.tasks = task_slots;
        metadata.carriers = carriers;

        let header = GreenPoolMetadataHeader {
            metadata_len: metadata.mapping.len,
            carrier_count,
            task_capacity,
            reactor_enabled,
        };
        unsafe {
            header_slice.write(0, header)?;
        }

        let tasks = GreenTaskRegistry::new(task_slots, free_entries)?;
        metadata.initialized_tasks = task_slots.len();

        for carrier_index in 0..carrier_count {
            let queue_entries = cursor.reserve_slice::<usize>(task_capacity)?;
            let waiters = if reactor_enabled {
                Some(cursor.reserve_slice::<Option<CarrierWaiterRecord>>(task_capacity)?)
            } else {
                None
            };
            let queue =
                CarrierQueue::new(queue_entries, waiters, initial_steal_seed(carrier_index))?;
            unsafe {
                carriers.write(carrier_index, queue)?;
            }
            metadata.initialized_carriers += 1;
        }

        Ok((tasks, carriers))
    }
}

impl Drop for GreenPoolMetadata {
    fn drop(&mut self) {
        for index in 0..self.initialized_carriers {
            unsafe {
                self.carriers.ptr.as_ptr().add(index).drop_in_place();
            }
        }
        for index in 0..self.initialized_tasks {
            unsafe {
                self.tasks.ptr.as_ptr().add(index).drop_in_place();
            }
        }
        let _ = unsafe { system_mem().unmap(self.mapping) };
    }
}

#[repr(C)]
struct GreenPoolControlBlock {
    header: SharedHeader,
    region: Region,
    metadata: ManuallyDrop<GreenPoolMetadata>,
    inner: GreenPoolInner,
}

struct GreenPoolLease {
    ptr: NonNull<GreenPoolControlBlock>,
}

unsafe impl Send for GreenPoolLease {}
unsafe impl Sync for GreenPoolLease {}

impl fmt::Debug for GreenPoolLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GreenPoolLease")
            .field("ptr", &self.ptr)
            .finish_non_exhaustive()
    }
}

impl GreenPoolLease {
    fn new(inner: GreenPoolInner, metadata: GreenPoolMetadata) -> Result<Self, FiberError> {
        let region = green_pool_control_region()?;
        if region.len < size_of::<GreenPoolControlBlock>()
            || !(region.base.as_ptr() as usize).is_multiple_of(align_of::<GreenPoolControlBlock>())
        {
            let _ = unsafe { system_mem().unmap(region) };
            return Err(FiberError::invalid());
        }

        let ptr = region.base.cast::<GreenPoolControlBlock>();
        // SAFETY: the control mapping is uniquely owned here, properly aligned, and large enough
        // to host exactly one green-pool control block.
        unsafe {
            ptr.as_ptr().write(GreenPoolControlBlock {
                header: SharedHeader::new(),
                region,
                metadata: ManuallyDrop::new(metadata),
                inner,
            });
        }
        Ok(Self { ptr })
    }

    fn try_clone(&self) -> Result<Self, FiberError> {
        self.block()
            .header
            .try_retain()
            .map_err(fiber_error_from_sync)?;
        Ok(Self { ptr: self.ptr })
    }

    const fn as_ptr(&self) -> *const GreenPoolInner {
        core::ptr::from_ref(&self.block().inner)
    }

    const fn block(&self) -> &GreenPoolControlBlock {
        // SAFETY: a live lease always points at a live green-pool control block.
        unsafe { self.ptr.as_ref() }
    }
}

impl Deref for GreenPoolLease {
    type Target = GreenPoolInner;

    fn deref(&self) -> &Self::Target {
        &self.block().inner
    }
}

impl Drop for GreenPoolLease {
    fn drop(&mut self) {
        let Ok(release) = self.block().header.release() else {
            return;
        };
        if release != SharedRelease::Last {
            return;
        }

        let block = self.ptr.as_ptr();
        // SAFETY: the final lease exclusively owns the control block. The inner value must be
        // dropped before the metadata mapping is released, and the control mapping itself is only
        // unmapped after both have been torn down.
        unsafe {
            ptr::drop_in_place(addr_of_mut!((*block).inner));
            let metadata = ManuallyDrop::take(&mut (*block).metadata);
            let region = (*block).region;
            drop(metadata);
            let _ = system_mem().unmap(region);
        }
    }
}

fn green_pool_control_region() -> Result<Region, FiberError> {
    let memory = system_mem();
    let page = memory.page_info().alloc_granule.get();
    let len = fiber_align_up(size_of::<GreenPoolControlBlock>(), page)?;
    let region = unsafe {
        memory.map(&MapRequest {
            len,
            align: page.max(align_of::<GreenPoolControlBlock>()),
            protect: Protect::NONE,
            flags: MapFlags::PRIVATE,
            attrs: RegionAttrs::VIRTUAL_ONLY,
            cache: CachePolicy::Default,
            placement: Placement::Anywhere,
            backing: Backing::Anonymous,
        })
    }
    .map_err(fiber_error_from_mem)?;
    unsafe { memory.protect(region, Protect::READ | Protect::WRITE) }
        .map_err(fiber_error_from_mem)?;
    Ok(region)
}

#[derive(Debug)]
struct GreenPoolInner {
    support: FiberSupport,
    scheduling: GreenScheduling,
    capacity_policy: CapacityPolicy,
    shutdown: AtomicBool,
    client_refs: AtomicUsize,
    active: AtomicUsize,
    next_id: AtomicU64,
    next_carrier: AtomicUsize,
    carriers: MetadataSlice<CarrierQueue>,
    tasks: GreenTaskRegistry,
    stack_slab: FiberStackSlab,
}

impl GreenPoolInner {
    fn enqueue(&self, carrier: usize, slot_index: usize) -> Result<(), FiberError> {
        self.enqueue_with_signal(carrier, slot_index, true)
    }

    fn enqueue_with_signal(
        &self,
        carrier: usize,
        slot_index: usize,
        signal: bool,
    ) -> Result<(), FiberError> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(FiberError::state_conflict());
        }

        let queue = self.carriers.get(carrier).ok_or_else(FiberError::invalid)?;
        let mut guard = queue.queue.lock().map_err(fiber_error_from_sync)?;
        guard.enqueue(slot_index)?;
        drop(guard);
        if !signal {
            return Ok(());
        }
        if matches!(self.scheduling, GreenScheduling::WorkStealing) {
            for queue in &*self.carriers {
                queue.signal()?;
            }
            return Ok(());
        }
        queue.signal()
    }

    fn request_shutdown(&self) -> Result<(), FiberError> {
        if self.shutdown.swap(true, Ordering::AcqRel) {
            return Ok(());
        }

        for carrier in &*self.carriers {
            carrier.signal()?;
        }
        Ok(())
    }

    fn migrate_ready_task(
        &self,
        slot_index: usize,
        task_id: u64,
        carrier: usize,
    ) -> Result<(), FiberError> {
        self.tasks.reassign_carrier(slot_index, task_id, carrier)?;
        let slab_slot = self.tasks.slab_slot(slot_index, task_id)?;
        self.stack_slab.attach_slot_identity(
            slab_slot,
            task_id,
            carrier,
            self.carriers[carrier].capacity_token(),
        )
    }

    fn try_steal_ready(&self, carrier: usize) -> Result<Option<usize>, FiberError> {
        if !matches!(self.scheduling, GreenScheduling::WorkStealing) || self.carriers.len() < 2 {
            return Ok(None);
        }

        let start = self.carriers[carrier].next_steal_start(self.carriers.len());
        for step in 0..(self.carriers.len() - 1) {
            let source = (carrier + start + step) % self.carriers.len();
            let source_queue = self.carriers.get(source).ok_or_else(FiberError::invalid)?;
            let stolen = {
                let mut guard = source_queue.queue.lock().map_err(fiber_error_from_sync)?;
                guard.steal()
            };

            let Some(slot_index) = stolen else {
                continue;
            };
            let task_id = self.tasks.current_id(slot_index)?;
            self.migrate_ready_task(slot_index, task_id, carrier)?;
            return Ok(Some(slot_index));
        }

        Ok(None)
    }

    fn park_on_readiness(
        &self,
        carrier_index: usize,
        slot_index: usize,
        task_id: u64,
        source: EventSourceHandle,
        interest: EventInterest,
    ) -> Result<(), FiberError> {
        let carrier = self
            .carriers
            .get(carrier_index)
            .ok_or_else(FiberError::invalid)?;
        let reactor = carrier
            .reactor
            .as_ref()
            .ok_or_else(FiberError::unsupported)?;
        reactor.register_wait(slot_index, task_id, source, interest)?;
        self.tasks
            .set_state(slot_index, task_id, GreenTaskState::Waiting)
    }

    fn dispatch_capacity_for_task(&self, slot_index: usize, id: u64) -> Result<(), FiberError> {
        let slab_slot = self.tasks.slab_slot(slot_index, id)?;
        self.stack_slab
            .dispatch_capacity_event(slab_slot, self.capacity_policy)
    }

    fn dispatch_capacity_for_carrier(&self, carrier_index: usize) -> Result<(), FiberError> {
        let mut first_error = None;
        for slot_index in 0..self.tasks.slots.len() {
            let assignment = match self.tasks.assignment(slot_index) {
                Ok(assignment) => assignment,
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                    continue;
                }
            };
            let Some((task_id, carrier)) = assignment else {
                continue;
            };
            if carrier != carrier_index {
                continue;
            }
            if let Err(error) = self.dispatch_capacity_for_task(slot_index, task_id)
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }

        if let Some(error) = first_error {
            return Err(error);
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

        if let Err(error) = self.tasks.signal_completed(slot_index, id)
            && first_error.is_none()
        {
            first_error = Some(error);
        }

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
pub struct GreenHandle<T = ()> {
    id: u64,
    slot_index: usize,
    inner: GreenPoolLease,
    _marker: PhantomData<fn() -> T>,
}

impl<T> GreenHandle<T>
where
    T: 'static,
{
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
    pub fn join(self) -> Result<T, FiberError> {
        let state = if let Some(current) = current_green_context() {
            if core::ptr::eq(current.inner, self.inner.as_ptr())
                && current.slot_index == self.slot_index
                && current.id == self.id
            {
                return Err(FiberError::state_conflict());
            }
            loop {
                let state = self.inner.tasks.state(self.slot_index, self.id)?;
                if is_terminal_task_state(state) {
                    break state;
                }
                system_yield_now()?;
            }
        } else {
            self.inner
                .tasks
                .wait_until_terminal(self.slot_index, self.id)?
        };

        match state {
            GreenTaskState::Completed => {
                match self.inner.tasks.take_output::<T>(self.slot_index, self.id) {
                    Ok(value) => Ok(value),
                    Err(error)
                        if error.kind() == FiberError::state_conflict().kind()
                            && TypeId::of::<T>() == TypeId::of::<()>() =>
                    {
                        Ok(unsafe { MaybeUninit::<T>::zeroed().assume_init() })
                    }
                    Err(error) => Err(error),
                }
            }
            GreenTaskState::Failed(error) => Err(error),
            GreenTaskState::Queued
            | GreenTaskState::Running
            | GreenTaskState::Yielded
            | GreenTaskState::Waiting => Err(FiberError::state_conflict()),
        }
    }
}

impl GreenHandle<()> {
    /// Attempts to clone one unit-result green-thread handle.
    ///
    /// # Errors
    ///
    /// Returns an error when the underlying green-pool root cannot be retained honestly.
    pub fn try_clone(&self) -> Result<Self, FiberError> {
        let inner = self.inner.try_clone()?;
        self.inner.tasks.clone_handle(self.slot_index)?;
        Ok(Self {
            id: self.id,
            slot_index: self.slot_index,
            inner,
            _marker: PhantomData,
        })
    }
}

impl<T> Drop for GreenHandle<T> {
    fn drop(&mut self) {
        let _ = self.inner.tasks.release_handle(self.slot_index, self.id);
    }
}

/// Public green-thread pool wrapper.
#[derive(Debug)]
pub struct GreenPool {
    inner: GreenPoolLease,
}

#[derive(Debug)]
struct SpawnReservation {
    lease: FiberStackLease,
    id: u64,
    carrier: usize,
    slot_index: usize,
    context: *mut (),
}

#[derive(Debug)]
#[cfg(feature = "std")]
struct AutomaticFiberRuntime {
    _carriers: ThreadPool,
    fibers: GreenPool,
}

#[cfg(feature = "std")]
static AUTOMATIC_FIBER_RUNTIME: OnceLock<SyncMutex<Option<AutomaticFiberRuntime>>> =
    OnceLock::new();

impl GreenPool {
    /// Returns the low-level fiber support available on the current backend.
    #[must_use]
    pub fn support() -> FiberSupport {
        FiberSystem::new().support()
    }

    /// Returns the shared automatic hosted fiber pool, creating it on first use.
    ///
    /// The current automatic carrier default prefers HAL-reported visible physical cores, then
    /// falls back to visible logical CPUs, and otherwise uses one carrier.
    ///
    /// # Errors
    ///
    /// Returns an honest bootstrap failure if the automatic carrier or fiber pool cannot be
    /// realized on the current platform.
    #[cfg(feature = "std")]
    pub fn automatic() -> Result<Self, FiberError> {
        let slot = AUTOMATIC_FIBER_RUNTIME
            .get_or_init(|| SyncMutex::new(None))
            .map_err(fiber_error_from_sync)?;
        let mut guard = slot.lock().map_err(fiber_error_from_sync)?;
        if let Some(runtime) = guard.as_ref() {
            return runtime.fibers.try_clone();
        }

        let runtime = build_automatic_fiber_runtime()?;
        let fibers = runtime.fibers.try_clone()?;
        *guard = Some(runtime);
        Ok(fibers)
    }

    /// Creates a green-thread pool on top of the supplied carrier pool.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the selected fiber backend cannot support the requested
    /// scheduling and migration contract, or the configured slab-backed stack pool cannot be
    /// realized.
    pub fn new(config: &FiberPoolConfig, carrier: &ThreadPool) -> Result<Self, FiberError> {
        let support = Self::support();
        if !support.context.caps.contains(ContextCaps::MAKE)
            || !support.context.caps.contains(ContextCaps::SWAP)
        {
            return Err(FiberError::unsupported());
        }
        if support.context.guard_required && config.guard_pages == 0 {
            return Err(FiberError::invalid());
        }

        let carrier_workers = carrier
            .worker_count()
            .map_err(fiber_error_from_thread_pool)?;
        if config.max_fibers_per_carrier == 0
            || config.growth_chunk == 0
            || config.growth_chunk > config.max_fibers_per_carrier
            || carrier_workers == 0
        {
            return Err(FiberError::invalid());
        }
        if matches!(config.scheduling, GreenScheduling::Priority) {
            return Err(FiberError::unsupported());
        }
        if matches!(config.scheduling, GreenScheduling::WorkStealing)
            && support.context.migration != ContextMigrationSupport::CrossCarrier
        {
            return Err(FiberError::unsupported());
        }
        if matches!(config.stack_backing, FiberStackBacking::Fixed { .. })
            && !matches!(config.growth, GreenGrowth::Fixed)
        {
            return Err(FiberError::unsupported());
        }

        let alignment = support.context.min_stack_alignment.max(16);
        let stack_slab = FiberStackSlab::new(config, alignment, support.context.stack_direction)?;
        let reactor_enabled = EventSystem::new()
            .support()
            .caps
            .contains(EventCaps::READINESS)
            && system_fiber_host().support().wake_signal;
        let (pool_metadata, tasks, carriers) = GreenPoolMetadata::new(
            carrier_workers,
            config.max_fibers_per_carrier,
            reactor_enabled,
        )?;

        let inner = GreenPoolLease::new(
            GreenPoolInner {
                support,
                scheduling: config.scheduling,
                capacity_policy: config.capacity_policy,
                shutdown: AtomicBool::new(false),
                client_refs: AtomicUsize::new(1),
                active: AtomicUsize::new(0),
                next_id: AtomicU64::new(1),
                next_carrier: AtomicUsize::new(0),
                carriers,
                tasks,
                stack_slab,
            },
            pool_metadata,
        )?;
        inner.tasks.initialize_owner(inner.as_ptr());

        for carrier_index in 0..inner.carriers.len() {
            let carrier_inner = inner.try_clone()?;
            if let Err(error) = carrier
                .submit(move || {
                    if run_carrier_loop(&carrier_inner, carrier_index).is_err() {
                        let _ = carrier_inner.request_shutdown();
                    }
                })
                .map_err(fiber_error_from_thread_pool)
            {
                let _ = inner.request_shutdown();
                return Err(error);
            }
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

    /// Returns an approximate stack-telemetry snapshot for live fibers.
    #[must_use]
    pub fn stack_stats(&self) -> Option<FiberStackStats> {
        self.inner.stack_slab.stack_stats()
    }

    fn reserve_spawn_slot(&self) -> Result<SpawnReservation, FiberError> {
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
        let slot_index = match self.inner.tasks.reserve_slot() {
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

        Ok(SpawnReservation {
            lease,
            id,
            carrier,
            slot_index,
            context,
        })
    }

    fn cleanup_failed_spawn(&self, reservation: &SpawnReservation) {
        let _ = self
            .inner
            .tasks
            .abandon(reservation.slot_index, reservation.id);
        let _ = self.inner.stack_slab.release(reservation.lease.slot_index);
        self.inner.active.fetch_sub(1, Ordering::AcqRel);
    }

    /// Spawns one green-thread job onto the carrier-backed scheduler.
    ///
    /// # Errors
    ///
    /// Returns an error when the pool is shut down, capacity is exhausted, the inline task
    /// storage cannot contain the submitted closure, or a new fiber cannot be constructed on the
    /// slab-backed stack store.
    pub fn spawn<F, T>(&self, job: F) -> Result<GreenHandle<T>, FiberError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: 'static,
    {
        if !InlineGreenResultStorage::supports::<T>() {
            return Err(FiberError::unsupported());
        }

        let reservation = self.reserve_spawn_slot()?;
        let slot_addr = reservation.context as usize;
        let wrapped = move || {
            let output = job();
            let slot = unsafe { &*(slot_addr as *const GreenTaskSlot) };
            if let Ok(id) = slot.current_id()
                && slot.store_output(id, output).is_err()
            {
                let _ = slot.set_state(id, GreenTaskState::Failed(FiberError::state_conflict()));
            }
        };

        if let Err(error) = self.inner.tasks.assign_job(
            reservation.slot_index,
            reservation.id,
            reservation.carrier,
            reservation.lease.slot_index,
            wrapped,
        ) {
            self.cleanup_failed_spawn(&reservation);
            return Err(error);
        }

        if let Err(error) = self.inner.stack_slab.attach_slot_identity(
            reservation.lease.slot_index,
            reservation.id,
            reservation.carrier,
            self.inner.carriers[reservation.carrier].capacity_token(),
        ) {
            self.cleanup_failed_spawn(&reservation);
            return Err(error);
        }

        let fiber = match Fiber::new(
            reservation.lease.stack,
            green_task_entry,
            reservation.context,
        ) {
            Ok(fiber) => fiber,
            Err(error) => {
                self.cleanup_failed_spawn(&reservation);
                return Err(error);
            }
        };

        if let Err(error) =
            self.inner
                .tasks
                .install_fiber(reservation.slot_index, reservation.id, fiber)
        {
            self.cleanup_failed_spawn(&reservation);
            return Err(error);
        }

        if let Err(error) = self
            .inner
            .enqueue(reservation.carrier, reservation.slot_index)
        {
            self.cleanup_failed_spawn(&reservation);
            return Err(error);
        }

        Ok(GreenHandle {
            id: reservation.id,
            slot_index: reservation.slot_index,
            inner: self.inner.try_clone()?,
            _marker: PhantomData,
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

impl GreenPool {
    /// Attempts to clone one green-thread pool handle.
    ///
    /// # Errors
    ///
    /// Returns an error when the shared pool root cannot be retained honestly.
    pub fn try_clone(&self) -> Result<Self, FiberError> {
        let inner = self.inner.try_clone()?;
        inner.client_refs.fetch_add(1, Ordering::AcqRel);
        Ok(Self { inner })
    }
}

impl Drop for GreenPool {
    fn drop(&mut self) {
        if self.inner.client_refs.fetch_sub(1, Ordering::AcqRel) == 1 {
            let _ = self.inner.request_shutdown();
        }
    }
}

#[cfg(feature = "std")]
fn build_automatic_fiber_runtime() -> Result<AutomaticFiberRuntime, FiberError> {
    let carrier_count = automatic_carrier_count();
    let carrier_config = ThreadPoolConfig {
        min_threads: carrier_count,
        max_threads: carrier_count,
        placement: if carrier_count > 1 {
            PoolPlacement::PerCore
        } else {
            PoolPlacement::Inherit
        },
        name_prefix: Some("fusion-fiber"),
        ..ThreadPoolConfig::new()
    };
    let carriers = ThreadPool::new(&carrier_config).map_err(fiber_error_from_thread_pool)?;
    let fibers = GreenPool::new(&automatic_fiber_config(), &carriers)?;
    Ok(AutomaticFiberRuntime {
        _carriers: carriers,
        fibers,
    })
}

#[cfg(feature = "std")]
fn automatic_carrier_count() -> usize {
    hal_visible_carrier_count()
        .filter(|count| *count != 0)
        .unwrap_or(1)
}

#[cfg(feature = "std")]
fn hal_visible_carrier_count() -> Option<usize> {
    system_hardware()
        .topology_summary()
        .ok()
        .and_then(select_automatic_carrier_count)
        .filter(|count| *count != 0)
}

#[cfg(feature = "std")]
fn select_automatic_carrier_count(
    summary: fusion_pal::hal::HardwareTopologySummary,
) -> Option<usize> {
    summary.core_count.or(summary.logical_cpu_count)
}

#[cfg(feature = "std")]
fn automatic_fiber_config() -> FiberPoolConfig {
    let mut config = FiberPoolConfig {
        max_fibers_per_carrier: 1024,
        growth_chunk: 32,
        ..FiberPoolConfig::new()
    };
    config.huge_pages = automatic_huge_page_policy(config.stack_backing);
    config
}

#[cfg(feature = "std")]
fn automatic_huge_page_policy(backing: FiberStackBacking) -> HugePagePolicy {
    let FiberStackBacking::Elastic { max_size, .. } = backing else {
        return HugePagePolicy::Disabled;
    };
    if max_size.get() < HugePageSize::TwoMiB.bytes() {
        return HugePagePolicy::Disabled;
    }
    if !system_mem()
        .support()
        .advice
        .contains(MemAdviceCaps::HUGE_PAGE)
    {
        return HugePagePolicy::Disabled;
    }
    HugePagePolicy::Enabled {
        size: HugePageSize::TwoMiB,
    }
}

const fn initial_steal_seed(carrier_index: usize) -> u64 {
    let seed = (carrier_index as u64)
        .wrapping_add(1)
        .wrapping_mul(STEAL_SEED_MIX);
    if seed == 0 { 1 } else { seed }
}

const fn xorshift64(mut state: u64) -> u64 {
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    if state == 0 { 1 } else { state }
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

    if run_green_job_contained(runner).is_err() {
        let _ = slot.set_state(id, GreenTaskState::Failed(FiberError::state_conflict()));
        return FiberReturn::new(usize::MAX);
    }

    FiberReturn::new(0)
}

fn run_carrier_loop(inner: &GreenPoolInner, carrier_index: usize) -> Result<(), FiberError> {
    if inner.carriers[carrier_index].reactor.is_some() {
        return run_reactor_carrier_loop(inner, carrier_index);
    }

    let _alt_stack = if inner.stack_slab.requires_signal_handler() {
        Some(install_carrier_signal_stack()?)
    } else {
        None
    };
    loop {
        while let Some(slot_index) = dequeue_ready(inner, carrier_index)? {
            run_ready_task(inner, carrier_index, slot_index)?;
        }
        if let Some(slot_index) = inner.try_steal_ready(carrier_index)? {
            run_ready_task(inner, carrier_index, slot_index)?;
            continue;
        }
        if inner.shutdown.load(Ordering::Acquire) {
            break;
        }
        let carrier = &inner.carriers[carrier_index];
        carrier.ready.acquire().map_err(fiber_error_from_sync)?;
    }
    Ok(())
}

fn run_reactor_carrier_loop(
    inner: &GreenPoolInner,
    carrier_index: usize,
) -> Result<(), FiberError> {
    let _alt_stack = if inner.stack_slab.requires_signal_handler() {
        Some(install_carrier_signal_stack()?)
    } else {
        None
    };
    let reactor = inner.carriers[carrier_index]
        .reactor
        .as_ref()
        .ok_or_else(FiberError::unsupported)?;

    loop {
        while let Some(slot_index) = dequeue_ready(inner, carrier_index)? {
            run_ready_task(inner, carrier_index, slot_index)?;
        }
        if let Some(slot_index) = inner.try_steal_ready(carrier_index)? {
            run_ready_task(inner, carrier_index, slot_index)?;
            continue;
        }

        if inner.shutdown.load(Ordering::Acquire) {
            while let Some(waiter) = reactor.cancel_one_waiter()? {
                inner.tasks.set_state(
                    waiter.slot_index,
                    waiter.task_id,
                    GreenTaskState::Failed(FiberError::state_conflict()),
                )?;
                inner.finish_task(waiter.slot_index, waiter.task_id)?;
            }
            if reactor.waiter_count()? == 0 {
                break;
            }
            continue;
        }

        let mut ready = [None; CARRIER_EVENT_BATCH];
        let poll_result = reactor.poll_ready(None, &mut ready)?;
        if poll_result.capacity_signaled {
            inner.dispatch_capacity_for_carrier(carrier_index)?;
        }
        for waiter in ready.into_iter().take(poll_result.ready_count).flatten() {
            inner
                .tasks
                .set_state(waiter.slot_index, waiter.task_id, GreenTaskState::Yielded)?;
            inner.enqueue_with_signal(carrier_index, waiter.slot_index, false)?;
        }
    }
    Ok(())
}

fn install_carrier_signal_stack() -> Result<PlatformFiberSignalStack, FiberError> {
    let host = system_fiber_host();
    host.ensure_elastic_fault_handler(elastic_stack_fault_handler)
        .map_err(fiber_error_from_host)?;
    host.install_signal_stack().map_err(fiber_error_from_host)
}

fn dequeue_ready(
    inner: &GreenPoolInner,
    carrier_index: usize,
) -> Result<Option<usize>, FiberError> {
    let carrier = inner
        .carriers
        .get(carrier_index)
        .ok_or_else(FiberError::invalid)?;
    let slot_index = carrier
        .queue
        .lock()
        .map_err(fiber_error_from_sync)?
        .dequeue();
    Ok(slot_index)
}

fn run_ready_task(
    inner: &GreenPoolInner,
    carrier_index: usize,
    slot_index: usize,
) -> Result<(), FiberError> {
    let task_id = inner.tasks.current_id(slot_index)?;
    inner
        .tasks
        .set_state(slot_index, task_id, GreenTaskState::Running)?;
    inner
        .tasks
        .slot(slot_index)?
        .set_yield_action(CurrentGreenYieldAction::Requeue)?;
    let resume = inner.tasks.resume(slot_index, task_id);

    match resume {
        Ok(FiberYield::Yielded) => match take_current_green_yield_action(inner, slot_index)? {
            CurrentGreenYieldAction::Requeue => {
                inner
                    .tasks
                    .set_state(slot_index, task_id, GreenTaskState::Yielded)?;
                inner.dispatch_capacity_for_task(slot_index, task_id)?;
                inner.enqueue_with_signal(carrier_index, slot_index, false)?;
            }
            CurrentGreenYieldAction::WaitReadiness { source, interest } => {
                inner.dispatch_capacity_for_task(slot_index, task_id)?;
                if let Err(error) =
                    inner.park_on_readiness(carrier_index, slot_index, task_id, source, interest)
                {
                    inner
                        .tasks
                        .set_state(slot_index, task_id, GreenTaskState::Failed(error))?;
                    inner.finish_task(slot_index, task_id)?;
                }
            }
        },
        Ok(FiberYield::Completed(_)) => {
            if !matches!(
                inner.tasks.state(slot_index, task_id)?,
                GreenTaskState::Failed(_)
            ) {
                inner
                    .tasks
                    .set_state(slot_index, task_id, GreenTaskState::Completed)?;
            }
            inner.dispatch_capacity_for_task(slot_index, task_id)?;
            inner.finish_task(slot_index, task_id)?;
        }
        Err(error) => {
            inner
                .tasks
                .set_state(slot_index, task_id, GreenTaskState::Failed(error))?;
            inner.dispatch_capacity_for_task(slot_index, task_id)?;
            inner.finish_task(slot_index, task_id)?;
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
    set_current_green_yield_action(CurrentGreenYieldAction::Requeue);
    system_yield_now()
}

#[doc(hidden)]
pub fn wait_for_readiness(
    source: EventSourceHandle,
    interest: EventInterest,
) -> Result<(), FiberError> {
    if current_green_context().is_none() {
        return Err(FiberError::state_conflict());
    }
    set_current_green_yield_action(CurrentGreenYieldAction::WaitReadiness { source, interest });
    if let Err(error) = system_yield_now() {
        set_current_green_yield_action(CurrentGreenYieldAction::Requeue);
        return Err(error);
    }
    Ok(())
}

#[doc(hidden)]
pub fn wait_blocking_for_readiness(
    source: EventSourceHandle,
    interest: EventInterest,
) -> Result<(), FiberError> {
    let reactor = EventSystem::new();
    let mut poller = reactor.create().map_err(fiber_error_from_event)?;
    let key = reactor
        .register(
            &mut poller,
            source,
            interest | EventInterest::ERROR | EventInterest::HANGUP,
        )
        .map_err(fiber_error_from_event)?;
    let mut events = [EMPTY_EVENT_RECORD; 1];
    let poll_result = reactor
        .poll(&mut poller, &mut events, None)
        .map_err(fiber_error_from_event);
    let deregister_result = reactor.deregister(&mut poller, key);
    poll_result?;
    deregister_result.map_err(fiber_error_from_event)?;
    Ok(())
}

fn run_capacity_callback_contained(callback: fn(FiberCapacityEvent), event: FiberCapacityEvent) {
    #[cfg(feature = "std")]
    {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        let _ = catch_unwind(AssertUnwindSafe(|| callback(event)));
    }

    #[cfg(not(feature = "std"))]
    {
        callback(event);
    }
}

fn run_green_job_contained(runner: InlineGreenJobRunner) -> Result<(), ()> {
    #[cfg(feature = "std")]
    {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        catch_unwind(AssertUnwindSafe(|| runner.run())).map_err(|_| ())
    }

    #[cfg(not(feature = "std"))]
    {
        runner.run();
        Ok(())
    }
}

/// Public alias for the carrier-backed stackful scheduler surface.
pub type FiberPool = GreenPool;
/// Public alias for one spawned fiber handle.
pub type FiberHandle<T = ()> = GreenHandle<T>;

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex, OnceLock as StdOnceLock};
    use std::vec::Vec;

    static CAPACITY_EVENT_CALLS: AtomicU32 = AtomicU32::new(0);
    static LAST_CAPACITY_FIBER_ID: AtomicU64 = AtomicU64::new(0);
    static LAST_CAPACITY_CARRIER_ID: AtomicUsize = AtomicUsize::new(usize::MAX);
    static LAST_CAPACITY_COMMITTED: AtomicU32 = AtomicU32::new(0);
    static LAST_CAPACITY_RESERVATION: AtomicU32 = AtomicU32::new(0);
    static ELASTIC_TEST_LOCK: StdOnceLock<StdMutex<()>> = StdOnceLock::new();

    fn lock_elastic_tests() -> std::sync::MutexGuard<'static, ()> {
        ELASTIC_TEST_LOCK
            .get_or_init(|| StdMutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn record_capacity_event(event: FiberCapacityEvent) {
        CAPACITY_EVENT_CALLS.fetch_add(1, Ordering::AcqRel);
        LAST_CAPACITY_FIBER_ID.store(event.fiber_id, Ordering::Release);
        LAST_CAPACITY_CARRIER_ID.store(event.carrier_id, Ordering::Release);
        LAST_CAPACITY_COMMITTED.store(event.committed_pages, Ordering::Release);
        LAST_CAPACITY_RESERVATION.store(event.reservation_pages, Ordering::Release);
    }

    #[test]
    fn elastic_stack_slab_grows_and_shrinks_by_chunk() {
        let _guard = lock_elastic_tests();
        let support = GreenPool::support();
        let config = FiberPoolConfig {
            stack_backing: FiberStackBacking::Elastic {
                initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
                max_size: NonZeroUsize::new(16 * 1024).expect("non-zero max stack"),
            },
            guard_pages: 1,
            growth_chunk: 2,
            max_fibers_per_carrier: 5,
            scheduling: GreenScheduling::Fifo,
            growth: GreenGrowth::OnDemand,
            telemetry: FiberTelemetry::Full,
            capacity_policy: CapacityPolicy::Abort,
            huge_pages: HugePagePolicy::Disabled,
        };
        let slab = FiberStackSlab::new(
            &config,
            support.context.min_stack_alignment.max(16),
            support.context.stack_direction,
        )
        .expect("elastic stack slab should build");

        {
            let state = slab
                .state
                .lock()
                .map_err(fiber_error_from_sync)
                .expect("slab state should be observable");
            assert_eq!(state.committed_slots, 2);
        }

        let mut leases = Vec::new();
        for _ in 0..5 {
            leases.push(slab.acquire().expect("chunked slab should grow on demand"));
        }
        {
            let state = slab
                .state
                .lock()
                .map_err(fiber_error_from_sync)
                .expect("slab state should be observable");
            assert_eq!(state.committed_slots, 5);
        }

        for lease in &leases {
            if lease.slot_index >= 2 {
                slab.release(lease.slot_index)
                    .expect("tail slots should release cleanly");
            }
        }
        {
            let state = slab
                .state
                .lock()
                .map_err(fiber_error_from_sync)
                .expect("slab state should be observable");
            assert_eq!(state.committed_slots, 4);
        }

        for lease in &leases {
            if lease.slot_index < 2 {
                slab.release(lease.slot_index)
                    .expect("initial slots should release cleanly");
            }
        }
        {
            let state = slab
                .state
                .lock()
                .map_err(fiber_error_from_sync)
                .expect("slab state should be observable");
            assert_eq!(state.committed_slots, 2);
        }
    }

    #[test]
    fn elastic_stack_fault_promotion_makes_detector_page_writable() {
        let _guard = lock_elastic_tests();
        let support = GreenPool::support();
        let config = FiberPoolConfig {
            stack_backing: FiberStackBacking::Elastic {
                initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
                max_size: NonZeroUsize::new(16 * 1024).expect("non-zero max stack"),
            },
            guard_pages: 1,
            growth_chunk: 1,
            max_fibers_per_carrier: 1,
            scheduling: GreenScheduling::Fifo,
            growth: GreenGrowth::OnDemand,
            telemetry: FiberTelemetry::Full,
            capacity_policy: CapacityPolicy::Abort,
            huge_pages: HugePagePolicy::Disabled,
        };
        let slab = FiberStackSlab::new(
            &config,
            support.context.min_stack_alignment.max(16),
            support.context.stack_direction,
        )
        .expect("elastic stack slab should build");

        let metadata = match &slab.backing {
            FiberStackBackingState::Elastic { metadata, .. } => metadata,
            FiberStackBackingState::Fixed(_) => panic!("expected elastic backing"),
        };
        let meta = &metadata[0];
        let detector = meta.detector_page.load(Ordering::Acquire);
        let old_guard = meta.guard_page.load(Ordering::Acquire);

        assert!(try_promote_elastic_stack_fault(detector));
        assert_eq!(meta.detector_page.load(Ordering::Acquire), old_guard);
        assert_ne!(meta.guard_page.load(Ordering::Acquire), old_guard);

        unsafe {
            (detector as *mut u8).write_volatile(0x5A);
            assert_eq!((detector as *const u8).read_volatile(), 0x5A);
        }
    }

    #[test]
    fn elastic_stack_stats_track_growth_and_capacity() {
        let _guard = lock_elastic_tests();
        let support = GreenPool::support();
        let config = FiberPoolConfig {
            stack_backing: FiberStackBacking::Elastic {
                initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
                max_size: NonZeroUsize::new(8 * 1024).expect("non-zero max stack"),
            },
            guard_pages: 1,
            growth_chunk: 1,
            max_fibers_per_carrier: 1,
            scheduling: GreenScheduling::Fifo,
            growth: GreenGrowth::OnDemand,
            telemetry: FiberTelemetry::Full,
            capacity_policy: CapacityPolicy::Abort,
            huge_pages: HugePagePolicy::Disabled,
        };
        let slab = FiberStackSlab::new(
            &config,
            support.context.min_stack_alignment.max(16),
            support.context.stack_direction,
        )
        .expect("elastic stack slab should build");

        let lease = slab
            .acquire()
            .expect("elastic slab should allocate one slot");
        let metadata = match &slab.backing {
            FiberStackBackingState::Elastic { metadata, .. } => metadata,
            FiberStackBackingState::Fixed(_) => panic!("expected elastic backing"),
        };
        let meta = &metadata[lease.slot_index];
        let detector = meta.detector_page.load(Ordering::Acquire);

        assert!(try_promote_elastic_stack_fault(detector));

        let stats = slab.stack_stats().expect("telemetry should be enabled");
        assert_eq!(stats.total_growth_events, 1);
        assert_eq!(stats.peak_committed_pages, 2);
        assert_eq!(stats.committed_distribution.as_slice(), &[(2, 1)]);
        assert_eq!(stats.at_capacity_count, 1);

        slab.release(lease.slot_index)
            .expect("elastic slab should release the slot cleanly");
        let stats = slab.stack_stats().expect("telemetry should remain enabled");
        assert_eq!(stats.total_growth_events, 0);
        assert_eq!(stats.peak_committed_pages, 0);
        assert!(stats.committed_distribution.is_empty());
        assert_eq!(stats.at_capacity_count, 0);
    }

    #[test]
    fn elastic_capacity_events_dispatch_with_fiber_identity() {
        let _guard = lock_elastic_tests();
        CAPACITY_EVENT_CALLS.store(0, Ordering::Release);
        LAST_CAPACITY_FIBER_ID.store(0, Ordering::Release);
        LAST_CAPACITY_CARRIER_ID.store(usize::MAX, Ordering::Release);
        LAST_CAPACITY_COMMITTED.store(0, Ordering::Release);
        LAST_CAPACITY_RESERVATION.store(0, Ordering::Release);

        let support = GreenPool::support();
        let config = FiberPoolConfig {
            stack_backing: FiberStackBacking::Elastic {
                initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
                max_size: NonZeroUsize::new(8 * 1024).expect("non-zero max stack"),
            },
            guard_pages: 1,
            growth_chunk: 1,
            max_fibers_per_carrier: 1,
            scheduling: GreenScheduling::Fifo,
            growth: GreenGrowth::OnDemand,
            telemetry: FiberTelemetry::Full,
            capacity_policy: CapacityPolicy::Notify(record_capacity_event),
            huge_pages: HugePagePolicy::Disabled,
        };
        let slab = FiberStackSlab::new(
            &config,
            support.context.min_stack_alignment.max(16),
            support.context.stack_direction,
        )
        .expect("elastic stack slab should build");

        let lease = slab
            .acquire()
            .expect("elastic slab should allocate one slot");
        slab.attach_slot_identity(lease.slot_index, 41, 3, PlatformWakeToken::invalid())
            .expect("slot identity should attach");

        let metadata = match &slab.backing {
            FiberStackBackingState::Elastic { metadata, .. } => metadata,
            FiberStackBackingState::Fixed(_) => panic!("expected elastic backing"),
        };
        let meta = &metadata[lease.slot_index];
        let detector = meta.detector_page.load(Ordering::Acquire);
        assert!(try_promote_elastic_stack_fault(detector));

        slab.dispatch_capacity_event(lease.slot_index, config.capacity_policy)
            .expect("capacity event should dispatch");
        assert_eq!(CAPACITY_EVENT_CALLS.load(Ordering::Acquire), 1);
        assert_eq!(LAST_CAPACITY_FIBER_ID.load(Ordering::Acquire), 41);
        assert_eq!(LAST_CAPACITY_CARRIER_ID.load(Ordering::Acquire), 3);
        assert_eq!(LAST_CAPACITY_COMMITTED.load(Ordering::Acquire), 2);
        assert_eq!(LAST_CAPACITY_RESERVATION.load(Ordering::Acquire), 2);

        slab.dispatch_capacity_event(lease.slot_index, config.capacity_policy)
            .expect("capacity event should not redispatch");
        assert_eq!(CAPACITY_EVENT_CALLS.load(Ordering::Acquire), 1);
    }

    #[test]
    fn elastic_stack_registry_tracks_live_slots_and_clears_on_drop() {
        let _guard = lock_elastic_tests();
        let support = GreenPool::support();
        let config = FiberPoolConfig {
            stack_backing: FiberStackBacking::Elastic {
                initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
                max_size: NonZeroUsize::new(16 * 1024).expect("non-zero max stack"),
            },
            guard_pages: 1,
            growth_chunk: 1,
            max_fibers_per_carrier: 1,
            scheduling: GreenScheduling::Fifo,
            growth: GreenGrowth::OnDemand,
            telemetry: FiberTelemetry::Disabled,
            capacity_policy: CapacityPolicy::Abort,
            huge_pages: HugePagePolicy::Disabled,
        };
        let slab = FiberStackSlab::new(
            &config,
            support.context.min_stack_alignment.max(16),
            support.context.stack_direction,
        )
        .expect("elastic stack slab should build");

        let lease = slab
            .acquire()
            .expect("elastic slab should allocate one slot");
        let metadata = match &slab.backing {
            FiberStackBackingState::Elastic { metadata, .. } => metadata,
            FiberStackBackingState::Fixed(_) => panic!("expected elastic backing"),
        };
        let detector = metadata[lease.slot_index]
            .detector_page
            .load(Ordering::Acquire);

        let snapshot_ptr =
            ELASTIC_STACK_SNAPSHOT.load(Ordering::Acquire) as *const ElasticRegistrySnapshotHeader;
        assert!(!snapshot_ptr.is_null());
        let snapshot = unsafe { &*snapshot_ptr };
        assert!(find_snapshot_elastic_entry(snapshot, detector).is_some());

        drop(slab);
        let snapshot_ptr =
            ELASTIC_STACK_SNAPSHOT.load(Ordering::Acquire) as *const ElasticRegistrySnapshotHeader;
        assert!(snapshot_ptr.is_null());
    }

    #[test]
    fn elastic_huge_page_policy_leaves_a_small_page_growth_window() {
        let _guard = lock_elastic_tests();
        if !system_mem()
            .support()
            .advice
            .contains(MemAdviceCaps::HUGE_PAGE)
        {
            return;
        }
        let support = GreenPool::support();
        let page = system_mem().page_info().alloc_granule.get();
        let config = FiberPoolConfig {
            stack_backing: FiberStackBacking::Elastic {
                initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
                max_size: NonZeroUsize::new(4 * 1024 * 1024).expect("non-zero max stack"),
            },
            guard_pages: 1,
            growth_chunk: 1,
            max_fibers_per_carrier: 1,
            scheduling: GreenScheduling::Fifo,
            growth: GreenGrowth::OnDemand,
            telemetry: FiberTelemetry::Disabled,
            capacity_policy: CapacityPolicy::Abort,
            huge_pages: HugePagePolicy::Enabled {
                size: HugePageSize::TwoMiB,
            },
        };
        let slab = FiberStackSlab::new(
            &config,
            support.context.min_stack_alignment.max(16),
            support.context.stack_direction,
        )
        .expect("elastic stack slab should build with huge-page advice");

        let (huge_region, no_huge_region) = slab
            .huge_page_regions(0, HugePageSize::TwoMiB)
            .expect("huge-page planning should succeed");
        let huge_region =
            huge_region.expect("large elastic slots should expose an upper huge region");
        let no_huge_region = no_huge_region
            .expect("elastic huge-page planning should keep a lower small-page window");
        assert!(huge_region.len >= HugePageSize::TwoMiB.bytes());
        assert!(no_huge_region.len >= 3 * page);
        assert!(huge_region.base.addr().get() > no_huge_region.base.addr().get());
    }

    #[test]
    fn work_stealing_runs_ready_work_on_an_idle_carrier() {
        if GreenPool::support().context.migration != ContextMigrationSupport::CrossCarrier {
            return;
        }

        let carrier = ThreadPool::new(&ThreadPoolConfig {
            min_threads: 2,
            max_threads: 2,
            placement: PoolPlacement::Inherit,
            ..ThreadPoolConfig::new()
        })
        .expect("two-carrier pool should build");
        let fibers = GreenPool::new(
            &FiberPoolConfig {
                scheduling: GreenScheduling::WorkStealing,
                growth_chunk: 4,
                max_fibers_per_carrier: 4,
                ..FiberPoolConfig::new()
            },
            &carrier,
        )
        .expect("work-stealing fiber pool should build");

        let first_thread = Arc::new(StdMutex::new(None));
        let second_thread = Arc::new(StdMutex::new(None));
        let started = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));

        fibers.inner.next_carrier.store(0, Ordering::Release);
        let blocker = fibers
            .spawn({
                let first_thread = Arc::clone(&first_thread);
                let started = Arc::clone(&started);
                let release = Arc::clone(&release);
                move || {
                    *first_thread
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) =
                        Some(std::thread::current().id());
                    started.store(true, Ordering::Release);
                    while !release.load(Ordering::Acquire) {
                        std::thread::yield_now();
                    }
                }
            })
            .expect("blocking fiber should spawn");
        while !started.load(Ordering::Acquire) {
            std::thread::yield_now();
        }

        fibers.inner.next_carrier.store(0, Ordering::Release);
        let stolen = fibers
            .spawn({
                let second_thread = Arc::clone(&second_thread);
                move || {
                    *second_thread
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) =
                        Some(std::thread::current().id());
                }
            })
            .expect("second fiber should spawn onto the busy source carrier");
        stolen
            .join()
            .expect("idle carrier should steal and complete ready work");

        release.store(true, Ordering::Release);
        blocker
            .join()
            .expect("blocking fiber should finish after release");

        let first = first_thread
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .expect("first fiber should record a carrier thread");
        let second = second_thread
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .expect("stolen fiber should record a carrier thread");
        assert_ne!(first, second);

        fibers
            .shutdown()
            .expect("work-stealing fiber pool should shut down cleanly");
        carrier
            .shutdown()
            .expect("carrier pool should shut down cleanly");
    }

    #[test]
    fn automatic_huge_page_policy_tracks_backend_support_and_reservation_size() {
        let small = automatic_huge_page_policy(FiberStackBacking::Elastic {
            initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
            max_size: NonZeroUsize::new(64 * 1024).expect("non-zero max stack"),
        });
        assert_eq!(small, HugePagePolicy::Disabled);

        let large = automatic_huge_page_policy(FiberStackBacking::Elastic {
            initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
            max_size: NonZeroUsize::new(4 * 1024 * 1024).expect("non-zero max stack"),
        });
        let expected = if system_mem()
            .support()
            .advice
            .contains(MemAdviceCaps::HUGE_PAGE)
        {
            HugePagePolicy::Enabled {
                size: HugePageSize::TwoMiB,
            }
        } else {
            HugePagePolicy::Disabled
        };
        assert_eq!(large, expected);
    }

    #[test]
    fn automatic_carrier_selection_prefers_visible_core_count() {
        let summary = fusion_pal::hal::HardwareTopologySummary {
            logical_cpu_count: Some(8),
            core_count: Some(4),
            cluster_count: None,
            package_count: None,
            numa_node_count: None,
            core_class_count: None,
        };
        assert_eq!(select_automatic_carrier_count(summary), Some(4));

        let no_cores = fusion_pal::hal::HardwareTopologySummary {
            core_count: None,
            ..summary
        };
        assert_eq!(select_automatic_carrier_count(no_cores), Some(8));
    }

    #[test]
    fn steal_seed_randomizes_the_first_victim_choice() {
        let first = usize::try_from(xorshift64(initial_steal_seed(0)) % 7).unwrap_or(0) + 1;
        let second = usize::try_from(xorshift64(initial_steal_seed(1)) % 7).unwrap_or(0) + 1;
        assert_ne!(first, second);
    }
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

const fn fiber_error_from_event(error: fusion_sys::event::EventError) -> FiberError {
    match error.kind() {
        fusion_sys::event::EventErrorKind::Unsupported => FiberError::unsupported(),
        fusion_sys::event::EventErrorKind::Invalid => FiberError::invalid(),
        fusion_sys::event::EventErrorKind::ResourceExhausted => FiberError::resource_exhausted(),
        fusion_sys::event::EventErrorKind::Busy
        | fusion_sys::event::EventErrorKind::Timeout
        | fusion_sys::event::EventErrorKind::StateConflict
        | fusion_sys::event::EventErrorKind::Platform(_) => FiberError::state_conflict(),
    }
}

const fn fiber_error_from_host(error: FiberHostError) -> FiberError {
    match error.kind() {
        FiberHostErrorKind::Unsupported => FiberError::unsupported(),
        FiberHostErrorKind::Invalid => FiberError::invalid(),
        FiberHostErrorKind::ResourceExhausted => FiberError::resource_exhausted(),
        FiberHostErrorKind::StateConflict | FiberHostErrorKind::Platform(_) => {
            FiberError::state_conflict()
        }
    }
}
