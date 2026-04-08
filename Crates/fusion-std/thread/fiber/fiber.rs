//! Domain 2: public green-thread and fiber orchestration surface.

use core::any::{
    TypeId,
    type_name,
};
use core::cell::UnsafeCell;
use core::fmt;
use core::marker::PhantomData;
use core::mem::{
    ManuallyDrop,
    MaybeUninit,
    align_of,
    size_of,
};
use core::num::NonZeroU16;
use core::num::NonZeroUsize;
use core::ops::{
    Deref,
    DerefMut,
};
use core::ptr::{
    self,
    NonNull,
    addr_of_mut,
};
#[cfg(feature = "std")]
use core::sync::atomic::AtomicU64;
use core::sync::atomic::{
    AtomicBool,
    AtomicU16,
    AtomicU32,
    AtomicUsize,
    Ordering,
};
use core::time::Duration;

#[cfg(feature = "std")]
use fusion_pal::contract::pal::{
    HardwareTopologyQueryContract as _,
    HardwareTopologySummary,
};
use fusion_pal::contract::pal::runtime::context::ContextErrorKind;
use fusion_pal::sys::cpu::CachePadded;
#[cfg(feature = "std")]
use fusion_pal::sys::cpu::system_cpu;
use fusion_pal::sys::fiber::{
    FiberHostError,
    FiberHostErrorKind,
    PlatformFiberSignalStack,
    PlatformFiberWakeSignal,
    PlatformWakeToken,
    system_fiber_host,
};
use fusion_pal::sys::mem::{
    Advise,
    Backing,
    CachePolicy,
    MapFlags,
    MapRequest,
    MemAdviceCaps,
    MemAdviseContract,
    MemBaseContract,
    MemMapContract,
    MemProtectContract,
    Placement,
    Protect,
    Region,
    RegionAttrs,
    system_mem,
};
use fusion_sys::domain::context::ContextId;
use fusion_sys::courier::{
    CourierChildLaunchRequest,
    CourierFiberRecord,
    CourierId,
    CourierLaneSummary,
    CourierLaunchControl,
    CourierMetadataSubject,
    CourierObligationId,
    CourierObligationSpec,
    CourierResponsiveness,
    CourierRunState,
    CourierRuntimeLedger,
    CourierRuntimeSink,
    CourierRuntimeSummary,
    CourierSchedulingPolicy,
    RunnableUnitKind,
    current_context_id as system_current_context_id,
    current_courier_id as system_current_courier_id,
};
use fusion_sys::event::{
    EventCaps,
    EventInterest,
    EventKey,
    EventNotification,
    EventPoller,
    EventReadiness,
    EventRecord,
    EventSourceHandle,
    EventSystem,
};
use fusion_sys::fiber::{
    ContextCaps,
    ContextMigrationSupport,
    ContextStackDirection,
    Fiber,
    FiberEntry,
    FiberError,
    FiberErrorKind,
    FiberId,
    FiberReturn,
    FiberStack,
    FiberSupport,
    FiberSystem,
    FiberYield,
    ManagedFiberSnapshot,
    current_context as system_fiber_context,
    yield_now as system_yield_now,
};
use fusion_sys::mem::resource::{
    BoundMemoryResource,
    BoundResourceSpec,
    MemoryResource,
    MemoryResourceHandle,
    ResourceBackingKind,
    ResourceError,
    ResourceErrorKind,
    ResourceRange,
};
use fusion_sys::sync::Mutex as SysMutex;
#[cfg(feature = "std")]
use fusion_sys::thread::{
    CarrierObservation,
    CarrierCountPolicy,
    CarrierWorkloadProfile,
    RawThreadEntry,
    ThreadConfig,
    ThreadConstraintMode,
    ThreadEntryReturn,
    ThreadHandle,
    ThreadJoinPolicy,
    ThreadLogicalCpuId,
    ThreadMigrationPolicy,
    ThreadPlacementPhase,
    ThreadPlacementRequest,
    ThreadPlacementTarget,
    ThreadProcessorGroupId,
    ThreadId,
    ThreadStartMode,
    carrier_count_for_profile,
    carrier_count_from_summary,
    system_carrier,
};
use fusion_sys::thread::{
    CarrierSpawnLocalityPolicy,
    RuntimeBackingError,
    RuntimeBackingErrorKind,
    SystemWorkItem,
    ThreadSchedulerCaps,
    ThreadSystem,
    allocate_owned_runtime_slab,
    uses_explicit_bound_runtime_backing,
};
#[cfg(not(feature = "std"))]
use fusion_sys::thread::system_thread;

use crate::sync::{
    Mutex as SyncMutex,
    OnceLock,
    Semaphore,
    SharedHeader,
    SharedRelease,
    SyncError,
    SyncErrorKind,
};
use super::ThreadPool;
#[cfg(feature = "std")]
use super::{
    PoolPlacement,
    ThreadPoolConfig,
};
use super::{
    RuntimeSizingStrategy,
    default_runtime_sizing_strategy,
};

