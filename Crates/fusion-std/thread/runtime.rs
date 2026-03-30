//! Domain 5: public runtime orchestration surface.

use super::{
    AsyncRuntimeMemoryFootprint,
    CurrentAsyncRuntime,
    CurrentAsyncRuntimeCombinedBackingPlan,
    CurrentFiberHandle,
    CurrentFiberPool,
    CurrentFiberPoolCombinedBackingPlan,
    Executor,
    ExecutorConfig,
    ExecutorError,
    ExecutorPlanningSupport,
    FiberPlanningSupport,
    FiberPoolBootstrap,
    FiberPoolMemoryFootprint,
    FiberStackBacking,
    FiberStackClass,
    GreenGrowth,
    GreenPool,
    GreenPoolConfig,
    GreenScheduling,
    TaskHandle,
    ThreadPool,
    ThreadPoolConfig,
    generated_default_fiber_stack_bytes,
};
use crate::sync::SyncErrorKind;
use core::cell::UnsafeCell;
use core::future::Future;
use core::hint::spin_loop;
use core::mem::{align_of, size_of};
use core::num::NonZeroUsize;
use core::ptr::NonNull;
use fusion_pal::sys::mem::MemBase;
use fusion_sys::alloc::{
    AllocError,
    AllocErrorKind,
    Allocator,
    ExtentLease,
    MemoryPoolExtentRequest,
};
use fusion_sys::fiber::{FiberError, FiberErrorKind, FiberSystem};
pub use fusion_sys::mem::resource::AllocatorLayoutPolicy;
use fusion_sys::mem::resource::{
    BoundMemoryResource,
    BoundResourceSpec,
    MemoryResource,
    MemoryResourceHandle,
    ResourceBackingKind,
    ResourceRange,
};
use fusion_sys::thread::{
    RuntimeBackingError,
    RuntimeBackingErrorKind,
    allocate_owned_runtime_slab,
    system_thread,
    uses_explicit_bound_runtime_backing,
};

/// Global sizing strategy for runtime-owned slabs, arenas, and derived envelopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeSizingStrategy {
    /// Use the exact counted requirement with no extra headroom.
    Exact,
    /// Add 20% headroom and round up to the next power-of-two envelope.
    GlobalNearestRoundUp,
}

impl RuntimeSizingStrategy {
    /// Returns the effective byte requirement for one counted backing size under this strategy.
    #[must_use]
    pub fn apply_bytes(self, bytes: usize) -> Option<usize> {
        match self {
            Self::Exact => Some(bytes),
            Self::GlobalNearestRoundUp => {
                if bytes == 0 {
                    return Some(0);
                }
                let extra = bytes.checked_add(4)? / 5;
                let padded = bytes.checked_add(extra)?;
                padded.checked_next_power_of_two()
            }
        }
    }
}

/// Returns the crate-wide default runtime sizing strategy selected by features.
#[must_use]
pub const fn default_runtime_sizing_strategy() -> RuntimeSizingStrategy {
    #[cfg(feature = "sizing-global-nearest-round-up")]
    {
        RuntimeSizingStrategy::GlobalNearestRoundUp
    }
    #[cfg(not(feature = "sizing-global-nearest-round-up"))]
    {
        RuntimeSizingStrategy::Exact
    }
}

/// Unified backing request for one combined runtime-owned slab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RuntimeBackingRequest {
    /// Requested byte count.
    pub bytes: usize,
    /// Required slab alignment.
    pub align: usize,
}

/// Explicit one-slab backing plan for one current-thread fiber + async runtime bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurrentFiberAsyncRuntimeBackingPlan {
    /// Total owning slab request for the combined runtime bundle.
    pub slab: RuntimeBackingRequest,
    /// Fiber-runtime sub-slab range inside the owning slab.
    pub fibers: ResourceRange,
    /// Async-runtime sub-slab range inside the owning slab.
    pub executor: ResourceRange,
    /// Nested current-thread fiber plan for the fiber sub-slab.
    pub fiber_plan: CurrentFiberPoolCombinedBackingPlan,
    /// Nested current-thread async plan for the async sub-slab.
    pub executor_plan: CurrentAsyncRuntimeCombinedBackingPlan,
}

/// Error for combined current-thread fiber + async bootstrap work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CurrentFiberAsyncRuntimeError {
    /// Fiber bootstrap failed honestly.
    Fiber(fusion_sys::fiber::FiberError),
    /// Async runtime bootstrap failed honestly.
    Executor(super::ExecutorError),
}

/// Ergonomic current-thread bootstrap for one fiber pool plus one async runtime.
#[derive(Debug, Clone, Copy)]
pub struct CurrentFiberAsyncBootstrap<'a> {
    fibers: FiberPoolBootstrap<'a>,
    executor: ExecutorConfig,
}

/// One current-thread runtime bundle containing both fibers and async.
#[derive(Debug)]
pub struct CurrentFiberAsyncRuntime {
    fibers: CurrentFiberPool,
    executor: CurrentAsyncRuntime,
}

fn selected_stack_size_with_optional_floor(
    stack_floor_bytes: Option<usize>,
) -> Result<NonZeroUsize, FiberError> {
    let requested = generated_default_fiber_stack_bytes()?.max(stack_floor_bytes.unwrap_or(0));
    let requested = NonZeroUsize::new(requested).ok_or_else(FiberError::invalid)?;
    Ok(FiberStackClass::from_stack_bytes(requested)?.size_bytes())
}

fn current_thread_default_guard_pages() -> usize {
    usize::from(FiberSystem::new().support().context.guard_required)
}

fn executor_error_from_fiber(error: FiberError) -> ExecutorError {
    match error.kind() {
        FiberErrorKind::Unsupported => ExecutorError::Unsupported,
        FiberErrorKind::Invalid => ExecutorError::Sync(SyncErrorKind::Invalid),
        FiberErrorKind::ResourceExhausted => ExecutorError::Sync(SyncErrorKind::Overflow),
        FiberErrorKind::DeadlineExceeded | FiberErrorKind::StateConflict => {
            ExecutorError::Sync(SyncErrorKind::Busy)
        }
        FiberErrorKind::Context(_) => ExecutorError::Sync(SyncErrorKind::Busy),
    }
}

fn fiber_error_from_executor(error: ExecutorError) -> FiberError {
    match error {
        ExecutorError::Unsupported => FiberError::unsupported(),
        ExecutorError::Stopped | ExecutorError::Cancelled => FiberError::state_conflict(),
        ExecutorError::Sync(kind) => match kind {
            SyncErrorKind::Invalid => FiberError::invalid(),
            SyncErrorKind::Overflow => FiberError::resource_exhausted(),
            _ => FiberError::state_conflict(),
        },
        ExecutorError::TaskPanicked => FiberError::state_conflict(),
    }
}

fn executor_error_from_current_runtime(error: CurrentFiberAsyncRuntimeError) -> ExecutorError {
    match error {
        CurrentFiberAsyncRuntimeError::Fiber(error) => executor_error_from_fiber(error),
        CurrentFiberAsyncRuntimeError::Executor(error) => error,
    }
}

fn executor_error_from_runtime_sync(error: crate::sync::SyncError) -> ExecutorError {
    match error.kind {
        SyncErrorKind::Unsupported => ExecutorError::Unsupported,
        SyncErrorKind::Invalid => ExecutorError::Sync(SyncErrorKind::Invalid),
        SyncErrorKind::Overflow => ExecutorError::Sync(SyncErrorKind::Overflow),
        SyncErrorKind::Busy | SyncErrorKind::PermissionDenied | SyncErrorKind::Platform(_) => {
            ExecutorError::Sync(SyncErrorKind::Busy)
        }
    }
}

fn executor_error_from_alloc(error: AllocError) -> ExecutorError {
    match error.kind {
        AllocErrorKind::Unsupported | AllocErrorKind::PolicyDenied => ExecutorError::Unsupported,
        AllocErrorKind::InvalidRequest | AllocErrorKind::InvalidDomain => executor_invalid(),
        AllocErrorKind::Busy | AllocErrorKind::SynchronizationFailure(_) => executor_busy(),
        AllocErrorKind::MetadataExhausted
        | AllocErrorKind::CapacityExhausted
        | AllocErrorKind::OutOfMemory
        | AllocErrorKind::ResourceFailure(_)
        | AllocErrorKind::PoolFailure(_) => executor_overflow(),
    }
}

const fn executor_invalid() -> ExecutorError {
    ExecutorError::Sync(SyncErrorKind::Invalid)
}

const fn executor_busy() -> ExecutorError {
    ExecutorError::Sync(SyncErrorKind::Busy)
}

const fn executor_overflow() -> ExecutorError {
    ExecutorError::Sync(SyncErrorKind::Overflow)
}

fn next_runtime_capacity(current: usize, required_minimum: usize) -> Result<usize, ExecutorError> {
    let mut next = current.max(1);
    if required_minimum <= next {
        return next.checked_mul(2).ok_or_else(executor_overflow);
    }
    while next < required_minimum {
        next = next.checked_mul(2).ok_or_else(executor_overflow)?;
    }
    Ok(next)
}

fn initial_runtime_capacity(
    limit: Option<usize>,
    requested_capacity: Option<usize>,
) -> Option<usize> {
    let requested = requested_capacity.unwrap_or(1).max(1);
    match limit {
        Some(limit) if requested > limit => None,
        Some(_) | None => Some(requested),
    }
}

fn next_bounded_runtime_capacity(
    current: usize,
    required_minimum: usize,
    limit: Option<usize>,
) -> Result<Option<usize>, ExecutorError> {
    if let Some(limit) = limit
        && required_minimum > limit
    {
        return Ok(None);
    }
    let next = if current == 0 {
        required_minimum.max(1)
    } else {
        next_runtime_capacity(current, required_minimum)?
    };
    Ok(Some(match limit {
        Some(limit) => next.min(limit),
        None => next,
    }))
}

