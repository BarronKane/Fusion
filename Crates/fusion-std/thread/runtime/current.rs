use crate::thread::{
    ExplicitFiberTask,
    FiberTaskAttributes,
    FiberTaskPriority,
    GeneratedExplicitFiberTask,
};
#[cfg(feature = "critical-safe")]
use crate::thread::{
    GeneratedExplicitFiberTaskContract,
    generated_explicit_task_contract_attributes,
};
use core::sync::atomic::{
    AtomicU32,
    AtomicUsize,
    Ordering,
};
use super::*;

#[unsafe(no_mangle)]
pub static CURRENT_SINGLETON_FIBER_SPAWN_PHASE: AtomicU32 = AtomicU32::new(0);

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

pub(super) struct CurrentFiberPoolBorrow<'a> {
    slot: &'a CurrentFiberRuntimeSlot,
    runtime: *const CurrentFiberPool,
    configured_capacity: usize,
}

impl CurrentFiberPoolBorrow<'_> {
    pub(super) const fn configured_capacity(&self) -> usize {
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

/// Tiny lazy current-thread fiber+async manual runner for bootstrap, audit, and board-local front
/// doors.
///
/// This is not the final autonomous courier runtime model. It exists for targets that want a
/// `thread::spawn()`-clean bootstrap surface while the exact generated metadata pipeline still
/// depends on build-time sidecars rather than compiler-native artifacts.
pub struct CurrentFiberAsyncSingleton {
    courier_id: Option<CourierId>,
    context_id: Option<ContextId>,
    runtime_sink: Option<CourierRuntimeSink>,
    launch_control: Option<CourierLaunchControl<'static>>,
    launch_request: Option<CourierChildLaunchRequest<'static>>,
    courier_plan: Option<CourierPlan>,
    fiber_capacity_limit: Option<usize>,
    async_capacity_limit: Option<usize>,
    stack_floor_bytes: Option<usize>,
    guard_pages: Option<usize>,
    runtime_dispatch_cookie: AtomicUsize,
    fibers: CurrentFiberRuntimeSlot,
    executor: CurrentAsyncRuntimeSlot,
}

fn current_singleton_runtime_dispatch_callback(context: usize) {
    if context == 0 {
        return;
    }
    // SAFETY: the runtime-dispatch broker stores only pointers registered from stable
    // `CurrentFiberAsyncSingleton` instances.
    let runtime = unsafe { &*(context as *const CurrentFiberAsyncSingleton) };
    runtime.autonomous_dispatch_once();
}

impl CurrentFiberAsyncSingleton {
    const AUTONOMOUS_DISPATCH_BATCH_LIMIT: usize = 8;

    #[must_use]
    pub const fn new() -> Self {
        Self {
            courier_id: None,
            context_id: None,
            runtime_sink: None,
            launch_control: None,
            launch_request: None,
            courier_plan: None,
            fiber_capacity_limit: None,
            async_capacity_limit: None,
            stack_floor_bytes: None,
            guard_pages: None,
            runtime_dispatch_cookie: AtomicUsize::new(0),
            fibers: CurrentFiberRuntimeSlot::new(),
            executor: CurrentAsyncRuntimeSlot::new(),
        }
    }

    /// Installs one explicit courier-shaped runtime plan for this singleton front door.
    ///
    /// Explicit per-lane overrides still win where they exist; the courier plan supplies the
    /// default capacity and outer scheduling policy truth.
    #[must_use]
    pub const fn with_courier_plan(mut self, courier_plan: CourierPlan) -> Self {
        self.courier_plan = Some(courier_plan);
        self
    }

    /// Installs one explicit owning courier identity for the lazily realized runtime.
    #[must_use]
    pub const fn with_courier_id(mut self, courier_id: CourierId) -> Self {
        self.courier_id = Some(courier_id);
        self
    }

    /// Installs one explicit owning context identity for the lazily realized runtime.
    #[must_use]
    pub const fn with_context_id(mut self, context_id: ContextId) -> Self {
        self.context_id = Some(context_id);
        self
    }

    /// Installs one explicit runtime-to-courier sink for the lazily realized runtime.
    #[must_use]
    pub const fn with_runtime_sink(mut self, runtime_sink: CourierRuntimeSink) -> Self {
        self.runtime_sink = Some(runtime_sink);
        self
    }

    /// Installs one explicit child-courier launch-control surface for the lazily realized
    /// runtime.
    #[must_use]
    pub const fn with_launch_control(
        mut self,
        launch_control: CourierLaunchControl<'static>,
    ) -> Self {
        self.launch_control = Some(launch_control);
        self
    }

    /// Installs one explicit child-courier launch request for the lazily realized runtime.
    #[must_use]
    pub const fn with_child_launch(
        mut self,
        launch_request: CourierChildLaunchRequest<'static>,
    ) -> Self {
        self.launch_request = Some(launch_request);
        self
    }

    /// Installs one explicit current-thread fiber capacity cap.
    ///
    /// This is runtime policy only. It does not describe backend structural minimums.
    ///
    /// When the singleton realizes its fiber runtime lazily for the first time, this value also
    /// becomes the initial pool capacity unless the caller explicitly requests a different startup
    /// size through a lower-level runtime path.
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

    fn runtime_dispatch_cookie_if_registered(
        &self,
    ) -> Option<fusion_pal::sys::runtime_dispatch::RuntimeDispatchCookie> {
        let raw = self.runtime_dispatch_cookie.load(Ordering::Acquire);
        if raw == 0 || raw == usize::MAX {
            return None;
        }
        u32::try_from(raw)
            .ok()
            .map(fusion_pal::sys::runtime_dispatch::RuntimeDispatchCookie)
    }

    fn ensure_runtime_dispatch_cookie(
        &'static self,
    ) -> Option<fusion_pal::sys::runtime_dispatch::RuntimeDispatchCookie> {
        loop {
            match self.runtime_dispatch_cookie.load(Ordering::Acquire) {
                0 => {
                    if self
                        .runtime_dispatch_cookie
                        .compare_exchange(0, usize::MAX, Ordering::AcqRel, Ordering::Acquire)
                        .is_err()
                    {
                        continue;
                    }
                    let registered =
                        fusion_pal::sys::runtime_dispatch::register_runtime_dispatch_callback(
                            current_singleton_runtime_dispatch_callback,
                            self as *const Self as usize,
                        )
                        .ok();
                    let stored = registered
                        .and_then(|cookie| usize::try_from(cookie.0).ok())
                        .unwrap_or(0);
                    self.runtime_dispatch_cookie
                        .store(stored, Ordering::Release);
                    if let Some(cookie) = registered {
                        self.install_runtime_dispatch_cookie_into_realized_runtimes(cookie);
                    }
                    return registered;
                }
                usize::MAX => core::hint::spin_loop(),
                _ => return self.runtime_dispatch_cookie_if_registered(),
            }
        }
    }

    fn install_runtime_dispatch_cookie_into_realized_runtimes(
        &'static self,
        cookie: fusion_pal::sys::runtime_dispatch::RuntimeDispatchCookie,
    ) {
        if let Ok(_guard) = self.fibers.lock.lock() {
            // SAFETY: the thin mutex serializes access to the singleton fiber slot state.
            let state = unsafe { &mut *self.fibers.state.get() };
            if let Some(runtime) = state.runtime.as_ref() {
                runtime.install_runtime_dispatch_cookie(cookie);
            }
        }

        if let Ok(_guard) = self.executor.lock.lock() {
            // SAFETY: the thin mutex serializes access to the singleton async slot state.
            let state = unsafe { &mut *self.executor.state.get() };
            let mut segment = state.head;
            while let Some(node) = segment {
                // SAFETY: append-only async segments remain live and stable while linked.
                let node_ref = unsafe { node.as_ref() };
                let _ = node_ref.runtime.install_runtime_dispatch_cookie(cookie);
                segment = node_ref.next;
            }
        }
    }

    fn request_runtime_dispatch_best_effort(&'static self) {
        CURRENT_SINGLETON_FIBER_SPAWN_PHASE.store(0x50, Ordering::Release);
        if let Some(cookie) = self.ensure_runtime_dispatch_cookie() {
            CURRENT_SINGLETON_FIBER_SPAWN_PHASE.store(0x51, Ordering::Release);
            let _ = fusion_pal::sys::runtime_dispatch::request_runtime_dispatch(cookie);
            CURRENT_SINGLETON_FIBER_SPAWN_PHASE.store(0x52, Ordering::Release);
        }
    }

    /// Requests one best-effort autonomous runtime dispatch pass for this singleton.
    ///
    /// This is the truthful wake surface for current-thread courier runtimes: callers can notify
    /// the runtime that local work became runnable without regressing to manual pump vocabulary.
    pub fn request_autonomous_dispatch(&'static self) {
        self.request_runtime_dispatch_best_effort();
    }

    fn autonomous_dispatch_once(&'static self) {
        // One current-thread dispatch request must stay bounded. Unlike a real carrier, this path
        // still runs on the caller's thread. Re-arming forever here lets one perpetually runnable
        // service fiber seize the whole lane and turns "autonomous dispatch" back into hidden
        // manual-pump theater with extra steps.
        for _ in 0..Self::AUTONOMOUS_DISPATCH_BATCH_LIMIT {
            let mut progressed = false;

            if matches!(self.pump_async_once(), Ok(true)) {
                progressed = true;
            }
            if matches!(self.pump_fiber_once(), Ok(true)) {
                progressed = true;
            }

            if !progressed {
                break;
            }
        }
    }

    /// Returns the configured owning courier identity for this singleton, when present.
    #[must_use]
    pub const fn courier_id(&self) -> Option<CourierId> {
        self.courier_id
    }

    /// Returns the configured owning context identity for this singleton, when present.
    #[must_use]
    pub const fn context_id(&self) -> Option<ContextId> {
        self.context_id
    }

    fn effective_fiber_capacity_limit(&self) -> Option<usize> {
        match self.fiber_capacity_limit {
            Some(limit) => Some(limit),
            None => self.courier_plan.map(|plan| {
                if plan.max_live_fibers == 0 {
                    1
                } else {
                    plan.max_live_fibers
                }
            }),
        }
    }

    fn effective_async_capacity_limit(&self) -> Option<usize> {
        match self.async_capacity_limit {
            Some(limit) => Some(limit),
            None => self.courier_plan.map(|plan| {
                if plan.max_async_tasks == 0 {
                    1
                } else {
                    plan.max_async_tasks
                }
            }),
        }
    }

    fn runnable_capacity_limit(&self) -> Option<usize> {
        self.courier_plan.map(|plan| plan.max_runnable_units)
    }

    fn scheduling_policy(&self, fallback: CourierSchedulingPolicy) -> CourierSchedulingPolicy {
        match self.courier_plan.and_then(|plan| plan.time_slice_ticks) {
            Some(quantum_ticks) => CourierSchedulingPolicy::TimeSliced { quantum_ticks },
            None => fallback,
        }
    }

    fn courier_responsiveness(
        &'static self,
        fallback: CourierResponsiveness,
    ) -> CourierResponsiveness {
        let (Some(runtime_sink), Some(courier_id)) = (self.runtime_sink, self.courier_id) else {
            return fallback;
        };
        runtime_sink
            .evaluate_responsiveness(courier_id, runtime_tick())
            .unwrap_or(fallback)
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
        let stack_floor_bytes = (stack_floor_bytes != 0).then_some(stack_floor_bytes);
        FiberPoolBootstrap::uniform_growing(
            selected_stack_size_with_optional_floor(stack_floor_bytes)?,
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
        ensure_runtime_reserved_wake_vectors_best_effort();
        let mut bootstrap = self.fiber_bootstrap_with_policy(fiber_capacity, stack_floor_bytes)?;
        if let Some(courier_id) = self.courier_id {
            bootstrap = bootstrap.with_courier_id(courier_id);
        }
        if let Some(context_id) = self.context_id {
            bootstrap = bootstrap.with_context_id(context_id);
        }
        if let Some(runtime_sink) = self.runtime_sink {
            bootstrap = bootstrap.with_runtime_sink(runtime_sink);
        }
        if let Some(launch_control) = self.launch_control {
            bootstrap = bootstrap.with_launch_control(launch_control);
        }
        if let Some(launch_request) = self.launch_request {
            bootstrap = bootstrap.with_child_launch(launch_request);
        }
        let runtime = bootstrap.build_current()?;
        if let Some(cookie) = self.runtime_dispatch_cookie_if_registered() {
            runtime.install_runtime_dispatch_cookie(cookie);
        }
        Ok(runtime)
    }

    fn build_async_runtime_with_policy(
        &self,
        async_capacity: usize,
    ) -> Result<CurrentAsyncRuntime, ExecutorError> {
        ensure_runtime_reserved_wake_vectors_best_effort();
        let mut config = ExecutorConfig::new().with_capacity(async_capacity.max(1));
        if let Some(courier_id) = self.courier_id {
            config = config.with_courier_id(courier_id);
        }
        if let Some(context_id) = self.context_id {
            config = config.with_context_id(context_id);
        }
        if let Some(runtime_sink) = self.runtime_sink {
            config = config.with_runtime_sink(runtime_sink);
        }
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
        let runtime = CurrentAsyncRuntime::with_executor_config(config);
        if let Some(cookie) = self.runtime_dispatch_cookie_if_registered() {
            let _ = runtime.install_runtime_dispatch_cookie(cookie);
        }
        Ok(runtime)
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

    pub(super) fn fiber_runtime_borrow(
        &'static self,
        requested_capacity: Option<usize>,
        requested_stack_floor_bytes: Option<usize>,
    ) -> Result<CurrentFiberPoolBorrow<'static>, FiberError> {
        CURRENT_SINGLETON_FIBER_SPAWN_PHASE.store(0x20, Ordering::Release);
        let _guard =
            self.fibers.lock.lock().map_err(|error| {
                fiber_error_from_executor(executor_error_from_runtime_sync(error))
            })?;
        CURRENT_SINGLETON_FIBER_SPAWN_PHASE.store(0x21, Ordering::Release);
        // SAFETY: the thin mutex serializes access to the singleton fiber slot state.
        let state = unsafe { &mut *self.fibers.state.get() };
        if state.runtime.is_none() {
            CURRENT_SINGLETON_FIBER_SPAWN_PHASE.store(0x22, Ordering::Release);
            state.configured_capacity =
                initial_runtime_capacity(self.effective_fiber_capacity_limit(), requested_capacity)
                    .ok_or_else(FiberError::resource_exhausted)?;
            state.effective_stack_floor_bytes = self
                .stack_floor_bytes
                .unwrap_or(0)
                .max(requested_stack_floor_bytes.unwrap_or(0));
            CURRENT_SINGLETON_FIBER_SPAWN_PHASE.store(0x23, Ordering::Release);
            state.runtime = Some(self.build_fiber_runtime_with_policy(
                state.configured_capacity,
                state.effective_stack_floor_bytes,
            )?);
            CURRENT_SINGLETON_FIBER_SPAWN_PHASE.store(0x24, Ordering::Release);
        }
        state.borrows = state
            .borrows
            .checked_add(1)
            .ok_or_else(FiberError::resource_exhausted)?;
        CURRENT_SINGLETON_FIBER_SPAWN_PHASE.store(0x25, Ordering::Release);
        let runtime = state
            .runtime
            .as_ref()
            .map(core::ptr::from_ref)
            .ok_or_else(FiberError::invalid)?;
        CURRENT_SINGLETON_FIBER_SPAWN_PHASE.store(0x26, Ordering::Release);
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
                    self.effective_fiber_capacity_limit(),
                )
                .map_err(fiber_error_from_executor)?
                .ok_or_else(FiberError::resource_exhausted)?
            }
            _ if state.runtime.is_none() => initial_runtime_capacity(
                self.effective_fiber_capacity_limit(),
                requested_fiber_capacity,
            )
            .ok_or_else(FiberError::resource_exhausted)?,
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

    fn ensure_fiber_admission(&'static self, task: FiberTaskAttributes) -> Result<(), FiberError> {
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

    fn active_fiber_units(&'static self) -> Result<usize, FiberError> {
        Ok(self
            .fiber_runtime_if_initialized()?
            .map_or(0, |runtime| runtime.active_count()))
    }

    fn active_async_units(&'static self) -> Result<usize, ExecutorError> {
        let mut segment = self.async_segment_head_snapshot()?;
        let mut active = 0usize;
        while let Some(node) = segment {
            // SAFETY: append-only segment nodes remain live and stable while linked from the slot.
            let node_ref = unsafe { node.as_ref() };
            active = active.saturating_add(node_ref.runtime.unfinished_task_count()?);
            segment = node_ref.next;
        }
        Ok(active)
    }

    fn active_runnable_units(&'static self) -> Result<usize, ExecutorError> {
        Ok(self
            .active_fiber_units()
            .map_err(executor_error_from_fiber)?
            .saturating_add(self.active_async_units()?))
    }

    fn ensure_runnable_budget_for_fiber(&'static self) -> Result<(), FiberError> {
        let Some(limit) = self.runnable_capacity_limit() else {
            return Ok(());
        };
        // TODO: Planned-versus-dynamic runnable accounting still needs one courier-owned runtime
        // ledger. This front door can only enforce the total runnable envelope honestly today.
        if self
            .active_runnable_units()
            .map_err(fiber_error_from_executor)?
            >= limit
        {
            return Err(FiberError::resource_exhausted());
        }
        Ok(())
    }

    fn ensure_runnable_budget_for_async(&'static self) -> Result<(), ExecutorError> {
        let Some(limit) = self.runnable_capacity_limit() else {
            return Ok(());
        };
        // TODO: Split planned versus dynamic runnable budget once the courier-owned runtime ledger
        // tracks that distinction directly instead of forcing this singleton to infer it.
        if self.active_runnable_units()? >= limit {
            return Err(executor_resource_exhausted());
        }
        Ok(())
    }

    fn next_async_segment_capacity(&self, total_capacity: usize) -> Option<usize> {
        let requested = if total_capacity == 0 {
            1
        } else {
            total_capacity
        };
        match self.effective_async_capacity_limit() {
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

    /// Pumps one ready async task across every realized singleton async segment.
    ///
    /// # Errors
    ///
    /// Returns any honest executor failure from the realized segments.
    pub(crate) fn pump_async_once(&'static self) -> Result<bool, ExecutorError> {
        let mut segment = self.async_segment_head_snapshot()?;
        let mut progressed = false;
        while let Some(node) = segment {
            // SAFETY: append-only segment nodes remain live and stable while linked from the slot.
            let node_ref = unsafe { node.as_ref() };
            progressed |= node_ref.runtime.pump_once()?;
            segment = node_ref.next;
        }
        Ok(progressed)
    }

    /// Drains every realized singleton async segment until no task remains runnable.
    ///
    /// # Errors
    ///
    /// Returns any honest executor failure from the realized segments.
    #[cfg(test)]
    pub(crate) fn drain_async_until_idle(&'static self) -> Result<usize, ExecutorError> {
        let mut total = 0_usize;
        loop {
            let mut segment = self.async_segment_head_snapshot()?;
            let mut progressed = 0_usize;
            while let Some(node) = segment {
                // SAFETY: append-only segment nodes remain live and stable while linked from the
                // singleton slot.
                let node_ref = unsafe { node.as_ref() };
                progressed = progressed.saturating_add(node_ref.runtime.drain_until_idle()?);
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
    pub(super) fn async_total_capacity(&'static self) -> Result<usize, ExecutorError> {
        let _guard = self
            .executor
            .lock
            .lock()
            .map_err(executor_error_from_runtime_sync)?;
        // SAFETY: the thin mutex serializes access to the singleton async slot state.
        let state = unsafe { &*self.executor.state.get() };
        Ok(state.total_capacity)
    }

    /// Returns one courier-facing run summary for the currently realized fiber lane, if any.
    ///
    /// # Errors
    ///
    /// Returns any honest fiber observation failure.
    pub fn fiber_runtime_summary(
        &'static self,
    ) -> Result<Option<CourierRuntimeSummary>, FiberError> {
        self.fiber_runtime_summary_with_responsiveness(CourierResponsiveness::Responsive)
    }

    /// Returns one courier-facing run summary for the currently realized fiber lane, if any, using
    /// one caller-supplied responsiveness classification.
    ///
    /// # Errors
    ///
    /// Returns any honest fiber observation failure.
    pub fn fiber_runtime_summary_with_responsiveness(
        &'static self,
        responsiveness: CourierResponsiveness,
    ) -> Result<Option<CourierRuntimeSummary>, FiberError> {
        Ok(self
            .fiber_runtime_if_initialized()?
            .map(|runtime| {
                runtime
                    .runtime_summary_with_responsiveness(responsiveness)
                    .map(|mut summary| {
                        summary.policy = self.scheduling_policy(summary.policy);
                        summary
                    })
            })
            .transpose()?)
    }

    /// Returns one courier-facing run summary for the currently realized async lane, if any.
    ///
    /// # Errors
    ///
    /// Returns any honest executor observation failure.
    pub fn async_runtime_summary(
        &'static self,
    ) -> Result<Option<CourierRuntimeSummary>, ExecutorError> {
        self.async_runtime_summary_with_responsiveness(CourierResponsiveness::Responsive)
    }

    /// Returns one courier-facing run summary for the currently realized async lane, if any, using
    /// one caller-supplied responsiveness classification.
    ///
    /// # Errors
    ///
    /// Returns any honest executor observation failure.
    pub fn async_runtime_summary_with_responsiveness(
        &'static self,
        responsiveness: CourierResponsiveness,
    ) -> Result<Option<CourierRuntimeSummary>, ExecutorError> {
        let mut segment = self.async_segment_head_snapshot()?;
        let mut fiber_lane = None;
        let mut async_lane =
            CourierLaneSummary::new(fusion_sys::courier::RunnableUnitKind::AsyncTask);
        let mut saw_segment = false;
        while let Some(node) = segment {
            saw_segment = true;
            // SAFETY: append-only segment nodes remain live and stable while linked from the slot.
            let node_ref = unsafe { node.as_ref() };
            let summary = node_ref
                .runtime
                .runtime_summary_with_responsiveness(responsiveness)?;
            fiber_lane = fiber_lane.or(summary.fiber_lane);
            if let Some(lane) = summary.async_lane {
                async_lane.active_units = async_lane.active_units.saturating_add(lane.active_units);
                async_lane.runnable_units = async_lane
                    .runnable_units
                    .saturating_add(lane.runnable_units);
                async_lane.running_units =
                    async_lane.running_units.saturating_add(lane.running_units);
                async_lane.blocked_units =
                    async_lane.blocked_units.saturating_add(lane.blocked_units);
                async_lane.available_slots = async_lane
                    .available_slots
                    .saturating_add(lane.available_slots);
            }
            segment = node_ref.next;
        }
        if !saw_segment {
            return Ok(None);
        }
        Ok(Some(
            CourierRuntimeSummary {
                policy: self.scheduling_policy(CourierSchedulingPolicy::CooperativePriority),
                run_state: if async_lane.running_units != 0 {
                    CourierRunState::Running
                } else if async_lane.runnable_units != 0 {
                    CourierRunState::Runnable
                } else {
                    CourierRunState::Idle
                },
                responsiveness,
                fiber_lane,
                async_lane: Some(async_lane),
                control_lane: None,
            }
            .with_responsiveness(responsiveness),
        ))
    }

    /// Returns one aggregate courier-facing runtime summary for the lazily realized singleton.
    ///
    /// # Errors
    ///
    /// Returns any honest fiber or async observation failure.
    pub fn runtime_summary(&'static self) -> Result<CourierRuntimeSummary, ExecutorError> {
        self.runtime_summary_with_responsiveness(CourierResponsiveness::Responsive)
    }

    /// Returns one aggregate courier-facing runtime summary for the lazily realized singleton using
    /// one caller-supplied responsiveness classification.
    ///
    /// # Errors
    ///
    /// Returns any honest fiber or async observation failure.
    pub fn runtime_summary_with_responsiveness(
        &'static self,
        responsiveness: CourierResponsiveness,
    ) -> Result<CourierRuntimeSummary, ExecutorError> {
        let responsiveness = self.courier_responsiveness(responsiveness);
        let fiber_summary = self
            .fiber_runtime_summary_with_responsiveness(responsiveness)
            .map_err(executor_error_from_fiber)?;
        let async_summary = self.async_runtime_summary_with_responsiveness(responsiveness)?;
        let fiber_lane = fiber_summary.and_then(|summary| summary.fiber_lane);
        let async_lane = async_summary.and_then(|summary| summary.async_lane);
        Ok(CourierRuntimeSummary {
            policy: self.scheduling_policy(
                fiber_summary
                    .map(|summary| summary.policy)
                    .or(async_summary.map(|summary| summary.policy))
                    .unwrap_or(CourierSchedulingPolicy::CooperativePriority),
            ),
            run_state: if fiber_lane.is_some_and(|lane| lane.running_units != 0)
                || async_lane.is_some_and(|lane| lane.running_units != 0)
            {
                CourierRunState::Running
            } else if fiber_lane.is_some_and(|lane| lane.runnable_units != 0)
                || async_lane.is_some_and(|lane| lane.runnable_units != 0)
            {
                CourierRunState::Runnable
            } else {
                CourierRunState::Idle
            },
            responsiveness,
            fiber_lane,
            async_lane,
            control_lane: None,
        }
        .with_responsiveness(responsiveness))
    }

    pub fn spawn_fiber<F, T>(&'static self, job: F) -> Result<CurrentFiberHandle<T>, FiberError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: 'static,
    {
        self.ensure_runnable_budget_for_fiber()?;
        let task = self
            .fiber_runtime_borrow(None, None)?
            .closure_task_attributes::<F>()?;
        self.ensure_fiber_admission(task)?;
        let handle = self
            .fiber_runtime_borrow(
                None,
                task.execution
                    .is_fiber()
                    .then(|| task.stack_class.size_bytes().get()),
            )?
            .spawn_with_attrs(task, job)?;
        Ok(handle)
    }

    pub fn spawn_fiber_with_stack<const STACK_BYTES: usize, F, T>(
        &'static self,
        job: F,
    ) -> Result<CurrentFiberHandle<T>, FiberError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: 'static,
    {
        CURRENT_SINGLETON_FIBER_SPAWN_PHASE.store(1, Ordering::Release);
        self.ensure_runnable_budget_for_fiber()?;
        CURRENT_SINGLETON_FIBER_SPAWN_PHASE.store(2, Ordering::Release);
        let stack_bytes = NonZeroUsize::new(STACK_BYTES).ok_or_else(FiberError::invalid)?;
        let task = FiberTaskAttributes::from_stack_bytes(stack_bytes, FiberTaskPriority::DEFAULT)?;
        CURRENT_SINGLETON_FIBER_SPAWN_PHASE.store(3, Ordering::Release);
        self.ensure_fiber_admission(task)?;
        CURRENT_SINGLETON_FIBER_SPAWN_PHASE.store(4, Ordering::Release);
        let handle = self
            .fiber_runtime_borrow(None, Some(stack_bytes.get()))?
            .spawn_with_attrs(task, job)?;
        CURRENT_SINGLETON_FIBER_SPAWN_PHASE.store(5, Ordering::Release);
        CURRENT_SINGLETON_FIBER_SPAWN_PHASE.store(6, Ordering::Release);
        Ok(handle)
    }

    pub fn spawn_planned_fiber<T>(
        &'static self,
        task: T,
    ) -> Result<CurrentFiberHandle<T::Output>, FiberError>
    where
        T: ExplicitFiberTask,
    {
        self.ensure_runnable_budget_for_fiber()?;
        let attributes = T::task_attributes()?;
        self.ensure_fiber_admission(attributes)?;
        let handle = self
            .fiber_runtime_borrow(
                None,
                attributes
                    .execution
                    .is_fiber()
                    .then(|| attributes.stack_class.size_bytes().get()),
            )?
            .spawn_with_attrs(attributes, move || task.run())?;
        Ok(handle)
    }

    /// Spawns one named fiber task using build-generated stack metadata.
    ///
    /// # Errors
    ///
    /// Returns an error when generated metadata is missing or the task cannot be admitted.
    #[cfg(not(feature = "critical-safe"))]
    pub fn spawn_generated_fiber<T>(
        &'static self,
        task: T,
    ) -> Result<CurrentFiberHandle<T::Output>, FiberError>
    where
        T: GeneratedExplicitFiberTask,
    {
        self.ensure_runnable_budget_for_fiber()?;
        let attributes = T::task_attributes()?;
        self.ensure_fiber_admission(attributes)?;
        let handle = self
            .fiber_runtime_borrow(
                None,
                attributes
                    .execution
                    .is_fiber()
                    .then(|| attributes.stack_class.size_bytes().get()),
            )?
            .spawn_with_attrs(attributes, move || task.run())?;
        Ok(handle)
    }

    /// Spawns one named fiber task using a compile-time generated contract.
    ///
    /// # Errors
    ///
    /// Returns an error when the generated contract is not provisioned by the current runtime.
    #[cfg(feature = "critical-safe")]
    pub fn spawn_generated_fiber<T>(
        &'static self,
        task: T,
    ) -> Result<CurrentFiberHandle<T::Output>, FiberError>
    where
        T: GeneratedExplicitFiberTask + GeneratedExplicitFiberTaskContract,
    {
        self.ensure_runnable_budget_for_fiber()?;
        let attributes = generated_explicit_task_contract_attributes::<T>()
            .with_optional_yield_budget(T::YIELD_BUDGET);
        self.ensure_fiber_admission(attributes)?;
        let handle = self
            .fiber_runtime_borrow(
                None,
                attributes
                    .execution
                    .is_fiber()
                    .then(|| attributes.stack_class.size_bytes().get()),
            )?
            .spawn_with_attrs(attributes, move || task.run())?;
        Ok(handle)
    }

    pub(crate) fn pump_fiber_once(&'static self) -> Result<bool, FiberError> {
        self.fiber_runtime_if_initialized()?
            .map_or(Ok(false), |runtime| runtime.pump_once())
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
        self.ensure_runnable_budget_for_async()?;
        let handle = self.async_runtime_for_spawn()?.spawn(future)?;
        Ok(handle)
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
        self.ensure_runnable_budget_for_async()?;
        let handle = self
            .async_runtime_for_spawn()?
            .spawn_with_poll_stack_bytes(poll_stack_bytes, future)?;
        Ok(handle)
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
            if !self.pump_async_once()?
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
            if !self.pump_async_once()?
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