const INLINE_GREEN_JOB_BYTES: usize = 256;
const INLINE_GREEN_RESULT_BYTES: usize = 256;
const CARRIER_EVENT_BATCH: usize = 64;
const FIBER_PRIORITY_LEVELS: usize = u8::MAX as usize + 1;
const FIBER_PRIORITY_WORDS: usize = FIBER_PRIORITY_LEVELS / usize::BITS as usize;
const EMPTY_QUEUE_SLOT: usize = usize::MAX;
const MAX_COOPERATIVE_LOCK_NESTING: usize = 16;
const COOPERATIVE_EXCLUSION_TREE_WORD_BITS: usize = u32::BITS as usize;
const ACTIVE_COOPERATIVE_EXCLUSION_FAST_SPAN_CAPACITY: usize = 1024;
const ACTIVE_COOPERATIVE_EXCLUSION_FAST_LEAF_WORDS: usize =
    ACTIVE_COOPERATIVE_EXCLUSION_FAST_SPAN_CAPACITY / COOPERATIVE_EXCLUSION_TREE_WORD_BITS;
const UNRANKED_COOPERATIVE_LOCK: u16 = 0;
const NO_COOPERATIVE_EXCLUSION_SPAN: u16 = 0;
const FIXED_STACK_WATERMARK_SENTINEL: u8 = 0xA5;
#[cfg(feature = "std")]
const FIBER_YIELD_WATCHDOG_POLL_INTERVAL: Duration = Duration::from_millis(1);
const GREEN_RUNTIME_REGION_CACHE_SLOTS: usize = 4;
#[cfg(target_pointer_width = "64")]
const STEAL_SEED_MIX: usize = 0x9e37_79b9_7f4a_7c15;
#[cfg(not(target_pointer_width = "64"))]
const STEAL_SEED_MIX: usize = 0x7f4a_7c15;
#[cfg(feature = "std")]
const ZERO_LOGICAL_CPU: ThreadLogicalCpuId = ThreadLogicalCpuId {
    group: ThreadProcessorGroupId(0),
    index: 0,
};
#[unsafe(no_mangle)]
pub static FUSION_GREEN_CARRIER_PHASE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static FUSION_GREEN_CARRIER_READY_COUNT: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static FUSION_GREEN_TASK_ENTRY_PHASE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static FUSION_GREEN_TASK_ENTRY_FAILURE_KIND: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static FUSION_GREEN_RESUME_PHASE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static FUSION_GREEN_RESUME_ERROR_KIND: AtomicU32 = AtomicU32::new(0);
#[allow(clippy::cast_possible_truncation)]
const fn wake_token_to_word(token: PlatformWakeToken) -> usize {
    let raw = token.into_raw();
    if raw > usize::MAX as u64 {
        0
    } else {
        raw as usize
    }
}

const fn word_to_wake_token(raw: usize) -> PlatformWakeToken {
    PlatformWakeToken::from_raw(raw as u64)
}

include!("types.rs");

/// TODO: anonymous closure task types remain a metadata blind spot at crate boundaries.
///
/// Today the build-generated manifest can provide exact contracts only when the active crate can
/// name and publish them honestly. Anonymous closure types (`|| { ... }`, `move || { ... }`) do
/// not survive that boundary cleanly under the current sidecar bridge, so missing metadata must
/// stay a hard error instead of silently borrowing the pool's default class and pretending the
/// machine knew better.
fn closure_spawn_task_attributes<F: 'static>(
    _default_class: FiberStackClass,
) -> Result<FiberTaskAttributes, FiberError> {
    let type_name = type_name::<F>();
    if generated_closure_metadata_miss_cached(type_name) {
        return Err(FiberError::unsupported());
    }
    generated_task_attributes_by_type_name(type_name).map_err(|_| {
        remember_generated_closure_metadata_miss(type_name);
        FiberError::unsupported()
    })
}

include!("current_pool.rs");

include!("pool_impl.rs");

/// Explicit fiber-task contract resolved through build-generated metadata.
///
/// The current generated path is backed by a build-script registry. A later stack-analysis tool
/// can replace the manifest source without changing the runtime-facing contract.
pub trait GeneratedExplicitFiberTask: Send + 'static {
    /// Result type produced by this task.
    type Output: 'static;

    /// Optional maximum cooperative run duration between explicit yields or completion.
    const YIELD_BUDGET: Option<Duration> = None;

    /// Runs the explicit task to completion.
    fn run(self) -> Self::Output;

    /// Resolves one runtime task-attribute bundle from generated metadata.
    ///
    /// # Errors
    ///
    /// Returns an error when the build metadata does not contain an entry for this task type, or
    /// when the generated stack budget cannot be mapped to a supported stack class.
    fn task_attributes() -> Result<FiberTaskAttributes, FiberError>
    where
        Self: Sized,
    {
        Ok(generated_task_attributes::<Self>()?.with_optional_yield_budget(Self::YIELD_BUDGET))
    }
}