#[derive(Debug)]
struct CurrentFiberRuntimeSlotState {
    runtime: Option<CurrentFiberPool>,
    configured_capacity: usize,
    effective_stack_floor_bytes: usize,
    borrows: usize,
}

struct CurrentFiberRuntimeSlot {
    lock: fusion_sys::sync::ThinMutex,
    state: UnsafeCell<CurrentFiberRuntimeSlotState>,
}

impl CurrentFiberRuntimeSlot {
    const fn new() -> Self {
        Self {
            lock: fusion_sys::sync::ThinMutex::new(),
            state: UnsafeCell::new(CurrentFiberRuntimeSlotState {
                runtime: None,
                configured_capacity: 0,
                effective_stack_floor_bytes: 0,
                borrows: 0,
            }),
        }
    }
}

// SAFETY: this slot only exposes thread-affine current-thread fiber state. Internal mutation is
// serialized through the thin mutex, and borrowed runtimes are pinned in place until the borrow
// count returns to zero.
unsafe impl Sync for CurrentFiberRuntimeSlot {}

struct CurrentFiberPoolBorrow<'a> {
    slot: &'a CurrentFiberRuntimeSlot,
    runtime: *const CurrentFiberPool,
    configured_capacity: usize,
}

impl CurrentFiberPoolBorrow<'_> {
    const fn configured_capacity(&self) -> usize {
        self.configured_capacity
    }
}

impl core::ops::Deref for CurrentFiberPoolBorrow<'_> {
    type Target = CurrentFiberPool;

    fn deref(&self) -> &Self::Target {
        // SAFETY: borrows increment the slot borrow count under lock, and the runtime is not
        // replaced while any borrow is live.
        unsafe { &*self.runtime }
    }
}

impl Drop for CurrentFiberPoolBorrow<'_> {
    fn drop(&mut self) {
        if let Ok(_guard) = self.slot.lock.lock() {
            // SAFETY: the thin mutex serializes mutation of the singleton fiber slot.
            let state = unsafe { &mut *self.slot.state.get() };
            debug_assert!(
                state.borrows != 0,
                "fiber runtime borrow count should not underflow"
            );
            if state.borrows != 0 {
                state.borrows -= 1;
            }
        }
    }
}

#[derive(Debug)]
struct CurrentAsyncRuntimeSegment {
    runtime: CurrentAsyncRuntime,
    next: Option<NonNull<CurrentAsyncRuntimeSegment>>,
    _backing: ExtentLease,
}

#[derive(Debug)]
struct CurrentAsyncRuntimeSlotState {
    head: Option<NonNull<CurrentAsyncRuntimeSegment>>,
    total_capacity: usize,
}

struct CurrentAsyncRuntimeSlot {
    lock: fusion_sys::sync::ThinMutex,
    state: UnsafeCell<CurrentAsyncRuntimeSlotState>,
}

impl CurrentAsyncRuntimeSlot {
    const fn new() -> Self {
        Self {
            lock: fusion_sys::sync::ThinMutex::new(),
            state: UnsafeCell::new(CurrentAsyncRuntimeSlotState {
                head: None,
                total_capacity: 0,
            }),
        }
    }
}

// SAFETY: this slot only exposes thread-affine current-thread async state. Internal mutation is
// serialized through the thin mutex, and appended segments remain stable for the singleton's life.
unsafe impl Sync for CurrentAsyncRuntimeSlot {}

/// Tiny lazy current-thread fiber+async runtime façade for board-local front doors.
///
/// This exists for targets that want a `thread::spawn()`-clean consumer surface while the exact
/// generated metadata pipeline still depends on build-time sidecars rather than compiler-native
/// artifacts.
pub struct CurrentFiberAsyncSingleton {
    fiber_capacity_limit: Option<usize>,
    async_capacity_limit: Option<usize>,
    stack_floor_bytes: Option<usize>,
    guard_pages: Option<usize>,
    fibers: CurrentFiberRuntimeSlot,
    executor: CurrentAsyncRuntimeSlot,
}

impl CurrentFiberAsyncSingleton {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            fiber_capacity_limit: None,
            async_capacity_limit: None,
            stack_floor_bytes: None,
            guard_pages: None,
            fibers: CurrentFiberRuntimeSlot::new(),
            executor: CurrentAsyncRuntimeSlot::new(),
        }
    }

    /// Installs one explicit current-thread fiber capacity cap.
    ///
    /// This is runtime policy only. It does not describe backend structural minimums.
    #[must_use]
    pub const fn with_fiber_capacity(mut self, fiber_capacity: usize) -> Self {
        self.fiber_capacity_limit = Some(if fiber_capacity == 0 {
            1
        } else {
            fiber_capacity
        });
        self
    }

    /// Installs one explicit current-thread async task capacity cap.
    ///
    /// This is runtime policy only. It does not describe backend structural minimums.
    #[must_use]
    pub const fn with_async_capacity(mut self, async_capacity: usize) -> Self {
        self.async_capacity_limit = Some(if async_capacity == 0 {
            1
        } else {
            async_capacity
        });
        self
    }

    /// Installs one optional policy floor above the machine- and metadata-derived stack minimum.
    ///
    /// This is not backend truth. It exists only as an escape hatch while generated task metadata
    /// still stops at crate boundaries.
    #[must_use]
    pub const fn with_stack_floor(mut self, stack_floor_bytes: usize) -> Self {
        self.stack_floor_bytes = Some(stack_floor_bytes);
        self
    }

    #[must_use]
    pub const fn with_guard_pages(mut self, guard_pages: usize) -> Self {
        self.guard_pages = Some(guard_pages);
        self
    }

    fn fiber_bootstrap_with_policy(
        &self,
        fiber_capacity: usize,
        stack_floor_bytes: usize,
    ) -> Result<FiberPoolBootstrap<'static>, FiberError> {
        let guard_pages = match self.guard_pages {
            Some(guard_pages) => guard_pages,
            None => current_thread_default_guard_pages(),
        };
        FiberPoolBootstrap::uniform_growing(
            selected_stack_size_with_optional_floor(Some(stack_floor_bytes))?,
            fiber_capacity,
            1,
        )
        .map(|bootstrap| {
            bootstrap
                .with_guard_pages(guard_pages)
                .with_sizing_strategy(default_runtime_sizing_strategy())
        })
    }

    fn build_fiber_runtime_with_policy(
        &self,
        fiber_capacity: usize,
        stack_floor_bytes: usize,
    ) -> Result<CurrentFiberPool, FiberError> {
        self.fiber_bootstrap_with_policy(fiber_capacity, stack_floor_bytes)?
            .build_current()
    }

    fn build_async_runtime_with_policy(
        &self,
        async_capacity: usize,
    ) -> Result<CurrentAsyncRuntime, ExecutorError> {
        let config = ExecutorConfig::new().with_capacity(async_capacity.max(1));
        if uses_explicit_bound_runtime_backing() {
            let layout = CurrentAsyncRuntime::backing_plan(config)?;
            let combined = layout.combined_eager()?;
            if let Some(slab) =
                allocate_owned_runtime_slab(combined.slab.bytes, combined.slab.align)
                    .map_err(current_runtime_error_from_owned_backing)
                    .map_err(executor_error_from_current_runtime)?
            {
                return CurrentAsyncRuntime::from_owned_extent(config, slab.lease);
            }
        }
        Ok(CurrentAsyncRuntime::with_executor_config(config))
    }

    fn allocate_runtime_node_backing(
        bytes: usize,
        align: usize,
    ) -> Result<ExtentLease, ExecutorError> {
        if let Some(slab) = allocate_owned_runtime_slab(bytes, align)
            .map_err(current_runtime_error_from_owned_backing)
            .map_err(executor_error_from_current_runtime)?
        {
            return Ok(slab.lease);
        }
        let allocator = Allocator::<1, 1>::system_default_with_capacity(bytes)
            .map_err(executor_error_from_alloc)?;
        let domain = allocator.default_domain().ok_or_else(executor_invalid)?;
        allocator
            .extent(domain, MemoryPoolExtentRequest { len: bytes, align })
            .map_err(executor_error_from_alloc)
    }

    fn fiber_runtime_borrow(
        &'static self,
        requested_capacity: Option<usize>,
        requested_stack_floor_bytes: Option<usize>,
    ) -> Result<CurrentFiberPoolBorrow<'static>, FiberError> {
        let _guard =
            self.fibers.lock.lock().map_err(|error| {
                fiber_error_from_executor(executor_error_from_runtime_sync(error))
            })?;
        // SAFETY: the thin mutex serializes access to the singleton fiber slot state.
        let state = unsafe { &mut *self.fibers.state.get() };
        if state.runtime.is_none() {
            state.configured_capacity =
                initial_runtime_capacity(self.fiber_capacity_limit, requested_capacity)
                    .ok_or_else(FiberError::resource_exhausted)?;
            state.effective_stack_floor_bytes = self
                .stack_floor_bytes
                .unwrap_or(0)
                .max(requested_stack_floor_bytes.unwrap_or(0));
            state.runtime = Some(self.build_fiber_runtime_with_policy(
                state.configured_capacity,
                state.effective_stack_floor_bytes,
            )?);
        }
        state.borrows = state
            .borrows
            .checked_add(1)
            .ok_or_else(FiberError::resource_exhausted)?;
        let runtime = state
            .runtime
            .as_ref()
            .map(core::ptr::from_ref)
            .ok_or_else(FiberError::invalid)?;
        Ok(CurrentFiberPoolBorrow {
            slot: &self.fibers,
            runtime,
            configured_capacity: state.configured_capacity,
        })
    }

    fn fiber_runtime_if_initialized(
        &'static self,
    ) -> Result<Option<CurrentFiberPoolBorrow<'static>>, FiberError> {
        let _guard =
            self.fibers.lock.lock().map_err(|error| {
                fiber_error_from_executor(executor_error_from_runtime_sync(error))
            })?;
        // SAFETY: the thin mutex serializes access to the singleton fiber slot state.
        let state = unsafe { &mut *self.fibers.state.get() };
        let Some(runtime) = state.runtime.as_ref().map(core::ptr::from_ref) else {
            return Ok(None);
        };
        state.borrows = state
            .borrows
            .checked_add(1)
            .ok_or_else(FiberError::resource_exhausted)?;
        Ok(Some(CurrentFiberPoolBorrow {
            slot: &self.fibers,
            runtime,
            configured_capacity: state.configured_capacity,
        }))
    }

    fn try_reconfigure_fiber_runtime(
        &'static self,
        requested_fiber_capacity: Option<usize>,
        requested_stack_floor_bytes: Option<usize>,
    ) -> Result<bool, FiberError> {
        let _guard =
            self.fibers.lock.lock().map_err(|error| {
                fiber_error_from_executor(executor_error_from_runtime_sync(error))
            })?;
        // SAFETY: the thin mutex serializes access to the singleton fiber slot state.
        let state = unsafe { &mut *self.fibers.state.get() };

        if state.borrows != 0 {
            return Ok(false);
        }

        if let Some(runtime) = state.runtime.as_ref()
            && runtime.active_count() != 0
        {
            return Ok(false);
        }

        let next_fiber_capacity = match requested_fiber_capacity {
            Some(required) if required > state.configured_capacity => {
                next_bounded_runtime_capacity(
                    state.configured_capacity,
                    required,
                    self.fiber_capacity_limit,
                )
                .map_err(fiber_error_from_executor)?
                .ok_or_else(FiberError::resource_exhausted)?
            }
            _ if state.runtime.is_none() => {
                initial_runtime_capacity(self.fiber_capacity_limit, requested_fiber_capacity)
                    .ok_or_else(FiberError::resource_exhausted)?
            }
            _ => state.configured_capacity,
        };
        let next_stack_floor_bytes = state
            .effective_stack_floor_bytes
            .max(requested_stack_floor_bytes.unwrap_or(0));

        if state.runtime.is_some()
            && next_fiber_capacity == state.configured_capacity
            && next_stack_floor_bytes == state.effective_stack_floor_bytes
        {
            return Ok(false);
        }

        state.runtime = Some(
            self.build_fiber_runtime_with_policy(next_fiber_capacity, next_stack_floor_bytes)?,
        );
        state.configured_capacity = next_fiber_capacity;
        state.effective_stack_floor_bytes = next_stack_floor_bytes;
        Ok(true)
    }

    fn ensure_fiber_admission(
        &'static self,
        task: super::FiberTaskAttributes,
    ) -> Result<(), FiberError> {
        loop {
            let requested_stack_floor_bytes = task
                .execution
                .is_fiber()
                .then(|| task.stack_class.size_bytes().get());
            let runtime = self.fiber_runtime_borrow(None, requested_stack_floor_bytes)?;
            let validation = runtime.validate_task_attributes(task);
            let available_slots = match validation {
                Ok(()) => runtime.available_slots()?,
                Err(_) => 0,
            };
            if validation.is_ok() && available_slots != 0 {
                return Ok(());
            }
            let requested_fiber_capacity =
                (available_slots == 0).then(|| runtime.configured_capacity().saturating_add(1));
            drop(runtime);
            let requested_stack_floor_bytes = match validation {
                Ok(()) => None,
                Err(error)
                    if error.kind() == FiberErrorKind::Unsupported && task.execution.is_fiber() =>
                {
                    requested_stack_floor_bytes
                }
                Err(error) => return Err(error),
            };
            if !self.try_reconfigure_fiber_runtime(
                requested_fiber_capacity,
                requested_stack_floor_bytes,
            )? {
                return Err(match requested_stack_floor_bytes {
                    Some(_) => FiberError::unsupported(),
                    None => FiberError::resource_exhausted(),
                });
            }
        }
    }

    fn next_async_segment_capacity(&self, total_capacity: usize) -> Option<usize> {
        let requested = if total_capacity == 0 {
            1
        } else {
            total_capacity
        };
        match self.async_capacity_limit {
            Some(limit) if total_capacity >= limit => None,
            Some(limit) => Some(requested.min(limit - total_capacity)),
            None => Some(requested),
        }
    }

    fn append_async_runtime_segment(
        &self,
        state: &mut CurrentAsyncRuntimeSlotState,
        capacity: usize,
    ) -> Result<NonNull<CurrentAsyncRuntimeSegment>, ExecutorError> {
        let runtime = self.build_async_runtime_with_policy(capacity)?;
        let backing = Self::allocate_runtime_node_backing(
            size_of::<CurrentAsyncRuntimeSegment>(),
            align_of::<CurrentAsyncRuntimeSegment>(),
        )?;
        let ptr = NonNull::new(backing.region().base.cast::<CurrentAsyncRuntimeSegment>())
            .ok_or_else(executor_invalid)?;
        // SAFETY: the leased backing is unique, correctly aligned for the node type, and large
        // enough to host exactly one segment node.
        unsafe {
            ptr.as_ptr().write(CurrentAsyncRuntimeSegment {
                runtime,
                next: state.head,
                _backing: backing,
            });
        }
        state.head = Some(ptr);
        state.total_capacity = state
            .total_capacity
            .checked_add(capacity)
            .ok_or_else(executor_overflow)?;
        Ok(ptr)
    }

    fn async_runtime_for_spawn(
        &'static self,
    ) -> Result<&'static CurrentAsyncRuntime, ExecutorError> {
        let _guard = self
            .executor
            .lock
            .lock()
            .map_err(executor_error_from_runtime_sync)?;
        // SAFETY: the thin mutex serializes access to the singleton async slot state. Segment
        // nodes are append-only and remain stable for the singleton's lifetime.
        let state = unsafe { &mut *self.executor.state.get() };

        let mut segment = state.head;
        while let Some(node) = segment {
            // SAFETY: append-only segment nodes remain live and stable while linked from the slot.
            let node_ref = unsafe { node.as_ref() };
            if node_ref.runtime.available_task_slots()? != 0 {
                return Ok(&node_ref.runtime);
            }
            segment = node_ref.next;
        }

        let capacity = self
            .next_async_segment_capacity(state.total_capacity)
            .ok_or_else(executor_busy)?;
        let node = self.append_async_runtime_segment(state, capacity)?;
        // SAFETY: the newly appended node is now linked into the append-only segment chain.
        Ok(unsafe { &node.as_ref().runtime })
    }

    fn async_segment_head_snapshot(
        &'static self,
    ) -> Result<Option<NonNull<CurrentAsyncRuntimeSegment>>, ExecutorError> {
        let _guard = self
            .executor
            .lock
            .lock()
            .map_err(executor_error_from_runtime_sync)?;
        // SAFETY: the thin mutex serializes access to the singleton async slot state. Segment
        // nodes are append-only and remain stable for the singleton's lifetime.
        let state = unsafe { &*self.executor.state.get() };
        Ok(state.head)
    }

    /// Drives one ready async task across every realized singleton async segment.
    ///
    /// # Errors
    ///
    /// Returns any honest executor failure from the realized segments.
    pub fn drive_async_once(&'static self) -> Result<bool, ExecutorError> {
        let mut segment = self.async_segment_head_snapshot()?;
        let mut progressed = false;
        while let Some(node) = segment {
            // SAFETY: append-only segment nodes remain live and stable while linked from the slot.
            let node_ref = unsafe { node.as_ref() };
            progressed |= node_ref.runtime.drive_once()?;
            segment = node_ref.next;
        }
        Ok(progressed)
    }

    /// Drains every realized singleton async segment until no task remains runnable.
    ///
    /// # Errors
    ///
    /// Returns any honest executor failure from the realized segments.
    pub fn run_async_until_idle(&'static self) -> Result<usize, ExecutorError> {
        let mut total = 0_usize;
        loop {
            let mut segment = self.async_segment_head_snapshot()?;
            let mut progressed = 0_usize;
            while let Some(node) = segment {
                // SAFETY: append-only segment nodes remain live and stable while linked from the
                // singleton slot.
                let node_ref = unsafe { node.as_ref() };
                progressed = progressed.saturating_add(node_ref.runtime.run_until_idle()?);
                segment = node_ref.next;
            }
            if progressed == 0 {
                return Ok(total);
            }
            total = total.saturating_add(progressed);
        }
    }

    fn drive_async_reactors_once(&'static self, wait: bool) -> Result<bool, ExecutorError> {
        let mut segment = self.async_segment_head_snapshot()?;
        let mut progressed = false;
        while let Some(node) = segment {
            // SAFETY: append-only segment nodes remain live and stable while linked from the slot.
            let node_ref = unsafe { node.as_ref() };
            let segment_wait =
                wait && !progressed && node_ref.runtime.unfinished_task_count()? != 0;
            progressed |= node_ref.runtime.drive_reactor_once(segment_wait)?;
            segment = node_ref.next;
        }
        Ok(progressed)
    }

    #[cfg(test)]
    fn async_total_capacity(&'static self) -> Result<usize, ExecutorError> {
        let _guard = self
            .executor
            .lock
            .lock()
            .map_err(executor_error_from_runtime_sync)?;
        // SAFETY: the thin mutex serializes access to the singleton async slot state.
        let state = unsafe { &*self.executor.state.get() };
        Ok(state.total_capacity)
    }

    pub fn spawn_fiber<F, T>(&'static self, job: F) -> Result<CurrentFiberHandle<T>, FiberError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: 'static,
    {
        let task = self
            .fiber_runtime_borrow(None, None)?
            .closure_task_attributes::<F>()?;
        self.ensure_fiber_admission(task)?;
        self.fiber_runtime_borrow(
            None,
            task.execution
                .is_fiber()
                .then(|| task.stack_class.size_bytes().get()),
        )?
        .spawn_with_attrs(task, job)
    }

    pub fn spawn_fiber_with_stack<const STACK_BYTES: usize, F, T>(
        &'static self,
        job: F,
    ) -> Result<CurrentFiberHandle<T>, FiberError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: 'static,
    {
        let stack_bytes = NonZeroUsize::new(STACK_BYTES).ok_or_else(FiberError::invalid)?;
        let task = super::FiberTaskAttributes::from_stack_bytes(
            stack_bytes,
            super::FiberTaskPriority::DEFAULT,
        )?;
        self.ensure_fiber_admission(task)?;
        self.fiber_runtime_borrow(None, Some(stack_bytes.get()))?
            .spawn_with_attrs(task, job)
    }

    pub fn drive_once(&'static self) -> Result<bool, FiberError> {
        self.fiber_runtime_if_initialized()?
            .map_or(Ok(false), |runtime| runtime.drive_once())
    }

    /// Spawns one async task through the lazily owned current-thread runtime.
    ///
    /// # Errors
    ///
    /// Returns any honest executor failure, including `Unsupported` when the future has no
    /// generated poll-stack metadata and the caller did not supply one explicit contract.
    pub fn spawn_async<F>(&'static self, future: F) -> Result<TaskHandle<F::Output>, ExecutorError>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.async_runtime_for_spawn()?.spawn(future)
    }

    /// Spawns one async task with one explicit poll-stack contract through the lazily owned
    /// current-thread runtime.
    ///
    /// # Errors
    ///
    /// Returns any honest executor failure.
    pub fn spawn_async_with_poll_stack_bytes<F>(
        &'static self,
        poll_stack_bytes: usize,
        future: F,
    ) -> Result<TaskHandle<F::Output>, ExecutorError>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.async_runtime_for_spawn()?
            .spawn_with_poll_stack_bytes(poll_stack_bytes, future)
    }

    /// Drives one future to completion through the lazily owned current-thread runtime.
    ///
    /// # Errors
    ///
    /// Returns any honest executor failure, including `Unsupported` when the future has no
    /// generated poll-stack metadata and the caller did not supply one explicit contract.
    pub fn block_on<F>(&'static self, future: F) -> Result<F::Output, ExecutorError>
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        let handle = self.async_runtime_for_spawn()?.spawn_local(future)?;
        while !handle.is_finished()? {
            if !self.drive_async_once()?
                && !self.drive_async_reactors_once(true)?
                && system_thread().yield_now().is_err()
            {
                spin_loop();
            }
        }
        handle.join()
    }

    /// Drives one future to completion with one explicit poll-stack contract through the lazily
    /// owned current-thread runtime.
    ///
    /// # Errors
    ///
    /// Returns any honest executor failure.
    pub fn block_on_with_poll_stack_bytes<F>(
        &'static self,
        poll_stack_bytes: usize,
        future: F,
    ) -> Result<F::Output, ExecutorError>
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        let handle = self
            .async_runtime_for_spawn()?
            .spawn_local_with_poll_stack_bytes(poll_stack_bytes, future)?;
        while !handle.is_finished()? {
            if !self.drive_async_once()?
                && !self.drive_async_reactors_once(true)?
                && system_thread().yield_now().is_err()
            {
                spin_loop();
            }
        }
        handle.join()
    }

    pub fn shutdown_fibers(&'static self) -> Result<(), FiberError> {
        self.fiber_runtime_if_initialized()?
            .map_or(Ok(()), |runtime| runtime.shutdown())
    }
}