/// Asserts in one const/static context that an explicit fiber task is supported by one pool
/// configuration.
///
/// This is the critical-safe admission hook for explicit tasks. It intentionally works only with
/// explicit task contracts for now; generated-task compile-time rejection will stay deferred until
/// the generated path stops depending on runtime type-name lookup.
#[macro_export]
macro_rules! assert_explicit_fiber_task_supported {
    ($config:expr, $task:ty $(,)?) => {
        const _: () = {
            ($config).assert_explicit_task_supported::<$task>();
        };
    };
}

/// Asserts in one const/static context that raw fiber-task attributes are supported by one pool
/// configuration.
///
/// This is the lowest-common-denominator compile-time admission hook. It is especially useful for
/// downstream build-generated code that can emit `FiberTaskAttributes` directly without requiring
/// the current crate to know the task type.
#[macro_export]
macro_rules! assert_fiber_task_attributes_supported {
    ($config:expr, $task:expr $(,)?) => {
        const _: () = {
            ($config).assert_task_attributes_supported($task);
        };
    };
}

/// Declares one build-generated fiber-task contract for use in downstream crates.
///
/// This is the cross-crate bridge for generated-task admission. The task still needs a
/// `GeneratedExplicitFiberTask` impl, but that impl can delegate `task_attributes()` to
/// [`generated_explicit_task_contract_attributes()`].
#[macro_export]
macro_rules! declare_generated_fiber_task_contract {
    ($task:ty, $stack_bytes:expr $(,)?) => {
        $crate::declare_generated_fiber_task_contract!(
            $task,
            $stack_bytes,
            $crate::thread::FiberTaskPriority::DEFAULT,
            $crate::thread::FiberTaskExecution::Fiber
        );
    };
    ($task:ty, $stack_bytes:expr, $priority:expr $(,)?) => {
        $crate::declare_generated_fiber_task_contract!(
            $task,
            $stack_bytes,
            $priority,
            $crate::thread::FiberTaskExecution::Fiber
        );
    };
    ($task:ty, $stack_bytes:expr, $priority:expr, $execution:expr $(,)?) => {
        impl $crate::thread::GeneratedExplicitFiberTaskContract for $task {
            const ATTRIBUTES: $crate::thread::FiberTaskAttributes =
                match $crate::thread::admit_generated_fiber_task_stack_bytes($stack_bytes) {
                    Ok(stack_bytes) => {
                        match $crate::thread::FiberTaskAttributes::from_stack_bytes(
                            stack_bytes,
                            $priority,
                        ) {
                            Ok(attributes) => attributes.with_execution($execution),
                            Err(_) => panic!("invalid generated fiber task contract"),
                        }
                    }
                    Err(_) => panic!("invalid generated fiber task contract"),
                };
        }
    };
}

/// Asserts in one const/static context that a build-generated fiber task contract is supported by
/// one pool configuration.
///
/// This currently works only for generated tasks with compile-time contracts emitted into the
/// current crate. Runtime type-name lookup still exists as a compatibility path for the broader
/// generated-task API.
#[macro_export]
macro_rules! assert_generated_fiber_task_supported {
    ($config:expr, $task:ty $(,)?) => {
        const _: () = {
            ($config).assert_generated_task_supported::<$task>();
        };
    };
}

/// Hidden generated-task anchor used to exercise the build-generated metadata pipeline in normal
/// library artifacts.
#[doc(hidden)]
pub struct GeneratedFiberTaskMetadataAnchorTask(u32);

#[doc(hidden)]
#[unsafe(no_mangle)]
pub const extern "Rust" fn generated_fiber_task_metadata_anchor(bytes: u32) -> u32 {
    generated_fiber_task_metadata_anchor_leaf(bytes)
}

#[inline(never)]
const fn generated_fiber_task_metadata_anchor_leaf(bytes: u32) -> u32 {
    bytes.saturating_add(1)
}

#[inline(never)]
fn generated_closure_task_root<F, T>(job: F) -> T
where
    F: FnOnce() -> T,
{
    job()
}

/// Hidden closure-root anchor used to exercise closure-task metadata generation in ordinary
/// library artifacts.
#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "Rust" fn generated_closure_task_metadata_anchor(bytes: u32) -> u32 {
    generated_closure_task_root(|| generated_fiber_task_metadata_anchor_leaf(bytes))
}

impl GeneratedExplicitFiberTask for GeneratedFiberTaskMetadataAnchorTask {
    type Output = u32;

    fn run(self) -> Self::Output {
        generated_fiber_task_metadata_anchor(self.0)
    }
}

include!("config.rs");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GreenTaskState {
    Queued,
    Running,
    Yielded,
    Waiting,
    Finishing,
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
            ptr: region
                .base
                .as_non_null::<T>()
                .ok_or_else(FiberError::invalid)?,
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

impl<T: Copy> Clone for MappedVec<T> {
    fn clone(&self) -> Self {
        let mut next = Self::new();
        if self.is_empty() {
            return next;
        }
        next.grow_for(self.len)
            .expect("mapped vec clone should grow for existing length");
        for item in self.as_slice() {
            next.push_copy(item)
                .expect("mapped vec clone should copy existing items");
        }
        next
    }
}

// SAFETY: `MappedVec<T>` owns its mapping and only exposes shared/mutable access according to `T`.
unsafe impl<T: Copy + Send> Send for MappedVec<T> {}
// SAFETY: see above.
unsafe impl<T: Copy + Sync> Sync for MappedVec<T> {}

#[derive(Debug, Clone, PartialEq, Eq)]
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

        let base = self.region.base.get();
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

struct RuntimeCell<T> {
    fast: bool,
    value: UnsafeCell<T>,
    lock: SysMutex<()>,
}

unsafe impl<T: Send> Send for RuntimeCell<T> {}
unsafe impl<T: Send> Sync for RuntimeCell<T> {}

impl<T: fmt::Debug> fmt::Debug for RuntimeCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeCell")
            .field("fast", &self.fast)
            .finish_non_exhaustive()
    }
}