/// Exact configured memory footprint for one combined current-thread fiber + async bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurrentFiberAsyncRuntimeMemoryFootprint {
    /// Fiber-pool footprint.
    pub fibers: FiberPoolMemoryFootprint,
    /// Async-runtime footprint.
    pub executor: AsyncRuntimeMemoryFootprint,
}

impl CurrentFiberAsyncRuntimeMemoryFootprint {
    /// Returns the total configured bytes reserved by the combined runtime.
    #[must_use]
    pub const fn total_bytes(self) -> usize {
        self.fibers.total_bytes() + self.executor.total_bytes()
    }
}

/// One deferred current-thread async runtime builder.
#[derive(Debug)]
pub struct CurrentAsyncRuntimeBuilder {
    config: ExecutorConfig,
    slab: Option<MemoryResourceHandle>,
    owned_backing: Option<ExtentLease>,
}

/// Split current-thread fiber + async bootstrap result.
#[derive(Debug)]
pub struct CurrentFiberAsyncParts {
    fibers: CurrentFiberPool,
    executor: CurrentAsyncRuntimeBuilder,
}

impl CurrentFiberAsyncRuntime {
    /// Returns the current-thread fiber pool.
    #[must_use]
    pub const fn fibers(&self) -> &CurrentFiberPool {
        &self.fibers
    }

    /// Returns the current-thread async runtime.
    #[must_use]
    pub const fn executor(&self) -> &CurrentAsyncRuntime {
        &self.executor
    }

    /// Consumes the bundle into its component runtimes.
    #[must_use]
    pub fn into_parts(self) -> (CurrentFiberPool, CurrentAsyncRuntime) {
        (self.fibers, self.executor)
    }

    /// Returns the exact configured memory footprint for this combined runtime bundle.
    ///
    /// This is the selected-target planning view of the bundle's owned fiber and async domains.
    /// It intentionally describes configured backing shape, not transient live queue occupancy.
    ///
    /// # Errors
    ///
    /// Returns any honest async sizing failure while materializing the configured executor view.
    pub fn configured_memory_footprint(
        &self,
    ) -> Result<CurrentFiberAsyncRuntimeMemoryFootprint, CurrentFiberAsyncRuntimeError> {
        Ok(CurrentFiberAsyncRuntimeMemoryFootprint {
            fibers: self.fibers.memory_footprint(),
            executor: self.executor.configured_memory_footprint()?,
        })
    }
}

impl CurrentAsyncRuntimeBuilder {
    /// Builds the current-thread async runtime.
    ///
    /// # Errors
    ///
    /// Returns any honest current-thread async bootstrap failure.
    pub fn build(self) -> Result<CurrentAsyncRuntime, CurrentFiberAsyncRuntimeError> {
        match (self.slab, self.owned_backing) {
            (Some(slab), _) => Ok(CurrentAsyncRuntime::from_bound_slab(self.config, slab)?),
            (None, Some(owned_backing)) => Ok(CurrentAsyncRuntime::from_owned_extent(
                self.config,
                owned_backing,
            )?),
            (None, None) => Ok(CurrentAsyncRuntime::with_executor_config(self.config)),
        }
    }

    /// Builds the current-thread async runtime from one explicit owning slab.
    ///
    /// This is the bare-metal honest path for callers that already know they are on the
    /// explicit-backed lane and do not want the platform-acquired fallback path retained in the
    /// resulting image.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` if this builder does not carry one explicit backing slab, or any
    /// honest explicit-backed current-thread async bootstrap failure.
    pub fn build_explicit(self) -> Result<CurrentAsyncRuntime, CurrentFiberAsyncRuntimeError> {
        match (self.slab, self.owned_backing) {
            (Some(slab), _) => Ok(CurrentAsyncRuntime::from_bound_slab(self.config, slab)?),
            (None, Some(owned_backing)) => Ok(CurrentAsyncRuntime::from_owned_extent(
                self.config,
                owned_backing,
            )?),
            (None, None) => Err(CurrentFiberAsyncRuntimeError::Executor(
                super::ExecutorError::Unsupported,
            )),
        }
    }
}

impl CurrentFiberAsyncParts {
    /// Returns the current-thread fiber pool.
    #[must_use]
    pub const fn fibers(&self) -> &CurrentFiberPool {
        &self.fibers
    }

    /// Returns the deferred async runtime builder.
    #[must_use]
    pub const fn executor(&self) -> &CurrentAsyncRuntimeBuilder {
        &self.executor
    }

    /// Consumes the split bootstrap result into its component parts.
    #[must_use]
    pub fn into_parts(self) -> (CurrentFiberPool, CurrentAsyncRuntimeBuilder) {
        (self.fibers, self.executor)
    }
}

impl From<fusion_sys::fiber::FiberError> for CurrentFiberAsyncRuntimeError {
    fn from(value: fusion_sys::fiber::FiberError) -> Self {
        Self::Fiber(value)
    }
}

impl From<super::ExecutorError> for CurrentFiberAsyncRuntimeError {
    fn from(value: super::ExecutorError) -> Self {
        Self::Executor(value)
    }
}

fn runtime_align_up_packed(
    offset: usize,
    align: usize,
) -> Result<usize, CurrentFiberAsyncRuntimeError> {
    if align == 0 || !align.is_power_of_two() {
        return Err(CurrentFiberAsyncRuntimeError::Fiber(
            fusion_sys::fiber::FiberError::invalid(),
        ));
    }
    let mask = align - 1;
    offset
        .checked_add(mask)
        .map(|value| value & !mask)
        .ok_or(CurrentFiberAsyncRuntimeError::Fiber(
            fusion_sys::fiber::FiberError::resource_exhausted(),
        ))
}

const fn runtime_base_alignment_from_addr(addr: usize) -> usize {
    if addr == 0 {
        1
    } else {
        1usize << addr.trailing_zeros()
    }
}

fn runtime_resource_base_alignment(handle: &MemoryResourceHandle) -> usize {
    runtime_base_alignment_from_addr(handle.view().base_addr().get())
}

const fn runtime_partition_backing_kind(
    kind: ResourceBackingKind,
) -> Result<ResourceBackingKind, CurrentFiberAsyncRuntimeError> {
    match kind {
        ResourceBackingKind::Borrowed
        | ResourceBackingKind::StaticRegion
        | ResourceBackingKind::Partition => Ok(ResourceBackingKind::Partition),
        _ => Err(CurrentFiberAsyncRuntimeError::Executor(
            super::ExecutorError::Unsupported,
        )),
    }
}