impl<T> RuntimeCell<T> {
    const fn new(fast: bool, value: T) -> Self {
        Self {
            fast,
            value: UnsafeCell::new(value),
            lock: SysMutex::new(()),
        }
    }

    fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> Result<R, FiberError> {
        if self.fast {
            // SAFETY: fast-mode cells are only used by the thread-affine current-thread runtime.
            return Ok(unsafe { f(&mut *self.value.get()) });
        }
        let _guard = self.lock.lock().map_err(fiber_error_from_sync)?;
        // SAFETY: the lock serializes mutable access in the shared runtime.
        Ok(unsafe { f(&mut *self.value.get()) })
    }

    fn with_ref<R>(&self, f: impl FnOnce(&T) -> R) -> Result<R, FiberError> {
        if self.fast {
            // SAFETY: fast-mode cells are only used by the thread-affine current-thread runtime.
            return Ok(unsafe { f(&*self.value.get()) });
        }
        let _guard = self.lock.lock().map_err(fiber_error_from_sync)?;
        // SAFETY: the lock serializes shared access in the shared runtime.
        Ok(unsafe { f(&*self.value.get()) })
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

#[derive(Debug, Clone, Copy)]
struct PriorityBucket {
    head: usize,
    tail: usize,
}

impl PriorityBucket {
    const fn empty() -> Self {
        Self {
            head: EMPTY_QUEUE_SLOT,
            tail: EMPTY_QUEUE_SLOT,
        }
    }

    const fn is_empty(self) -> bool {
        self.head == EMPTY_QUEUE_SLOT
    }
}

#[derive(Debug)]
struct MetadataPriorityQueue {
    buckets: MetadataSlice<PriorityBucket>,
    next: MetadataSlice<usize>,
    base_priorities: MetadataSlice<i8>,
    enqueue_epochs: MetadataSlice<u64>,
    age_cap: Option<FiberTaskAgeCap>,
    len: usize,
    epoch: u64,
    non_empty: [usize; FIBER_PRIORITY_WORDS],
}

impl MetadataPriorityQueue {
    fn new(
        buckets: MetadataSlice<PriorityBucket>,
        next: MetadataSlice<usize>,
        base_priorities: MetadataSlice<i8>,
        enqueue_epochs: MetadataSlice<u64>,
        age_cap: Option<FiberTaskAgeCap>,
    ) -> Result<Self, FiberError> {
        if next.is_empty()
            || buckets.len() != FIBER_PRIORITY_LEVELS
            || base_priorities.len() != next.len()
            || enqueue_epochs.len() != next.len()
        {
            return Err(FiberError::invalid());
        }
        for index in 0..buckets.len() {
            unsafe {
                buckets.write(index, PriorityBucket::empty())?;
            }
        }
        for index in 0..next.len() {
            unsafe {
                next.write(index, EMPTY_QUEUE_SLOT)?;
                base_priorities.write(index, FiberTaskPriority::DEFAULT.get())?;
                enqueue_epochs.write(index, 0)?;
            }
        }
        Ok(Self {
            buckets,
            next,
            base_priorities,
            enqueue_epochs,
            age_cap,
            len: 0,
            epoch: 0,
            non_empty: [0; FIBER_PRIORITY_WORDS],
        })
    }

    fn enqueue(&mut self, value: usize, priority: FiberTaskPriority) -> Result<(), FiberError> {
        if value >= self.next.len() || self.len == self.next.len() {
            return Err(FiberError::resource_exhausted());
        }

        let index = priority.queue_index();
        let bucket = self
            .buckets
            .get(index)
            .copied()
            .ok_or_else(FiberError::invalid)?;
        self.next[value] = EMPTY_QUEUE_SLOT;
        self.base_priorities[value] = priority.get();
        self.enqueue_epochs[value] = self.epoch;

        if bucket.is_empty() {
            self.buckets[index] = PriorityBucket {
                head: value,
                tail: value,
            };
            self.non_empty[index / usize::BITS as usize] |=
                1usize << (index % usize::BITS as usize);
        } else {
            self.next[bucket.tail] = value;
            self.buckets[index].tail = value;
        }

        self.len += 1;
        Ok(())
    }

    fn dequeue(&mut self) -> Option<usize> {
        if self.len == 0 {
            return None;
        }

        let mut selected = None::<(usize, FiberTaskPriority)>;
        let bits_per_word = usize::BITS as usize;

        for word_index in (0..self.non_empty.len()).rev() {
            let mut word = self.non_empty[word_index];
            while word != 0 {
                let highest_bit = usize::BITS as usize - 1 - word.leading_zeros() as usize;
                let bucket_index = word_index * bits_per_word + highest_bit;
                let bucket = self.buckets[bucket_index];
                let candidate = bucket.head;
                let effective = self.effective_priority(candidate);
                if selected
                    .as_ref()
                    .is_none_or(|(selected_bucket, selected_effective)| {
                        effective > *selected_effective
                            || (effective == *selected_effective && bucket_index > *selected_bucket)
                    })
                {
                    selected = Some((bucket_index, effective));
                }
                word &= !(1usize << highest_bit);
            }
        }

        let (bucket_index, _) = selected?;
        let value = self.pop_bucket_head(bucket_index)?;
        self.epoch = self.epoch.saturating_add(1);
        self.enqueue_epochs[value] = self.epoch;
        Some(value)
    }

    fn effective_priority(&self, value: usize) -> FiberTaskPriority {
        FiberTaskPriority::new(self.base_priorities[value])
            .effective_with_age(self.waiting_age(value))
    }

    fn waiting_age(&self, value: usize) -> FiberTaskAge {
        let age =
            FiberTaskAge::from_epoch_delta(self.epoch.saturating_sub(self.enqueue_epochs[value]));
        match self.age_cap {
            Some(cap) if age.get() > cap.as_age().get() => cap.as_age(),
            _ => age,
        }
    }

    fn pop_bucket_head(&mut self, bucket_index: usize) -> Option<usize> {
        let mut bucket = self.buckets[bucket_index];
        let value = bucket.head;
        if value == EMPTY_QUEUE_SLOT {
            return None;
        }
        let next = self.next[value];
        bucket.head = next;
        if bucket.tail == value {
            bucket.tail = next;
        }
        self.next[value] = EMPTY_QUEUE_SLOT;

        if bucket.head == EMPTY_QUEUE_SLOT {
            bucket = PriorityBucket::empty();
            self.non_empty[bucket_index / usize::BITS as usize] &=
                !(1usize << (bucket_index % usize::BITS as usize));
        }

        self.buckets[bucket_index] = bucket;
        self.len -= 1;
        Some(value)
    }
}

type InlineGreenJobBytes = CachePadded<[u8; INLINE_GREEN_JOB_BYTES]>;

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

type InlineGreenResultBytes = CachePadded<[u8; INLINE_GREEN_RESULT_BYTES]>;

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

include!("stacks.rs");

include!("green.rs");

/// Opaque public green-thread handle.
#[derive(Debug)]
pub struct GreenHandle<T = ()> {
    id: u64,
    slot_index: usize,
    inner: GreenPoolLease,
    drive_mode: GreenHandleDriveMode,
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

    /// Returns the execution strategy used for this admitted task.
    ///
    /// This is the runtime-facing truth for whether the task became one real stackful fiber or
    /// one admitted inline `NoYield` job.
    ///
    /// # Errors
    ///
    /// Returns an error if the green-thread state cannot be observed honestly.
    pub fn execution(&self) -> Result<FiberTaskExecution, FiberError> {
        self.inner.tasks.execution_for(self.slot_index, self.id)
    }

    /// Returns whether this admitted task is executing inline instead of on one dedicated fiber
    /// stack.
    ///
    /// # Errors
    ///
    /// Returns an error if the green-thread state cannot be observed honestly.
    pub fn runs_inline(&self) -> Result<bool, FiberError> {
        Ok(self.execution()?.is_inline())
    }

    /// Returns the runtime admission snapshot for this task.
    ///
    /// # Errors
    ///
    /// Returns an error if the green-thread state cannot be observed honestly.
    pub fn admission(&self) -> Result<FiberTaskAdmission, FiberError> {
        self.inner.tasks.admission_for(self.slot_index, self.id)
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
                ensure_current_green_handoff_unlocked()?;
                system_yield_now()?;
            }
        } else {
            match self.drive_mode {
                GreenHandleDriveMode::CarrierPool => self
                    .inner
                    .tasks
                    .wait_until_terminal(self.slot_index, self.id)?,
                GreenHandleDriveMode::CurrentThread => loop {
                    let state = self.inner.tasks.state(self.slot_index, self.id)?;
                    if is_terminal_task_state(state) {
                        break state;
                    }
                    if !drive_current_pool_once(&self.inner)? {
                        return Err(FiberError::state_conflict());
                    }
                },
            }
        };

        match state {
            GreenTaskState::Completed if size_of::<T>() == 0 => {
                Ok(unsafe { MaybeUninit::<T>::zeroed().assume_init() })
            }
            GreenTaskState::Completed => {
                self.inner.tasks.take_output::<T>(self.slot_index, self.id)
            }
            GreenTaskState::Failed(error) => Err(error),
            GreenTaskState::Queued
            | GreenTaskState::Running
            | GreenTaskState::Yielded
            | GreenTaskState::Finishing
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
            drive_mode: self.drive_mode,
            _marker: PhantomData,
        })
    }
}

impl<T> Drop for GreenHandle<T> {
    fn drop(&mut self) {
        let _ = self.inner.tasks.release_handle(self.slot_index, self.id);
    }
}

/// Thread-affine current-thread fiber handle.
#[derive(Debug)]
pub struct CurrentFiberHandle<T = ()> {
    inner: GreenHandle<T>,
    _not_send_sync: PhantomData<*mut ()>,
}

impl<T> CurrentFiberHandle<T>
where
    T: 'static,
{
    /// Returns the stable current-thread fiber identifier.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.inner.id()
    }

    /// Returns whether the fiber has completed.
    ///
    /// # Errors
    ///
    /// Returns an error if the current-thread pool state cannot be observed honestly.
    pub fn is_finished(&self) -> Result<bool, FiberError> {
        self.inner.is_finished()
    }

    /// Returns the execution strategy used for this admitted task.
    ///
    /// # Errors
    ///
    /// Returns an error if the current-thread pool state cannot be observed honestly.
    pub fn execution(&self) -> Result<FiberTaskExecution, FiberError> {
        self.inner.execution()
    }

    /// Returns whether this admitted task is executing inline instead of on one dedicated fiber
    /// stack.
    ///
    /// # Errors
    ///
    /// Returns an error if the current-thread pool state cannot be observed honestly.
    pub fn runs_inline(&self) -> Result<bool, FiberError> {
        self.inner.runs_inline()
    }

    /// Returns the runtime admission snapshot for this task.
    ///
    /// # Errors
    ///
    /// Returns an error if the current-thread pool state cannot be observed honestly.
    pub fn admission(&self) -> Result<FiberTaskAdmission, FiberError> {
        self.inner.admission()
    }

    /// Waits for the fiber to complete by manually driving the current-thread pool.
    ///
    /// # Errors
    ///
    /// Returns the fiber failure that stopped execution, if any.
    pub fn join(self) -> Result<T, FiberError> {
        self.inner.join()
    }
}