fn partition_runtime_bound_resource(
    handle: &MemoryResourceHandle,
    range: ResourceRange,
) -> Result<MemoryResourceHandle, CurrentFiberAsyncRuntimeError> {
    let info = match handle {
        MemoryResourceHandle::Bound(resource) => resource.info(),
        MemoryResourceHandle::Virtual(_) => {
            return Err(CurrentFiberAsyncRuntimeError::Executor(
                super::ExecutorError::Unsupported,
            ));
        }
    };
    let region = handle
        .subview(range)
        .map(|view| unsafe { view.raw_region() })
        .map_err(|_| CurrentFiberAsyncRuntimeError::Executor(super::ExecutorError::Unsupported))?;
    let resource = BoundMemoryResource::new(BoundResourceSpec::new(
        region,
        info.domain,
        runtime_partition_backing_kind(info.backing)?,
        info.attrs,
        info.geometry,
        info.layout,
        info.contract,
        info.support,
        handle.state(),
    ))
    .map_err(|_| CurrentFiberAsyncRuntimeError::Executor(super::ExecutorError::Unsupported))?;
    Ok(MemoryResourceHandle::from(resource))
}

impl CurrentFiberAsyncBootstrap<'static> {
    /// Returns one deterministic bootstrap using the largest generated fiber stack contract.
    ///
    /// # Errors
    ///
    /// Returns an error when generated fiber stack metadata is unavailable.
    pub fn auto(
        max_fibers: usize,
        async_capacity: usize,
    ) -> Result<Self, CurrentFiberAsyncRuntimeError> {
        let fibers = FiberPoolBootstrap::auto(max_fibers)?;
        Ok(Self::from_parts(
            fibers,
            ExecutorConfig::new().with_capacity(async_capacity),
        ))
    }

    /// Returns one deterministic bootstrap with one explicit uniform fiber stack size.
    #[must_use]
    pub const fn uniform(
        max_fibers: usize,
        stack_size: NonZeroUsize,
        async_capacity: usize,
    ) -> Self {
        Self::from_parts(
            FiberPoolBootstrap::uniform(max_fibers, stack_size),
            ExecutorConfig::new().with_capacity(async_capacity),
        )
    }
}

impl<'a> CurrentFiberAsyncBootstrap<'a> {
    /// Returns one bootstrap from already-built fiber and executor configurations.
    #[must_use]
    pub const fn from_parts(fibers: FiberPoolBootstrap<'a>, executor: ExecutorConfig) -> Self {
        Self { fibers, executor }
    }

    /// Returns the fiber bootstrap half.
    #[must_use]
    pub const fn fibers(&self) -> &FiberPoolBootstrap<'a> {
        &self.fibers
    }

    /// Returns the async executor configuration half.
    #[must_use]
    pub const fn executor_config(&self) -> ExecutorConfig {
        self.executor
    }

    /// Applies one sizing strategy to both fiber and async sizing lanes.
    #[must_use]
    pub const fn with_sizing_strategy(mut self, sizing: RuntimeSizingStrategy) -> Self {
        self.fibers =
            FiberPoolBootstrap::from_config(self.fibers.config().with_sizing_strategy(sizing));
        self.executor = self.executor.with_sizing_strategy(sizing);
        self
    }

    /// Applies one sizing strategy only to the fiber half.
    #[must_use]
    pub const fn with_fiber_sizing_strategy(mut self, sizing: RuntimeSizingStrategy) -> Self {
        self.fibers =
            FiberPoolBootstrap::from_config(self.fibers.config().with_sizing_strategy(sizing));
        self
    }

    /// Applies one sizing strategy only to the async half.
    #[must_use]
    pub const fn with_async_sizing_strategy(mut self, sizing: RuntimeSizingStrategy) -> Self {
        self.executor = self.executor.with_sizing_strategy(sizing);
        self
    }

    /// Applies one explicit fiber guard-page count.
    #[must_use]
    pub const fn with_guard_pages(mut self, guard_pages: usize) -> Self {
        self.fibers =
            FiberPoolBootstrap::from_config(self.fibers.config().with_guard_pages(guard_pages));
        self
    }

    /// Returns the one-slab backing plan for this combined current-thread runtime bundle.
    ///
    /// # Errors
    ///
    /// Returns any honest fiber/async sizing or partitioning failure.
    pub fn backing_plan(
        self,
    ) -> Result<CurrentFiberAsyncRuntimeBackingPlan, CurrentFiberAsyncRuntimeError> {
        self.backing_plan_with_allocator_layout_policy(AllocatorLayoutPolicy::hosted_vm(
            fusion_pal::sys::mem::system_mem().page_info().alloc_granule,
        ))
    }

    /// Returns the one-slab backing plan for a caller that can guarantee at least
    /// `base_align` alignment for the owning slab base.
    ///
    /// # Errors
    ///
    /// Returns any honest fiber/async sizing or partitioning failure.
    pub fn backing_plan_for_base_alignment(
        self,
        base_align: usize,
    ) -> Result<CurrentFiberAsyncRuntimeBackingPlan, CurrentFiberAsyncRuntimeError> {
        self.backing_plan_for_base_alignment_with_allocator_layout_policy(
            base_align,
            AllocatorLayoutPolicy::hosted_vm(
                fusion_pal::sys::mem::system_mem().page_info().alloc_granule,
            ),
        )
    }

    /// Returns the one-slab backing plan under one explicit allocator layout policy.
    pub fn backing_plan_with_allocator_layout_policy(
        self,
        layout_policy: AllocatorLayoutPolicy,
    ) -> Result<CurrentFiberAsyncRuntimeBackingPlan, CurrentFiberAsyncRuntimeError> {
        self.backing_plan_for_base_alignment_with_allocator_layout_policy(1, layout_policy)
    }

    /// Returns the one-slab backing plan under one explicit fiber-planning surface and allocator
    /// layout policy.
    pub fn backing_plan_with_fiber_planning_support_and_allocator_layout_policy(
        self,
        fiber_planning: FiberPlanningSupport,
        layout_policy: AllocatorLayoutPolicy,
    ) -> Result<CurrentFiberAsyncRuntimeBackingPlan, CurrentFiberAsyncRuntimeError> {
        self.backing_plan_with_planning_support_and_allocator_layout_policy(
            fiber_planning,
            ExecutorPlanningSupport::selected_target(),
            layout_policy,
        )
    }

    /// Returns the one-slab backing plan under explicit fiber/executor planning surfaces and one
    /// explicit allocator layout policy.
    pub fn backing_plan_with_planning_support_and_allocator_layout_policy(
        self,
        fiber_planning: FiberPlanningSupport,
        executor_planning: ExecutorPlanningSupport,
        layout_policy: AllocatorLayoutPolicy,
    ) -> Result<CurrentFiberAsyncRuntimeBackingPlan, CurrentFiberAsyncRuntimeError> {
        self.backing_plan_for_base_alignment_with_planning_support_and_allocator_layout_policy(
            1,
            fiber_planning,
            executor_planning,
            layout_policy,
        )
    }

    /// Returns the one-slab backing plan for a caller that can guarantee at least `base_align`
    /// alignment under one explicit allocator layout policy.
    pub fn backing_plan_for_base_alignment_with_allocator_layout_policy(
        self,
        base_align: usize,
        layout_policy: AllocatorLayoutPolicy,
    ) -> Result<CurrentFiberAsyncRuntimeBackingPlan, CurrentFiberAsyncRuntimeError> {
        self.backing_plan_for_base_alignment_with_planning_support_and_allocator_layout_policy(
            base_align,
            FiberPlanningSupport::selected_runtime(),
            ExecutorPlanningSupport::selected_target(),
            layout_policy,
        )
    }

    /// Returns the one-slab backing plan for a caller that can guarantee at least `base_align`
    /// alignment under one explicit fiber-planning surface and allocator layout policy.
    pub fn backing_plan_for_base_alignment_with_fiber_planning_support_and_allocator_layout_policy(
        self,
        base_align: usize,
        fiber_planning: FiberPlanningSupport,
        layout_policy: AllocatorLayoutPolicy,
    ) -> Result<CurrentFiberAsyncRuntimeBackingPlan, CurrentFiberAsyncRuntimeError> {
        self.backing_plan_for_base_alignment_with_planning_support_and_allocator_layout_policy(
            base_align,
            fiber_planning,
            ExecutorPlanningSupport::selected_target(),
            layout_policy,
        )
    }

    /// Returns the one-slab backing plan for a caller that can guarantee at least `base_align`
    /// alignment under explicit fiber/executor planning surfaces and one allocator layout policy.
    pub fn backing_plan_for_base_alignment_with_planning_support_and_allocator_layout_policy(
        self,
        base_align: usize,
        fiber_planning: FiberPlanningSupport,
        executor_planning: ExecutorPlanningSupport,
        layout_policy: AllocatorLayoutPolicy,
    ) -> Result<CurrentFiberAsyncRuntimeBackingPlan, CurrentFiberAsyncRuntimeError> {
        let fiber_plan = CurrentFiberPool::backing_plan_with_planning_support(
            self.fibers.config(),
            fiber_planning,
        )?
        .combined_for_base_alignment(base_align)?;
        let executor_plan =
            CurrentAsyncRuntime::backing_plan_with_layout_policy_and_planning_support(
                self.executor,
                layout_policy,
                executor_planning,
            )?
            .combined_eager_for_base_alignment(base_align)?;

        let mut max_align = fiber_plan.slab.align;
        if executor_plan.slab.align > max_align {
            max_align = executor_plan.slab.align;
        }

        let mut cursor = if base_align >= max_align {
            0
        } else {
            max_align.saturating_sub(1)
        };
        let fiber_offset = runtime_align_up_packed(cursor, fiber_plan.slab.align)?;
        cursor = fiber_offset.checked_add(fiber_plan.slab.bytes).ok_or(
            CurrentFiberAsyncRuntimeError::Fiber(
                fusion_sys::fiber::FiberError::resource_exhausted(),
            ),
        )?;
        let executor_offset = runtime_align_up_packed(cursor, executor_plan.slab.align)?;
        let total_bytes = executor_offset
            .checked_add(executor_plan.slab.bytes)
            .ok_or(CurrentFiberAsyncRuntimeError::Fiber(
                fusion_sys::fiber::FiberError::resource_exhausted(),
            ))?;

        Ok(CurrentFiberAsyncRuntimeBackingPlan {
            slab: RuntimeBackingRequest {
                bytes: total_bytes,
                align: max_align,
            },
            fibers: ResourceRange::new(fiber_offset, fiber_plan.slab.bytes),
            executor: ResourceRange::new(executor_offset, executor_plan.slab.bytes),
            fiber_plan,
            executor_plan,
        })
    }

    /// Builds one current-thread fiber + async runtime bundle through platform acquisition.
    ///
    /// # Errors
    ///
    /// Returns any honest current-thread fiber or async bootstrap failure.
    pub fn build_current(self) -> Result<CurrentFiberAsyncRuntime, CurrentFiberAsyncRuntimeError> {
        let parts = self.build_current_parts()?;
        let (fibers, executor) = parts.into_parts();
        Ok(CurrentFiberAsyncRuntime {
            fibers,
            executor: executor.build()?,
        })
    }

    /// Builds one split current-thread fiber + async bootstrap through platform acquisition.
    ///
    /// The async runtime stays deferred so callers can realize it inside one spawned current-thread
    /// fiber without forcing `CurrentAsyncRuntime` itself to become `Send`.
    ///
    /// # Errors
    ///
    /// Returns any honest current-thread fiber bootstrap failure.
    pub fn build_current_parts(
        self,
    ) -> Result<CurrentFiberAsyncParts, CurrentFiberAsyncRuntimeError> {
        if uses_explicit_bound_runtime_backing() {
            let fibers = self.fibers.build_current()?;
            let layout = CurrentAsyncRuntime::backing_plan(self.executor)?;
            let combined = layout.combined_eager()?;
            let slab = allocate_owned_runtime_slab(combined.slab.bytes, combined.slab.align)
                .map_err(current_runtime_error_from_owned_backing)?;
            if let Some(slab) = slab {
                return Ok(CurrentFiberAsyncParts {
                    fibers,
                    executor: CurrentAsyncRuntimeBuilder {
                        config: self.executor,
                        slab: None,
                        owned_backing: Some(slab.lease),
                    },
                });
            }
        }
        let fibers = self.fibers.build_current()?;
        Ok(CurrentFiberAsyncParts {
            fibers,
            executor: CurrentAsyncRuntimeBuilder {
                config: self.executor,
                slab: None,
                owned_backing: None,
            },
        })
    }

    /// Builds one current-thread fiber + async runtime bundle from one caller-owned bound slab.
    ///
    /// # Errors
    ///
    /// Returns any honest sizing, partitioning, or bootstrap failure.
    pub fn from_bound_slab(
        self,
        slab: MemoryResourceHandle,
    ) -> Result<CurrentFiberAsyncRuntime, CurrentFiberAsyncRuntimeError> {
        let parts = self.from_bound_slab_parts(slab)?;
        let (fibers, executor) = parts.into_parts();
        Ok(CurrentFiberAsyncRuntime {
            fibers,
            executor: executor.build()?,
        })
    }

    /// Builds one split current-thread fiber + async bundle from one caller-owned bound slab.
    ///
    /// The async runtime stays deferred so callers can realize it inside a spawned current-thread
    /// fiber while still consuming one combined owning slab.
    ///
    /// # Errors
    ///
    /// Returns any honest sizing, partitioning, or bootstrap failure.
    pub fn from_bound_slab_parts(
        self,
        slab: MemoryResourceHandle,
    ) -> Result<CurrentFiberAsyncParts, CurrentFiberAsyncRuntimeError> {
        let layout = self.backing_plan_for_base_alignment_with_allocator_layout_policy(
            runtime_resource_base_alignment(&slab),
            slab.info().layout,
        )?;
        if slab.view().len() < layout.slab.bytes {
            return Err(CurrentFiberAsyncRuntimeError::Fiber(
                fusion_sys::fiber::FiberError::resource_exhausted(),
            ));
        }
        let fibers = CurrentFiberPool::from_bound_slab(
            self.fibers.config(),
            partition_runtime_bound_resource(&slab, layout.fibers)?,
        )?;
        Ok(CurrentFiberAsyncParts {
            fibers,
            executor: CurrentAsyncRuntimeBuilder {
                config: self.executor,
                slab: Some(partition_runtime_bound_resource(&slab, layout.executor)?),
                owned_backing: None,
            },
        })
    }

    /// Builds one current-thread fiber + async runtime bundle from one caller-owned static slab.
    ///
    /// # Safety
    ///
    /// The caller must guarantee the supplied pointer/length pair names one valid writable static
    /// extent for the whole lifetime of the bundle.
    ///
    /// # Errors
    ///
    /// Returns any honest binding, sizing, partitioning, or bootstrap failure.
    pub unsafe fn from_static_slab(
        self,
        ptr: *mut u8,
        len: usize,
    ) -> Result<CurrentFiberAsyncRuntime, CurrentFiberAsyncRuntimeError> {
        let slab = MemoryResourceHandle::from(
            unsafe { BoundMemoryResource::static_allocatable_bytes(ptr, len) }.map_err(|_| {
                CurrentFiberAsyncRuntimeError::Executor(super::ExecutorError::Unsupported)
            })?,
        );
        self.from_bound_slab(slab)
    }

    /// Builds one split current-thread fiber + async bundle from one caller-owned static slab.
    ///
    /// # Safety
    ///
    /// The caller must guarantee the supplied pointer/length pair names one valid writable static
    /// extent for the whole lifetime of the split bundle.
    ///
    /// # Errors
    ///
    /// Returns any honest binding, sizing, partitioning, or bootstrap failure.
    pub unsafe fn from_static_slab_parts(
        self,
        ptr: *mut u8,
        len: usize,
    ) -> Result<CurrentFiberAsyncParts, CurrentFiberAsyncRuntimeError> {
        let slab = MemoryResourceHandle::from(
            unsafe { BoundMemoryResource::static_allocatable_bytes(ptr, len) }.map_err(|_| {
                CurrentFiberAsyncRuntimeError::Executor(super::ExecutorError::Unsupported)
            })?,
        );
        self.from_bound_slab_parts(slab)
    }
}