impl CurrentFiberHandle<()> {
    /// Attempts to clone one unit-result current-thread fiber handle.
    ///
    /// # Errors
    ///
    /// Returns an error when the underlying current-thread pool root cannot be retained honestly.
    pub fn try_clone(&self) -> Result<Self, FiberError> {
        Ok(Self {
            inner: self.inner.try_clone()?,
            _not_send_sync: PhantomData,
        })
    }
}

/// Public green-thread pool wrapper.
#[derive(Debug)]
pub struct GreenPool {
    inner: GreenPoolLease,
}

#[derive(Debug)]
struct SpawnReservation {
    lease: Option<FiberStackLease>,
    id: u64,
    carrier: usize,
    slot_index: usize,
    context: *mut (),
}

#[cfg(feature = "std")]
#[allow(clippy::trivially_copy_pass_by_ref)]
fn trace_spawn_failure(stage: &str, slot_index: Option<usize>, error: &FiberError) {
    if std::env::var_os("FUSION_TRACE_SPAWN_FAILURES").is_none() {
        return;
    }
    std::eprintln!(
        "fusion-std spawn failure: stage={stage} slot_index={slot_index:?} kind={:?}",
        error.kind()
    );
}

#[cfg(not(feature = "std"))]
#[allow(clippy::trivially_copy_pass_by_ref)]
fn trace_spawn_failure(_stage: &str, _slot_index: Option<usize>, _error: &FiberError) {}

#[cfg(feature = "std")]
#[allow(clippy::trivially_copy_pass_by_ref)]
fn trace_carrier_failure(stage: &str, carrier_index: usize, error: &FiberError) {
    if std::env::var_os("FUSION_TRACE_CARRIER_ERRORS").is_none() {
        return;
    }
    std::eprintln!(
        "fusion-std carrier failure: stage={stage} carrier_index={carrier_index} kind={:?}",
        error.kind()
    );
}

#[cfg(not(feature = "std"))]
#[allow(clippy::trivially_copy_pass_by_ref)]
fn trace_carrier_failure(_stage: &str, _carrier_index: usize, _error: &FiberError) {}

const fn fiber_error_from_resource(error: ResourceError) -> FiberError {
    match error.kind {
        ResourceErrorKind::UnsupportedRequest | ResourceErrorKind::UnsupportedOperation => {
            FiberError::unsupported()
        }
        ResourceErrorKind::OutOfMemory => FiberError::resource_exhausted(),
        ResourceErrorKind::SynchronizationFailure(_)
        | ResourceErrorKind::InvalidRequest
        | ResourceErrorKind::ContractViolation
        | ResourceErrorKind::InvalidRange
        | ResourceErrorKind::Platform(_) => FiberError::invalid(),
    }
}

fn apply_fiber_sizing_strategy_bytes(
    bytes: usize,
    strategy: RuntimeSizingStrategy,
) -> Result<usize, FiberError> {
    strategy
        .apply_bytes(bytes)
        .ok_or_else(FiberError::resource_exhausted)
}

fn apply_fiber_sizing_strategy_non_zero(
    bytes: NonZeroUsize,
    strategy: RuntimeSizingStrategy,
) -> Result<NonZeroUsize, FiberError> {
    let bytes = apply_fiber_sizing_strategy_bytes(bytes.get(), strategy)?;
    NonZeroUsize::new(bytes).ok_or_else(FiberError::invalid)
}

fn apply_fiber_sizing_strategy_backing(
    backing: FiberStackBacking,
    strategy: RuntimeSizingStrategy,
) -> Result<FiberStackBacking, FiberError> {
    match backing {
        FiberStackBacking::Fixed { stack_size } => Ok(FiberStackBacking::Fixed {
            stack_size: apply_fiber_sizing_strategy_non_zero(stack_size, strategy)?,
        }),
        FiberStackBacking::Elastic {
            initial_size,
            max_size,
        } => {
            let initial_size = apply_fiber_sizing_strategy_non_zero(initial_size, strategy)?;
            let max_size = apply_fiber_sizing_strategy_non_zero(max_size, strategy)?;
            if initial_size.get() > max_size.get() {
                return Err(FiberError::invalid());
            }
            Ok(FiberStackBacking::Elastic {
                initial_size,
                max_size,
            })
        }
    }
}

fn apply_fiber_backing_request(
    request: FiberPoolBackingRequest,
    strategy: RuntimeSizingStrategy,
) -> Result<FiberPoolBackingRequest, FiberError> {
    Ok(FiberPoolBackingRequest {
        bytes: apply_fiber_sizing_strategy_bytes(request.bytes, strategy)?,
        align: request.align,
    })
}

fn task_attributes_from_stack_bytes<const STACK_BYTES: usize>()
-> Result<FiberTaskAttributes, FiberError> {
    let stack_bytes = NonZeroUsize::new(STACK_BYTES).ok_or_else(FiberError::invalid)?;
    FiberTaskAttributes::from_stack_bytes(stack_bytes, FiberTaskPriority::DEFAULT)
}

#[cfg(feature = "std")]
fn select_spawn_carrier(inner: &GreenPoolLease) -> usize {
    let carrier_count = inner.carriers.len();
    if carrier_count <= 1 {
        return 0;
    }
    let start = inner.next_carrier.fetch_add(1, Ordering::AcqRel) % carrier_count;
    let Ok(origin) = system_carrier().observe_current() else {
        return start;
    };
    select_spawn_carrier_by_locality(inner, origin, start, inner.spawn_locality_policy)
        .unwrap_or(start)
}

#[cfg(not(feature = "std"))]
fn select_spawn_carrier(inner: &GreenPoolLease) -> usize {
    inner.next_carrier.fetch_add(1, Ordering::AcqRel) % inner.carriers.len()
}

#[cfg(feature = "std")]
fn select_spawn_carrier_by_locality(
    inner: &GreenPoolLease,
    origin: CarrierObservation,
    start: usize,
    policy: CarrierSpawnLocalityPolicy,
) -> Option<usize> {
    let carrier_contexts = inner.block().metadata.carrier_contexts;
    if carrier_contexts.is_empty() {
        return None;
    }

    let mut best: Option<(u8, usize)> = None;
    for offset in 0..carrier_contexts.len() {
        let carrier_index = (start + offset) % carrier_contexts.len();
        let Some(context) = carrier_contexts.get(carrier_index) else {
            continue;
        };
        if context.observed_thread_id() == Some(origin.thread_id) {
            return Some(carrier_index);
        }
        let rank = fusion_sys::thread::carrier_spawn_locality_rank(
            policy,
            origin.location,
            context.observed_location(),
        );
        let Some(rank) = rank else {
            continue;
        };
        if best.is_none_or(|(best_rank, _)| rank < best_rank) {
            best = Some((rank, carrier_index));
        }
    }
    best.map(|(_, carrier_index)| carrier_index)
}