fn current_runtime_error_from_owned_backing(
    error: RuntimeBackingError,
) -> CurrentFiberAsyncRuntimeError {
    let executor_error = match error.kind() {
        RuntimeBackingErrorKind::Unsupported => super::ExecutorError::Unsupported,
        RuntimeBackingErrorKind::Invalid => {
            super::ExecutorError::Sync(crate::sync::SyncErrorKind::Invalid)
        }
        RuntimeBackingErrorKind::ResourceExhausted => {
            super::ExecutorError::Sync(crate::sync::SyncErrorKind::Overflow)
        }
        RuntimeBackingErrorKind::StateConflict => {
            super::ExecutorError::Sync(crate::sync::SyncErrorKind::Busy)
        }
    };
    CurrentFiberAsyncRuntimeError::Executor(executor_error)
}

/// Runtime profile selecting broad safety and elasticity policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeProfile {
    /// Fixed-capacity, deterministic carrier and queue behavior.
    Deterministic,
    /// Balanced hosted profile with optional elasticity.
    Balanced,
    /// Throughput-oriented profile with relaxed elasticity limits.
    Throughput,
    /// Fully custom manual control.
    Custom,
}

/// Hard constraints enforced by the deterministic runtime profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeterministicConstraints {
    /// Carrier thread count is fixed after startup.
    pub workers: FixedConstraint,
    /// Queue capacities are fixed and bounded.
    pub queues: FixedConstraint,
    /// Green-thread population is fixed or explicitly capped.
    pub green_limit: FixedConstraint,
    /// Cross-domain stealing is forbidden unless explicitly allowed.
    pub global_steal: GlobalStealConstraint,
}

impl DeterministicConstraints {
    /// Returns strict deterministic defaults.
    #[must_use]
    pub const fn strict() -> Self {
        Self {
            workers: FixedConstraint::Required,
            queues: FixedConstraint::Required,
            green_limit: FixedConstraint::Required,
            global_steal: GlobalStealConstraint::Disallow,
        }
    }
}

/// Whether a runtime resource must remain fixed after startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FixedConstraint {
    /// The resource must remain fixed after startup.
    Required,
    /// The resource may remain flexible.
    Flexible,
}

/// Whether global stealing is allowed under deterministic policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GlobalStealConstraint {
    /// Global stealing is forbidden.
    Disallow,
    /// Global stealing is allowed.
    Allow,
}

/// Elastic behavior knobs for hosted-oriented runtime profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ElasticConfig {
    /// Allow carrier-pool resizing.
    pub allow_resize: bool,
    /// Allow on-demand green-thread growth.
    pub allow_on_demand_green: bool,
    /// Allow work stealing across the full machine.
    pub allow_global_steal: bool,
}

impl ElasticConfig {
    /// Returns permissive hosted defaults.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            allow_resize: true,
            allow_on_demand_green: true,
            allow_global_steal: true,
        }
    }
}

impl Default for ElasticConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Public runtime configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RuntimeConfig<'a> {
    /// Selected runtime profile.
    pub profile: RuntimeProfile,
    /// Carrier thread-pool configuration.
    pub thread_pool: ThreadPoolConfig<'a>,
    /// Optional green-thread configuration.
    pub green: Option<GreenPoolConfig<'a>>,
    /// Executor configuration.
    pub executor: ExecutorConfig,
    /// Optional deterministic constraints.
    pub deterministic: Option<DeterministicConstraints>,
    /// Optional elastic profile configuration.
    pub elastic: Option<ElasticConfig>,
}

/// Public runtime statistics snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RuntimeStats {
    /// Active carrier workers, when the pool exists.
    pub carrier_workers: usize,
    /// Active green threads, when the pool exists.
    pub green_threads: usize,
    /// Known queued tasks.
    pub queued_tasks: usize,
}

/// Public runtime error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeError {
    /// The requested composition is unsupported or not implemented yet.
    Unsupported,
    /// The requested carrier thread pool could not be created honestly.
    ThreadPool(super::ThreadPoolError),
    /// The requested green-thread pool could not be created honestly.
    Green(fusion_sys::fiber::FiberError),
    /// The configured executor could not be bound honestly.
    Executor(super::ExecutorError),
}

/// Public runtime orchestrator.
#[derive(Debug)]
pub struct Runtime {
    executor: Executor,
    green_pool: Option<GreenPool>,
    thread_pool: Option<ThreadPool>,
}

impl Runtime {
    /// Creates a runtime orchestrator from the supplied configuration.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` for not-yet-implemented green-thread-backed configurations, or
    /// a lower-level carrier-pool error when pool creation fails.
    pub fn new(config: &RuntimeConfig<'_>) -> Result<Self, RuntimeError> {
        validate_runtime_config(config)?;

        let thread_pool = ThreadPool::new(&config.thread_pool).map_err(RuntimeError::ThreadPool)?;
        let green_pool = match config.green {
            Some(green) => Some(GreenPool::new(&green, &thread_pool).map_err(RuntimeError::Green)?),
            None => None,
        };
        let executor = match config.executor.mode {
            super::ExecutorMode::CurrentThread => Executor::new(config.executor),
            super::ExecutorMode::ThreadPool => Executor::new(config.executor)
                .on_pool(&thread_pool)
                .map_err(RuntimeError::Executor)?,
            super::ExecutorMode::GreenPool => Executor::new(config.executor)
                .on_green(green_pool.as_ref().ok_or(RuntimeError::Unsupported)?)
                .map_err(RuntimeError::Executor)?,
            super::ExecutorMode::Hybrid => return Err(RuntimeError::Unsupported),
        };

        Ok(Self {
            executor,
            green_pool,
            thread_pool: Some(thread_pool),
        })
    }

    /// Returns the configured executor surface.
    #[must_use]
    pub const fn executor(&self) -> &Executor {
        &self.executor
    }

    /// Returns the thread pool when one exists.
    #[must_use]
    pub const fn thread_pool(&self) -> Option<&ThreadPool> {
        self.thread_pool.as_ref()
    }

    /// Returns the green-thread pool when one exists.
    #[must_use]
    pub const fn green_pool(&self) -> Option<&GreenPool> {
        self.green_pool.as_ref()
    }

    /// Returns a snapshot of the current runtime state.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying carrier-pool statistics cannot be observed honestly.
    pub fn stats(&self) -> Result<RuntimeStats, RuntimeError> {
        let (carrier_workers, queued_tasks) = match self.thread_pool.as_ref() {
            Some(pool) => {
                let stats = pool.stats().map_err(RuntimeError::ThreadPool)?;
                (stats.active_workers, stats.queued_items)
            }
            None => (0, 0),
        };
        let green_threads = self.green_pool.as_ref().map_or(0, GreenPool::active_count);

        Ok(RuntimeStats {
            carrier_workers,
            green_threads,
            queued_tasks,
        })
    }
}