fn reserve_spawn_slot_for(
    inner: &GreenPoolLease,
    task: FiberTaskAttributes,
) -> Result<SpawnReservation, FiberError> {
    if task.yield_budget.is_some() && !inner.yield_budget_supported {
        return Err(FiberError::unsupported());
    }
    #[cfg(feature = "std")]
    ensure_yield_budget_watchdog_started(inner, task)?;
    if task.execution.requires_fiber() && !inner.stacks.supports_task_class(task.stack_class) {
        return Err(FiberError::unsupported());
    }
    loop {
        let active = inner.active.load(Ordering::Acquire);
        if active >= inner.stacks.total_capacity() {
            return Err(FiberError::resource_exhausted());
        }
        if inner
            .active
            .compare_exchange(active, active + 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            break;
        }
    }

    let lease = if task.execution.requires_fiber() {
        match inner.stacks.acquire(task) {
            Ok(lease) => Some(lease),
            Err(error) => {
                trace_spawn_failure("stacks.acquire", None, &error);
                inner.active.fetch_sub(1, Ordering::AcqRel);
                return Err(error);
            }
        }
    } else {
        None
    };

    let id = inner.next_id.fetch_add(1, Ordering::AcqRel) as u64;
    let carrier = select_spawn_carrier(inner);
    let slot_index = match inner.tasks.reserve_slot() {
        Ok(slot_index) => slot_index,
        Err(error) => {
            trace_spawn_failure("tasks.reserve_slot", None, &error);
            if let Some(lease) = lease {
                let _ = inner.stacks.release(lease.pool_index, lease.slot_index);
            }
            inner.active.fetch_sub(1, Ordering::AcqRel);
            return Err(error);
        }
    };

    let context = match inner.tasks.slot_context(slot_index) {
        Ok(context) => context,
        Err(error) => {
            trace_spawn_failure("tasks.slot_context", Some(slot_index), &error);
            let _ = inner.tasks.abandon(slot_index, id);
            if let Some(lease) = lease {
                let _ = inner.stacks.release(lease.pool_index, lease.slot_index);
            }
            inner.active.fetch_sub(1, Ordering::AcqRel);
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

fn cleanup_failed_spawn_for(inner: &GreenPoolLease, reservation: &SpawnReservation) {
    let _ = inner.tasks.abandon(reservation.slot_index, reservation.id);
    if let Some(lease) = reservation.lease {
        let _ = inner.stacks.release(lease.pool_index, lease.slot_index);
    }
    inner.active.fetch_sub(1, Ordering::AcqRel);
}

fn spawn_on_lease<F, T>(
    inner: &GreenPoolLease,
    task: FiberTaskAttributes,
    job: F,
    class: fusion_sys::courier::CourierFiberClass,
    signal: bool,
    drive_mode: GreenHandleDriveMode,
    use_generated_closure_root: bool,
) -> Result<GreenHandle<T>, FiberError>
where
    F: FnOnce() -> T + Send + 'static,
    T: 'static,
{
    if !InlineGreenResultStorage::supports::<T>() {
        return Err(FiberError::unsupported());
    }

    let reservation = reserve_spawn_slot_for(inner, task)?;
    let fiber_id = next_green_fiber_id();
    let slot_addr = reservation.context as usize;
    let wrapped = move || {
        let output = if use_generated_closure_root {
            generated_closure_task_root(job)
        } else {
            job()
        };
        if size_of::<T>() == 0 {
            return;
        }
        let slot = unsafe { &*(slot_addr as *const GreenTaskSlot) };
        if let Ok(id) = slot.current_id()
            && slot.store_output(id, output).is_err()
        {
            let _ = slot.set_state(id, GreenTaskState::Failed(FiberError::state_conflict()));
        }
    };

    if let Err(error) = inner.tasks.assign_job(
        reservation.slot_index,
        reservation.id,
        fiber_id,
        class,
        reservation.carrier,
        reservation.lease,
        task,
        wrapped,
    ) {
        trace_spawn_failure("tasks.assign_job", Some(reservation.slot_index), &error);
        cleanup_failed_spawn_for(inner, &reservation);
        return Err(error);
    }

    if let Some(lease) = reservation.lease {
        if let Err(error) = inner.stacks.attach_slot_identity(
            lease.pool_index,
            lease.slot_index,
            reservation.id,
            reservation.carrier,
            inner.carriers[reservation.carrier].capacity_token(),
        ) {
            trace_spawn_failure(
                "stacks.attach_slot_identity",
                Some(reservation.slot_index),
                &error,
            );
            cleanup_failed_spawn_for(inner, &reservation);
            return Err(error);
        }

        if inner.support.context.migration == ContextMigrationSupport::CrossCarrier {
            let fiber = match Fiber::new(lease.stack, green_task_entry, reservation.context) {
                Ok(fiber) => fiber,
                Err(error) => {
                    trace_spawn_failure("Fiber::new", Some(reservation.slot_index), &error);
                    cleanup_failed_spawn_for(inner, &reservation);
                    return Err(error);
                }
            };

            if let Err(error) =
                inner
                    .tasks
                    .install_fiber(reservation.slot_index, reservation.id, fiber)
            {
                trace_spawn_failure("tasks.install_fiber", Some(reservation.slot_index), &error);
                cleanup_failed_spawn_for(inner, &reservation);
                return Err(error);
            }
        }
    }

    if let Err(error) =
        inner.enqueue_with_signal(reservation.carrier, reservation.slot_index, signal)
    {
        trace_spawn_failure("enqueue_with_signal", Some(reservation.slot_index), &error);
        cleanup_failed_spawn_for(inner, &reservation);
        return Err(error);
    }

    if let Err(error) = inner.register_runtime_fiber(fiber_id, reservation.id, class) {
        trace_spawn_failure(
            "runtime.register_runtime_fiber",
            Some(reservation.slot_index),
            &error,
        );
        cleanup_failed_spawn_for(inner, &reservation);
        return Err(error);
    }

    Ok(GreenHandle {
        id: reservation.id,
        slot_index: reservation.slot_index,
        inner: inner.try_clone()?,
        drive_mode,
        _marker: PhantomData,
    })
}

fn drive_current_pool_once(inner: &GreenPoolLease) -> Result<bool, FiberError> {
    let Some(slot_index) = dequeue_ready(inner, 0)? else {
        return Ok(false);
    };
    run_ready_task(inner, 0, slot_index)?;
    Ok(true)
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