fn validate_runtime_config(config: &RuntimeConfig<'_>) -> Result<(), RuntimeError> {
    if matches!(config.executor.mode, super::ExecutorMode::GreenPool) && config.green.is_none() {
        return Err(RuntimeError::Unsupported);
    }

    if matches!(config.executor.mode, super::ExecutorMode::Hybrid) {
        return Err(RuntimeError::Unsupported);
    }

    if config.profile == RuntimeProfile::Deterministic {
        let constraints = config
            .deterministic
            .unwrap_or_else(DeterministicConstraints::strict);
        if matches!(constraints.workers, FixedConstraint::Required)
            && config.thread_pool.min_threads != config.thread_pool.max_threads
        {
            return Err(RuntimeError::Unsupported);
        }
        if matches!(constraints.global_steal, GlobalStealConstraint::Disallow)
            && matches!(
                config.thread_pool.steal_boundary,
                super::StealBoundary::Global
            )
        {
            return Err(RuntimeError::Unsupported);
        }
        if let Some(green) = config.green {
            if green.task_capacity_per_carrier().is_err() {
                return Err(RuntimeError::Unsupported);
            }
            if green.uses_legacy_capacity_model()
                && !matches!(green.stack_backing, FiberStackBacking::Fixed { .. })
            {
                return Err(RuntimeError::Unsupported);
            }
            if !matches!(green.growth, GreenGrowth::Fixed) {
                return Err(RuntimeError::Unsupported);
            }
            if matches!(green.scheduling, GreenScheduling::WorkStealing) {
                return Err(RuntimeError::Unsupported);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread::async_yield_now;
    use fusion_pal::sys::mem::{Address, CachePolicy, MemAdviceCaps, Protect, Region};
    use fusion_sys::mem::resource::{
        BoundResourceSpec,
        MemoryDomain,
        MemoryGeometry,
        OvercommitPolicy,
        ResourceAttrs,
        ResourceContract,
        ResourceOpSet,
        ResourceResidencySupport,
        ResourceState,
        ResourceSupport,
        SharingPolicy,
        StateValue,
    };
    use std::alloc::{Layout, alloc_zeroed};

    fn aligned_bound_resource(len: usize, align: usize) -> MemoryResourceHandle {
        let layout = Layout::from_size_align(len, align).expect("aligned test layout should build");
        let ptr = unsafe { alloc_zeroed(layout) };
        assert!(
            !ptr.is_null(),
            "aligned test slab allocation should succeed"
        );
        MemoryResourceHandle::from(
            BoundMemoryResource::new(BoundResourceSpec::new(
                Region {
                    base: Address::new(ptr as usize),
                    len,
                },
                MemoryDomain::StaticRegion,
                ResourceBackingKind::Borrowed,
                ResourceAttrs::ALLOCATABLE | ResourceAttrs::CACHEABLE | ResourceAttrs::COHERENT,
                MemoryGeometry {
                    base_granule: NonZeroUsize::new(1).expect("non-zero granule"),
                    alloc_granule: NonZeroUsize::new(1).expect("non-zero granule"),
                    protect_granule: None,
                    commit_granule: None,
                    lock_granule: None,
                    large_granule: None,
                },
                AllocatorLayoutPolicy::exact_static(),
                ResourceContract {
                    allowed_protect: Protect::READ | Protect::WRITE,
                    write_xor_execute: true,
                    sharing: SharingPolicy::Private,
                    overcommit: OvercommitPolicy::Disallow,
                    cache_policy: CachePolicy::Default,
                    integrity: None,
                },
                ResourceSupport {
                    protect: Protect::READ | Protect::WRITE,
                    ops: ResourceOpSet::QUERY,
                    advice: MemAdviceCaps::empty(),
                    residency: ResourceResidencySupport::BEST_EFFORT,
                },
                ResourceState::static_state(
                    StateValue::Uniform(Protect::READ | Protect::WRITE),
                    StateValue::Uniform(false),
                    StateValue::Uniform(true),
                ),
            ))
            .expect("aligned bound resource should bind"),
        )
    }

    #[test]
    fn combined_current_runtime_backing_plan_rounds_up_when_requested() {
        let exact = CurrentFiberAsyncBootstrap::uniform(
            1,
            NonZeroUsize::new(8 * 1024).expect("non-zero stack"),
            1,
        )
        .with_guard_pages(0)
        .with_sizing_strategy(RuntimeSizingStrategy::Exact)
        .backing_plan()
        .expect("exact plan should build");
        let rounded = CurrentFiberAsyncBootstrap::uniform(
            1,
            NonZeroUsize::new(8 * 1024).expect("non-zero stack"),
            1,
        )
        .with_guard_pages(0)
        .with_sizing_strategy(RuntimeSizingStrategy::GlobalNearestRoundUp)
        .backing_plan()
        .expect("rounded plan should build");

        assert!(rounded.slab.bytes >= exact.slab.bytes);
        assert!(rounded.slab.align >= exact.slab.align);
    }

    #[test]
    fn combined_current_runtime_exact_aligned_plan_reduces_padding() {
        let bootstrap = CurrentFiberAsyncBootstrap::uniform(
            1,
            NonZeroUsize::new(8 * 1024).expect("non-zero stack"),
            2,
        )
        .with_guard_pages(0);
        let conservative = bootstrap
            .backing_plan()
            .expect("conservative plan should build");
        let exact = bootstrap
            .backing_plan_for_base_alignment(conservative.slab.align)
            .expect("exact-aligned plan should build");

        assert!(exact.slab.bytes <= conservative.slab.bytes);
        assert_eq!(exact.slab.align, conservative.slab.align);
    }

    #[test]
    fn combined_current_runtime_target_planning_support_can_shrink_fiber_backing() {
        let bootstrap = CurrentFiberAsyncBootstrap::uniform(
            1,
            NonZeroUsize::new((12 * 1024) + 8).expect("non-zero stack"),
            1,
        )
        .with_guard_pages(0)
        .with_sizing_strategy(RuntimeSizingStrategy::Exact);
        let hosted_like = bootstrap
            .backing_plan_with_fiber_planning_support_and_allocator_layout_policy(
                FiberPlanningSupport::same_carrier(
                    352,
                    16,
                    128,
                    fusion_sys::fiber::ContextStackDirection::Down,
                    false,
                ),
                AllocatorLayoutPolicy::exact_static(),
            )
            .expect("hosted-like plan should build");
        let cortex_m = bootstrap
            .backing_plan_with_fiber_planning_support_and_allocator_layout_policy(
                FiberPlanningSupport::same_carrier(
                    0,
                    8,
                    0,
                    fusion_sys::fiber::ContextStackDirection::Down,
                    false,
                ),
                AllocatorLayoutPolicy::exact_static(),
            )
            .expect("cortex-m plan should build");

        assert!(cortex_m.fiber_plan.stacks.len < hosted_like.fiber_plan.stacks.len);
        assert!(cortex_m.slab.bytes <= hosted_like.slab.bytes);
    }

    #[test]
    fn current_runtime_from_bound_slab_parts_build_both_runtimes() {
        let bootstrap = CurrentFiberAsyncBootstrap::uniform(
            1,
            NonZeroUsize::new(8 * 1024).expect("non-zero stack"),
            2,
        )
        .with_guard_pages(0);
        let layout = bootstrap.backing_plan().expect("backing plan should build");
        let slab = aligned_bound_resource(layout.slab.bytes, layout.slab.align);
        let runtime = bootstrap
            .from_bound_slab_parts(slab)
            .expect("combined runtime should build from one bound slab");
        let (fibers, executor) = runtime.into_parts();

        let handle = fibers
            .spawn_with_stack::<4096, _, _>(|| 7_u8)
            .expect("fiber should spawn");
        assert_eq!(handle.join().expect("fiber join should complete"), 7);
        let executor = executor
            .build_explicit()
            .expect("executor should build from split backing");
        let task = executor
            .spawn_with_poll_stack_bytes(2048, async {
                async_yield_now().await;
                41_u8
            })
            .expect("async task should spawn");

        assert_eq!(
            executor
                .block_on_with_poll_stack_bytes(2048, task)
                .expect("runtime should drive async task")
                .expect("async task should complete"),
            41
        );

        fibers
            .shutdown()
            .expect("combined current runtime should shut down fibers");
    }

    #[test]
    fn current_runtime_from_exact_aligned_bound_slab_parts_builds() {
        let bootstrap = CurrentFiberAsyncBootstrap::uniform(
            1,
            NonZeroUsize::new(8 * 1024).expect("non-zero stack"),
            2,
        )
        .with_guard_pages(0);
        let conservative = bootstrap.backing_plan().expect("plan should build");
        let exact = bootstrap
            .backing_plan_for_base_alignment(conservative.slab.align)
            .expect("exact-aligned plan should build");
        let slab = aligned_bound_resource(exact.slab.bytes, exact.slab.align);
        let runtime = bootstrap
            .from_bound_slab_parts(slab)
            .expect("combined runtime should build from exact-aligned slab");
        let (fibers, executor) = runtime.into_parts();

        let handle = fibers
            .spawn_with_stack::<4096, _, _>(|| 9_u8)
            .expect("fiber should spawn");
        assert_eq!(handle.join().expect("fiber join should complete"), 9);
        let executor = executor
            .build_explicit()
            .expect("executor should build from exact-aligned split backing");
        let task = executor
            .spawn_with_poll_stack_bytes(2048, async { 43_u8 })
            .expect("async task should spawn");
        assert_eq!(
            executor
                .block_on_with_poll_stack_bytes(2048, task)
                .expect("runtime should drive task")
                .expect("task should complete"),
            43
        );

        fibers
            .shutdown()
            .expect("combined current runtime should shut down fibers");
    }

    #[test]
    fn current_runtime_reports_configured_memory_footprint() {
        let bootstrap = CurrentFiberAsyncBootstrap::uniform(
            1,
            NonZeroUsize::new(8 * 1024).expect("non-zero stack"),
            2,
        )
        .with_guard_pages(0);
        let layout = bootstrap.backing_plan().expect("backing plan should build");
        let slab = aligned_bound_resource(layout.slab.bytes, layout.slab.align);
        let runtime = bootstrap
            .from_bound_slab(slab)
            .expect("combined runtime should build from one bound slab");
        let footprint = runtime
            .configured_memory_footprint()
            .expect("configured runtime footprint should build");

        assert!(footprint.fibers.total_bytes() > 0);
        assert!(footprint.executor.total_bytes() > 0);
        assert_eq!(
            footprint.total_bytes(),
            footprint.fibers.total_bytes() + footprint.executor.total_bytes()
        );

        runtime
            .fibers()
            .shutdown()
            .expect("combined current runtime should shut down fibers");
    }

    #[test]
    fn current_runtime_singleton_grows_fiber_capacity_quiescently() {
        static RUNTIME: CurrentFiberAsyncSingleton = CurrentFiberAsyncSingleton::new();

        let initial_capacity = RUNTIME
            .fiber_runtime_borrow(None, None)
            .expect("singleton fiber runtime should initialize")
            .configured_capacity();
        assert_eq!(initial_capacity, 1);

        let first = RUNTIME
            .spawn_fiber_with_stack::<4096, _, _>(|| 11_u8)
            .expect("first fiber should spawn");
        while !first.is_finished().expect("fiber completion should read") {
            assert!(RUNTIME.drive_once().expect("fiber drive should succeed"));
        }

        let second = RUNTIME
            .spawn_fiber_with_stack::<4096, _, _>(|| 13_u8)
            .expect("second fiber should trigger quiescent growth");
        let grown_capacity = RUNTIME
            .fiber_runtime_borrow(None, None)
            .expect("singleton fiber runtime should remain borrowable")
            .configured_capacity();
        assert!(
            grown_capacity > initial_capacity,
            "fiber capacity should grow after quiescent slot exhaustion"
        );

        while !second.is_finished().expect("fiber completion should read") {
            assert!(RUNTIME.drive_once().expect("fiber drive should succeed"));
        }

        assert_eq!(first.join().expect("first fiber should join"), 11);
        assert_eq!(second.join().expect("second fiber should join"), 13);
        RUNTIME
            .shutdown_fibers()
            .expect("singleton fibers should shut down cleanly");
    }

    #[test]
    fn current_runtime_singleton_grows_async_capacity_quiescently() {
        static RUNTIME: CurrentFiberAsyncSingleton = CurrentFiberAsyncSingleton::new();

        let initial_capacity = RUNTIME
            .async_total_capacity()
            .expect("singleton async capacity should read");
        assert_eq!(initial_capacity, 0);

        let first = RUNTIME
            .spawn_async_with_poll_stack_bytes(2048, async { 21_u8 })
            .expect("first async task should spawn");
        let after_first_capacity = RUNTIME
            .async_total_capacity()
            .expect("singleton async capacity should read");
        assert_eq!(after_first_capacity, 1);
        assert_eq!(
            RUNTIME
                .run_async_until_idle()
                .expect("singleton async runtime should run to idle"),
            1
        );
        assert!(first.is_finished().expect("first task state should read"));

        let second = RUNTIME
            .spawn_async_with_poll_stack_bytes(2048, async { 34_u8 })
            .expect("second async task should trigger segmented growth");
        let grown_capacity = RUNTIME
            .async_total_capacity()
            .expect("singleton async capacity should read");
        assert!(
            grown_capacity > after_first_capacity,
            "async capacity should grow by appending another segment after slot exhaustion"
        );

        assert_eq!(
            RUNTIME
                .run_async_until_idle()
                .expect("singleton async runtime should run to idle"),
            1
        );

        assert_eq!(first.join().expect("first task should join"), 21);
        assert_eq!(second.join().expect("second task should join"), 34);
    }

    #[test]
    fn current_runtime_singleton_respects_fiber_capacity_cap() {
        static RUNTIME: CurrentFiberAsyncSingleton =
            CurrentFiberAsyncSingleton::new().with_fiber_capacity(1);

        let first = RUNTIME
            .spawn_fiber_with_stack::<4096, _, _>(|| 55_u8)
            .expect("first fiber should spawn");
        while !first.is_finished().expect("fiber completion should read") {
            assert!(RUNTIME.drive_once().expect("fiber drive should succeed"));
        }

        let second = RUNTIME.spawn_fiber_with_stack::<4096, _, _>(|| 89_u8);
        assert!(matches!(
            second,
            Err(error) if error.kind() == FiberErrorKind::ResourceExhausted
        ));

        assert_eq!(first.join().expect("first fiber should join"), 55);
        RUNTIME
            .shutdown_fibers()
            .expect("singleton fibers should shut down cleanly");
    }

    #[test]
    fn current_runtime_singleton_respects_async_capacity_cap() {
        static RUNTIME: CurrentFiberAsyncSingleton =
            CurrentFiberAsyncSingleton::new().with_async_capacity(1);

        let first = RUNTIME
            .spawn_async_with_poll_stack_bytes(2048, async { 144_u8 })
            .expect("first async task should spawn");
        assert_eq!(
            RUNTIME
                .run_async_until_idle()
                .expect("singleton async runtime should run to idle"),
            1
        );
        assert!(first.is_finished().expect("task completion should read"));

        let second = RUNTIME.spawn_async_with_poll_stack_bytes(2048, async { 233_u8 });
        assert!(matches!(
            second,
            Err(ExecutorError::Sync(SyncErrorKind::Busy))
        ));
        assert_eq!(
            RUNTIME
                .async_total_capacity()
                .expect("singleton async capacity should read"),
            1
        );

        assert_eq!(first.join().expect("first task should join"), 144);
    }

    #[test]
    fn current_runtime_singleton_block_on_drives_cross_segment_task_handles() {
        static RUNTIME: CurrentFiberAsyncSingleton = CurrentFiberAsyncSingleton::new();

        let first = RUNTIME
            .spawn_async_with_poll_stack_bytes(2048, async { 99_u8 })
            .expect("first async task should spawn");

        let result = RUNTIME
            .block_on_with_poll_stack_bytes(2048, async move {
                first.await.expect("first task should complete")
            })
            .expect("singleton block_on should drive tasks across every async segment");

        assert_eq!(result, 99);
        assert!(
            RUNTIME
                .async_total_capacity()
                .expect("singleton async capacity should read")
                > 1
        );
    }
}
