include!("state.rs");

struct AsyncTaskSlot {
    generation: AtomicUsize,
    core: ExecutorCell<Option<ControlLease<ExecutorCore>>>,
    #[cfg(feature = "debug-insights")]
    task_id: ExecutorCell<Option<TaskId>>,
    future: ExecutorCell<InlineAsyncFutureStorage>,
    result: ExecutorCell<InlineAsyncResultStorage>,
    state: AtomicU8,
    error: ExecutorCell<Option<ExecutorError>>,
    join_waker: ExecutorCell<Option<Waker>>,
    completed: ExecutorCell<Option<Semaphore>>,
    run_state: AtomicU8,
    handle_live: AtomicBool,
    waker_refs: AtomicUsize,
    waker: AsyncTaskWakerData,
}

impl fmt::Debug for AsyncTaskSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsyncTaskSlot")
            .field("generation", &self.generation.load(Ordering::Acquire))
            .field("state", &self.state.load(Ordering::Acquire))
            .field("run_state", &self.run_state.load(Ordering::Acquire))
            .field("handle_live", &self.handle_live.load(Ordering::Acquire))
            .field("waker_refs", &self.waker_refs.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

const SLOT_RUN_SCHEDULED: u8 = 0b01;
const SLOT_RUN_RUNNING: u8 = 0b10;

impl AsyncTaskSlot {
    fn new(slot_index: usize, fast: bool) -> Result<Self, ExecutorError> {
        Ok(Self {
            generation: AtomicUsize::new(0),
            core: ExecutorCell::new(fast, None),
            #[cfg(feature = "debug-insights")]
            task_id: ExecutorCell::new(fast, None),
            future: ExecutorCell::new(fast, InlineAsyncFutureStorage::empty()),
            result: ExecutorCell::new(fast, InlineAsyncResultStorage::empty()),
            state: AtomicU8::new(SLOT_EMPTY),
            error: ExecutorCell::new(fast, None),
            join_waker: ExecutorCell::new(fast, None),
            completed: ExecutorCell::new(fast, None),
            run_state: AtomicU8::new(0),
            handle_live: AtomicBool::new(false),
            waker_refs: AtomicUsize::new(0),
            waker: AsyncTaskWakerData::new(slot_index),
        })
    }

    fn clear_run_state(&self) {
        self.run_state.store(0, Ordering::Release);
    }

    fn is_running(&self) -> bool {
        self.run_state.load(Ordering::Acquire) & SLOT_RUN_RUNNING != 0
    }

    fn try_mark_scheduled(&self) -> bool {
        let mut state = self.run_state.load(Ordering::Acquire);
        loop {
            if state & SLOT_RUN_SCHEDULED != 0 {
                return false;
            }
            match self.run_state.compare_exchange(
                state,
                state | SLOT_RUN_SCHEDULED,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(previous) => return previous & SLOT_RUN_RUNNING == 0,
                Err(current) => state = current,
            }
        }
    }

    fn begin_run(&self) -> bool {
        let mut state = self.run_state.load(Ordering::Acquire);
        loop {
            if state & SLOT_RUN_RUNNING != 0 {
                return false;
            }
            match self.run_state.compare_exchange(
                state,
                (state | SLOT_RUN_RUNNING) & !SLOT_RUN_SCHEDULED,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(current) => state = current,
            }
        }
    }

    fn mark_self_requeue(&self) {
        self.run_state
            .fetch_or(SLOT_RUN_SCHEDULED, Ordering::AcqRel);
    }

    fn finish_pending_run(&self) -> bool {
        let mut state = self.run_state.load(Ordering::Acquire);
        loop {
            let scheduled = state & SLOT_RUN_SCHEDULED != 0;
            match self.run_state.compare_exchange(
                state,
                state & !SLOT_RUN_RUNNING,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return scheduled,
                Err(current) => state = current,
            }
        }
    }

    fn bind_core(
        &self,
        core: &ControlLease<ExecutorCore>,
        generation: u64,
    ) -> Result<(), ExecutorError> {
        if self.generation() != generation || self.state() == SLOT_EMPTY {
            return Err(ExecutorError::Stopped);
        }
        self.core.with(|slot| {
            *slot = Some(core.try_clone().map_err(executor_error_from_alloc)?);
            Ok::<(), ExecutorError>(())
        })??;
        self.waker.set_core(core.as_ptr());
        Ok(())
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire) as u64
    }

    fn state(&self) -> u8 {
        self.state.load(Ordering::Acquire)
    }

    fn initialize_for_allocation(
        &self,
        spill_store: &AsyncTaskSpillStore,
    ) -> Result<u64, ExecutorError> {
        if self.state() != SLOT_EMPTY {
            return Err(executor_invalid());
        }

        let previous = self
            .generation
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current: usize| {
                current.checked_add(1)
            })
            .map_err(|_| executor_overflow())?;
        let generation = previous.checked_add(1).ok_or_else(executor_overflow)? as u64;

        self.future.with(|future| future.clear(spill_store))??;
        self.result.with(|result| result.clear(spill_store))??;
        self.error.with(|error| *error = None)?;
        #[cfg(feature = "debug-insights")]
        self.task_id.with(|task_id| *task_id = None)?;
        self.join_waker.with(|waker| *waker = None)?;
        self.drain_completed()?;
        self.clear_run_state();
        self.handle_live.store(true, Ordering::Release);
        self.waker_refs.store(0, Ordering::Release);
        self.waker.set_generation(generation);
        self.state.store(SLOT_PENDING, Ordering::Release);
        Ok(generation)
    }

    fn store_future<F>(
        &self,
        spill_store: &AsyncTaskSpillStore,
        future: F,
    ) -> Result<(), ExecutorError>
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        self.future
            .with(|slot| slot.store_future(spill_store, future))?
    }

    #[cfg(feature = "debug-insights")]
    fn set_task_id(&self, task_id: TaskId) -> Result<(), ExecutorError> {
        self.task_id.with(|slot| *slot = Some(task_id))?;
        Ok(())
    }

    #[cfg(feature = "debug-insights")]
    fn task_id(&self) -> Option<TaskId> {
        self.task_id.with_ref(|slot| *slot).ok().flatten()
    }

    fn create_waker(&self, generation: u64) -> Result<Waker, ExecutorError> {
        if self.generation() != generation
            || self.waker.generation() != generation
            || self.state() == SLOT_EMPTY
        {
            return Err(ExecutorError::Stopped);
        }
        let core_ptr = self.waker.core_ptr();
        if core_ptr.is_null() {
            return Err(ExecutorError::Stopped);
        }
        self.waker_refs
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current: usize| {
                current.checked_add(1)
            })
            .map_err(|_| executor_overflow())?;

        let raw = RawWaker::new(
            ::core::ptr::from_ref(&self.waker).cast::<()>(),
            &ASYNC_TASK_WAKER_VTABLE,
        );
        Ok(unsafe { Waker::from_raw(raw) })
    }

    fn poll_in_place(
        &self,
        spill_store: &AsyncTaskSpillStore,
        generation: u64,
        context: &mut Context<'_>,
    ) -> Result<Poll<()>, ExecutorError> {
        if self.generation() != generation || self.state() != SLOT_PENDING {
            return Ok(Poll::Ready(()));
        }
        self.future
            .with(|future| future.poll_in_place(&self.result, spill_store, context))?
    }

    fn complete(
        &self,
        spill_store: &AsyncTaskSpillStore,
        generation: u64,
    ) -> Result<(), ExecutorError> {
        if self.generation() != generation {
            return Ok(());
        }
        if self
            .state
            .compare_exchange(
                SLOT_PENDING,
                SLOT_READY,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            return Ok(());
        }

        self.future.with(|future| future.clear(spill_store))??;
        self.error.with(|error| *error = None)?;
        self.clear_run_state();
        self.wake_join_waker()?;
        self.signal_completed()?;
        Ok(())
    }

    fn fail(
        &self,
        spill_store: &AsyncTaskSpillStore,
        generation: u64,
        error: ExecutorError,
    ) -> Result<(), ExecutorError> {
        if self.generation() != generation {
            return Ok(());
        }
        if self
            .state
            .compare_exchange(
                SLOT_PENDING,
                SLOT_FAILED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            return Ok(());
        }

        self.future.with(|future| future.clear(spill_store))??;
        self.result.with(|result| result.clear(spill_store))??;
        self.error.with(|slot| *slot = Some(error))?;
        self.clear_run_state();
        self.wake_join_waker()?;
        self.signal_completed()?;
        Ok(())
    }

    fn cancel(
        &self,
        spill_store: &AsyncTaskSpillStore,
        generation: u64,
    ) -> Result<(), ExecutorError> {
        self.fail(spill_store, generation, ExecutorError::Cancelled)
    }

    fn clear_core_if_no_wakers(&self, generation: u64) -> Result<bool, ExecutorError> {
        if self.generation() != generation {
            return Err(ExecutorError::Stopped);
        }
        if self.waker_refs.load(Ordering::Acquire) != 0 {
            return Ok(false);
        }
        self.core.with(|core| *core = None)?;
        self.waker.set_core(::core::ptr::null());
        Ok(true)
    }

    fn force_shutdown(
        &self,
        spill_store: &AsyncTaskSpillStore,
        generation: u64,
    ) -> Result<(), ExecutorError> {
        if self.generation() != generation {
            return Ok(());
        }

        match self.state() {
            SLOT_PENDING => {
                let _ = self.fail(spill_store, generation, ExecutorError::Stopped);
            }
            SLOT_READY | SLOT_FAILED | SLOT_EMPTY => {}
            _ => return Err(executor_invalid()),
        }

        self.clear_run_state();
        let _ = self.clear_core_if_no_wakers(generation)?;
        Ok(())
    }

    fn is_finished(&self, generation: u64) -> Result<bool, ExecutorError> {
        if self.generation() != generation {
            return Err(ExecutorError::Stopped);
        }
        Ok(matches!(self.state(), SLOT_READY | SLOT_FAILED))
    }

    fn take_result<T: 'static>(
        &self,
        spill_store: &AsyncTaskSpillStore,
        generation: u64,
    ) -> Result<T, ExecutorError> {
        if self.generation() != generation {
            return Err(ExecutorError::Stopped);
        }
        match self.state() {
            SLOT_READY => self.result.with(|result| result.take::<T>(spill_store))?,
            SLOT_FAILED => Err(self
                .error
                .with(Option::take)?
                .ok_or(ExecutorError::Stopped)?),
            SLOT_PENDING | SLOT_EMPTY => Err(ExecutorError::Stopped),
            _ => Err(executor_invalid()),
        }
    }

    fn mark_handle_released(&self, generation: u64) -> Result<(), ExecutorError> {
        if self.generation() != generation {
            return Err(ExecutorError::Stopped);
        }
        self.handle_live.store(false, Ordering::Release);
        Ok(())
    }

    fn can_recycle(&self, generation: u64) -> Result<bool, ExecutorError> {
        if self.generation() != generation {
            return Err(ExecutorError::Stopped);
        }
        let state = self.state();
        Ok(!self.handle_live.load(Ordering::Acquire)
            && self.waker_refs.load(Ordering::Acquire) == 0
            && !self.is_running()
            && matches!(state, SLOT_READY | SLOT_FAILED))
    }

    fn reset_empty(
        &self,
        spill_store: &AsyncTaskSpillStore,
        generation: u64,
    ) -> Result<(), ExecutorError> {
        if self.generation() != generation {
            return Err(ExecutorError::Stopped);
        }

        self.future.with(|future| future.clear(spill_store))??;
        self.result.with(|result| result.clear(spill_store))??;
        self.error.with(|error| *error = None)?;
        #[cfg(feature = "debug-insights")]
        self.task_id.with(|task_id| *task_id = None)?;
        self.join_waker.with(|waker| *waker = None)?;
        self.drain_completed()?;
        self.clear_run_state();
        self.handle_live.store(false, Ordering::Release);
        self.state.store(SLOT_EMPTY, Ordering::Release);
        self.core.with(|core| *core = None)?;
        self.waker.set_core(::core::ptr::null());
        Ok(())
    }

    fn drain_completed(&self) -> Result<(), ExecutorError> {
        self.completed.with_ref(|completed| {
            let Some(completed) = completed.as_ref() else {
                return Ok(());
            };
            while completed.try_acquire().map_err(executor_error_from_sync)? {}
            Ok(())
        })?
    }

    fn ensure_completed_semaphore(&self) -> Result<(), ExecutorError> {
        self.completed.with(|completed| {
            if completed.is_none() {
                let semaphore = Semaphore::new(0, 1).map_err(executor_error_from_sync)?;
                if matches!(self.state(), SLOT_READY | SLOT_FAILED) {
                    semaphore.release(1).map_err(executor_error_from_sync)?;
                }
                *completed = Some(semaphore);
            }
            Ok::<(), ExecutorError>(())
        })?
    }

    fn signal_completed(&self) -> Result<(), ExecutorError> {
        self.completed.with_ref(|completed| {
            if let Some(completed) = completed.as_ref() {
                completed.release(1).map_err(executor_error_from_sync)?;
            }
            Ok(())
        })?
    }

    fn wait_completed(&self) -> Result<(), ExecutorError> {
        self.ensure_completed_semaphore()?;
        let completed = self.completed.with_ref(|completed| {
            completed
                .as_ref()
                .map(|completed| ::core::ptr::from_ref(completed))
                .ok_or_else(executor_invalid)
        })??;
        // SAFETY: the slot keeps its completion semaphore allocated for the active generation.
        unsafe { completed.as_ref() }
            .ok_or_else(executor_invalid)?
            .acquire()
            .map_err(executor_error_from_sync)
    }

    fn register_join_waker(&self, generation: u64, waker: &Waker) -> Result<(), ExecutorError> {
        if self.generation() != generation || self.state() == SLOT_EMPTY {
            return Err(ExecutorError::Stopped);
        }
        self.join_waker.with(|slot| {
            if slot
                .as_ref()
                .is_some_and(|current| current.will_wake(waker))
            {
                return;
            }
            *slot = Some(waker.clone());
        })
    }

    fn wake_join_waker(&self) -> Result<(), ExecutorError> {
        if let Some(waker) = self.join_waker.with(Option::take)? {
            waker.wake();
        }
        Ok(())
    }

    fn release_waker_ref(&self, generation: u64) -> Result<bool, ExecutorError> {
        if self.generation() != generation {
            return Ok(false);
        }
        let previous = self
            .waker_refs
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current: usize| {
                current.checked_sub(1)
            })
            .map_err(|_| executor_invalid())?;
        Ok(previous == 1)
    }
}

struct AsyncTaskRegistry {
    slots: ArenaSlice<AsyncTaskSlot>,
    free: ExecutorCell<FixedIndexStack>,
    spill_store: AsyncTaskSpillStore,
    _arena: BoundedArena,
}

impl fmt::Debug for AsyncTaskRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsyncTaskRegistry")
            .field("capacity", &self.slots.len())
            .finish_non_exhaustive()
    }
}

impl AsyncTaskRegistry {
    fn new(
        capacity: usize,
        fast: bool,
        allocators: &mut ExecutorBackingAllocators,
    ) -> Result<Self, ExecutorError> {
        let arena_capacity = executor_registry_capacity(capacity)?;
        let arena = allocators
            .registry
            .arena(arena_capacity, executor_registry_align())?;
        let slots = match arena
            .try_alloc_array_with(capacity, |slot_index| AsyncTaskSlot::new(slot_index, fast))
        {
            Ok(slots) => slots,
            Err(ArenaInitError::Alloc(error)) => return Err(executor_error_from_alloc(error)),
            Err(ArenaInitError::Init(error)) => return Err(error),
        };
        let free = FixedIndexStack::new_in(&arena, capacity)?;
        Ok(Self {
            slots,
            free: ExecutorCell::new(fast, free),
            spill_store: AsyncTaskSpillStore::new(fast, allocators.spill.take()),
            _arena: arena,
        })
    }

    fn slot(&self, slot_index: usize) -> Result<&AsyncTaskSlot, ExecutorError> {
        self.slots.get(slot_index).ok_or_else(executor_invalid)
    }

    fn allocate_slot(&self) -> Result<(usize, u64), ExecutorError> {
        let slot_index = self
            .free
            .with(FixedIndexStack::pop)?
            .ok_or_else(executor_busy)?;
        let generation = self
            .slot(slot_index)?
            .initialize_for_allocation(&self.spill_store)?;
        Ok((slot_index, generation))
    }

    fn release_slot(&self, slot_index: usize, generation: u64) -> Result<(), ExecutorError> {
        let slot = self.slot(slot_index)?;
        if slot.generation() != generation || slot.state() != SLOT_EMPTY {
            return Err(executor_invalid());
        }
        self.free.with(|free| {
            if free.contains(slot_index) {
                // Teardown can converge through multiple honest paths, such as handle detachment
                // racing the final task-waker release after the slot has already been reset.
                return Ok(());
            }
            free.push(slot_index)
        })?
    }

    fn available_slots(&self) -> Result<usize, ExecutorError> {
        self.free.with_ref(|free| free.len)
    }

    fn unfinished_task_count(&self) -> Result<usize, ExecutorError> {
        let mut count = 0usize;
        for slot in &self.slots {
            let generation = slot.generation();
            let state = slot.state();
            if generation == 0 || state == SLOT_EMPTY {
                continue;
            }
            if !slot.is_finished(generation)? {
                count = count.saturating_add(1);
            }
        }
        Ok(count)
    }

    fn scheduled_task_count(&self) -> usize {
        let mut count = 0usize;
        for slot in &self.slots {
            if slot.generation() == 0 || slot.state() != SLOT_PENDING {
                continue;
            }
            if slot.run_state.load(Ordering::Acquire) & SLOT_RUN_SCHEDULED != 0 {
                count = count.saturating_add(1);
            }
        }
        count
    }

    fn running_task_count(&self) -> usize {
        let mut count = 0usize;
        for slot in &self.slots {
            if slot.generation() == 0 || slot.state() != SLOT_PENDING {
                continue;
            }
            if slot.is_running() {
                count = count.saturating_add(1);
            }
        }
        count
    }
}

impl Drop for AsyncTaskRegistry {
    fn drop(&mut self) {
        for slot in &self.slots {
            let generation = slot.generation();
            if generation == 0 {
                continue;
            }
            let _ = slot.force_shutdown(&self.spill_store, generation);
            let _ = slot.reset_empty(&self.spill_store, generation);
        }
    }
}

#[derive(Debug)]
enum SchedulerBinding {
    Current,
    #[cfg(not(feature = "std"))]
    ThreadPool(ThreadPool),
    #[cfg(feature = "std")]
    ThreadWorkers(Arc<HostedThreadScheduler>),
    GreenPool(GreenPool),
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AsyncSlotRunDisposition {
    Terminal,
    Pending,
    PendingRequeue,
}

#[cfg(feature = "std")]
#[derive(Debug)]
struct HostedThreadScheduler {
    ready: SyncMutex<HostedReadyQueueState>,
    signal: Semaphore,
    shutdown: AtomicBool,
    worker_count: usize,
}

#[cfg(feature = "std")]
#[derive(Debug)]
struct HostedThreadWorkers {
    scheduler: Arc<HostedThreadScheduler>,
    handles: Vec<ThreadHandle>,
    system: ThreadSystem,
}

#[cfg(feature = "std")]
#[derive(Debug)]
enum ThreadAsyncCarriers {
    Direct(HostedThreadWorkers),
    ThreadPool(ThreadPool),
}

/// Hosted thread-async runtime bootstrap realization.
#[cfg(feature = "std")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadAsyncRuntimeBootstrap {
    /// Hosted async workers are born directly as long-lived OS-thread carriers.
    DirectHostedWorkers,
    /// Hosted async workers are composed on top of one generic thread pool.
    ComposedThreadPool,
}

#[cfg(feature = "std")]
impl HostedThreadScheduler {
    fn new(pool: &ThreadPool) -> Result<Arc<Self>, ExecutorError> {
        let worker_count = pool
            .stats()
            .map_err(executor_error_from_thread_pool)?
            .max_threads
            .max(1);
        let scheduler = Arc::new(Self {
            ready: SyncMutex::new(HostedReadyQueueState::new()),
            signal: Semaphore::new(
                0,
                u32::try_from(CURRENT_QUEUE_CAPACITY.saturating_add(worker_count))
                    .unwrap_or(u32::MAX),
            )
            .map_err(executor_error_from_sync)?,
            shutdown: AtomicBool::new(false),
            worker_count,
        });
        for _ in 0..worker_count {
            let worker = Arc::clone(&scheduler);
            pool.submit(move || run_hosted_thread_scheduler(&worker))
                .map_err(executor_error_from_thread_pool)?;
        }
        Ok(scheduler)
    }

    fn new_direct(
        config: &super::ThreadPoolConfig<'_>,
    ) -> Result<HostedThreadWorkers, ExecutorError> {
        let worker_count = config.max_threads.max(1);
        let scheduler = Arc::new(Self {
            ready: SyncMutex::new(HostedReadyQueueState::new()),
            signal: Semaphore::new(
                0,
                u32::try_from(CURRENT_QUEUE_CAPACITY.saturating_add(worker_count))
                    .unwrap_or(u32::MAX),
            )
            .map_err(executor_error_from_sync)?,
            shutdown: AtomicBool::new(false),
            worker_count,
        });
        let mut handles = Vec::with_capacity(worker_count);

        let system = ThreadSystem::new();
        let thread_config = ThreadConfig {
            join_policy: ThreadJoinPolicy::Joinable,
            name: config.name_prefix,
            start_mode: ThreadStartMode::Immediate,
            placement: ThreadPlacementRequest::new(),
            scheduler: config.scheduler,
            stack: config.stack,
        };

        for _ in 0..worker_count {
            let scheduler_context = Arc::into_raw(Arc::clone(&scheduler));
            let handle = unsafe {
                system.spawn_raw(
                    &thread_config,
                    hosted_thread_scheduler_entry,
                    scheduler_context.cast_mut().cast(),
                )
            };
            match handle {
                Ok(handle) => handles.push(handle),
                Err(error) => {
                    unsafe {
                        drop(Arc::from_raw(scheduler_context));
                    }
                    let _ = scheduler.request_shutdown();
                    for handle in handles.drain(..) {
                        let _ = system.join(handle);
                    }
                    return Err(executor_error_from_thread(error));
                }
            }
        }

        Ok(HostedThreadWorkers {
            scheduler,
            handles,
            system,
        })
    }

    fn enqueue(&self, job: CurrentJob) -> Result<(), ExecutorError> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(ExecutorError::Stopped);
        }
        let mut ready = self.ready.lock().map_err(executor_error_from_sync)?;
        ready.enqueue(job)?;
        drop(ready);
        self.signal.release(1).map_err(executor_error_from_sync)
    }

    fn request_shutdown(&self) -> Result<usize, ExecutorError> {
        self.shutdown.store(true, Ordering::Release);
        let dropped = self.ready.lock().map_err(executor_error_from_sync)?.clear();
        self.signal
            .release(u32::try_from(self.worker_count).unwrap_or(u32::MAX))
            .map_err(executor_error_from_sync)?;
        Ok(dropped)
    }
}

#[cfg(feature = "std")]
impl HostedThreadWorkers {
    fn direct_supported(config: &super::ThreadPoolConfig<'_>) -> bool {
        matches!(config.placement, super::PoolPlacement::Inherit)
            && matches!(config.resize_policy, super::ResizePolicy::Fixed)
            && matches!(config.shutdown_policy, super::ShutdownPolicy::Drain)
            && config.min_threads == config.max_threads
            && config.min_threads != 0
    }
    fn shutdown_and_join(&mut self) {
        let _ = self.scheduler.request_shutdown();
        for handle in self.handles.drain(..) {
            let _ = self.system.join(handle);
        }
    }
}

#[derive(Clone, Copy)]
struct ScheduledExecutorCorePtr(NonNull<ExecutorCore>);

impl ScheduledExecutorCorePtr {
    fn from_ref(core: &ExecutorCore) -> Self {
        Self(NonNull::from(core))
    }

    fn run_slot(self, slot_index: usize, generation: u64) {
        // SAFETY: scheduler jobs only capture this handle from a live `ExecutorCore` and use it
        // immediately to route back into the same executor's slot table.
        unsafe { self.0.as_ref().run_slot_by_ref(slot_index, generation) };
    }
}

// SAFETY: scheduled jobs move this wrapper between carriers, but only to call back into the
// originating executor core. The explicit wrapper is safer than laundering the pointer through
// `usize`, while lifetime validity remains the surrounding executor's invariant.
unsafe impl Send for ScheduledExecutorCorePtr {}

impl SchedulerBinding {
    const fn uses_external_carrier(&self) -> bool {
        match self {
            Self::Current | Self::Unsupported => false,
            #[cfg(not(feature = "std"))]
            Self::ThreadPool(_) => true,
            #[cfg(feature = "std")]
            Self::ThreadWorkers(_) => true,
            Self::GreenPool(_) => true,
        }
    }

    fn schedule_slot(
        &self,
        core: &ExecutorCore,
        slot_index: usize,
        generation: u64,
    ) -> Result<(), ExecutorError> {
        match self {
            Self::Current => core
                .current_queue
                .schedule_slot(core, slot_index, generation),
            #[cfg(feature = "std")]
            Self::ThreadWorkers(queue) => queue.enqueue(CurrentJob {
                run: run_current_slot,
                core: ::core::ptr::from_ref(core) as usize,
                slot_index,
                generation,
            }),
            #[cfg(not(feature = "std"))]
            Self::ThreadPool(pool) => {
                let core = ScheduledExecutorCorePtr::from_ref(core);
                pool.submit(move || run_scheduled_slot_ptr(core, slot_index, generation))
                    .map_err(|_| ExecutorError::Stopped)
            }
            Self::GreenPool(pool) => {
                let core = ScheduledExecutorCorePtr::from_ref(core);
                pool.spawn(move || run_scheduled_slot_ptr(core, slot_index, generation))
                    .map(|_| ())
                    .map_err(|_| ExecutorError::Stopped)
            }
            Self::Unsupported => Err(ExecutorError::Unsupported),
        }
    }
}

#[derive(Debug)]
struct ExecutorRegistry {
    ready: Option<AsyncTaskRegistry>,
    error: Option<ExecutorError>,
}

impl ExecutorRegistry {
    fn new(capacity: usize, fast: bool, allocators: &mut ExecutorBackingAllocators) -> Self {
        match AsyncTaskRegistry::new(capacity, fast, allocators) {
            Ok(registry) => Self {
                ready: Some(registry),
                error: None,
            },
            Err(error) => Self {
                ready: None,
                error: Some(error),
            },
        }
    }

    fn get(&self) -> Result<&AsyncTaskRegistry, ExecutorError> {
        if let Some(registry) = self.ready.as_ref() {
            return Ok(registry);
        }
        Err(self.error.unwrap_or_else(executor_invalid))
    }
}

struct ExecutorCore {
    courier_id: Option<CourierId>,
    context_id: Option<ContextId>,
    runtime_sink: Option<CourierRuntimeSink>,
    reactor: Reactor,
    reactor_max_events: Option<usize>,
    current_queue: CurrentQueue,
    reactor_state: ExecutorReactorState,
    reactor_driver_ready: AtomicBool,
    #[cfg(feature = "std")]
    reactor_driver: ExecutorCell<Option<Arc<ExecutorReactorDriverState>>>,
    scheduler: SchedulerBinding,
    next_id: AtomicUsize,
    registry: ExecutorRegistry,
    #[cfg(feature = "debug-insights")]
    task_lifecycle: ExecutorCell<Option<AsyncTaskLifecycleInsightState>>,
    shutdown_requested: AtomicBool,
    external_inflight: AtomicUsize,
    _owned_backing: Option<ExtentLease>,
}

impl fmt::Debug for ExecutorCore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecutorCore")
            .field("scheduler", &self.scheduler)
            .finish_non_exhaustive()
    }
}

impl ExecutorCore {
    fn runtime_tick(&self) -> u64 {
        match runtime_monotonic_raw_now() {
            Ok(fusion_sys::thread::MonotonicRawInstant::Bits32(raw)) => u64::from(raw),
            Ok(fusion_sys::thread::MonotonicRawInstant::Bits64(raw)) => raw,
            Err(_) => 0,
        }
    }

    fn publish_runtime_context(&self) -> Result<(), ExecutorError> {
        let (Some(runtime_sink), Some(courier_id), Some(context_id)) =
            (self.runtime_sink, self.courier_id, self.context_id)
        else {
            return Ok(());
        };
        runtime_sink
            .record_context(courier_id, context_id, self.runtime_tick())
            .map_err(executor_error_from_runtime_sink)
    }

    fn publish_runtime_summary(&self) -> Result<(), ExecutorError> {
        let (Some(runtime_sink), Some(courier_id)) = (self.runtime_sink, self.courier_id) else {
            return Ok(());
        };
        let registry = self.registry()?;
        let active_units = registry.unfinished_task_count()?;
        let runnable_units = registry.scheduled_task_count();
        let running_units = registry.running_task_count();
        let blocked_units =
            active_units.saturating_sub(runnable_units.saturating_add(running_units));
        let available_slots = registry.available_slots()?;
        let responsiveness = runtime_sink
            .evaluate_responsiveness(courier_id, self.runtime_tick())
            .map_err(executor_error_from_runtime_sink)?;
        let summary = CourierRuntimeSummary::new(
            match self.scheduler {
                SchedulerBinding::Current | SchedulerBinding::GreenPool(_) => {
                    CourierSchedulingPolicy::CooperativePriority
                }
                #[cfg(feature = "std")]
                SchedulerBinding::ThreadWorkers(_) => {
                    CourierSchedulingPolicy::CooperativeRoundRobin
                }
                #[cfg(not(feature = "std"))]
                SchedulerBinding::ThreadPool(_) => CourierSchedulingPolicy::CooperativeRoundRobin,
                SchedulerBinding::Unsupported => CourierSchedulingPolicy::CooperativePriority,
            },
            responsiveness,
        )
        .with_async_lane(CourierLaneSummary {
            kind: RunnableUnitKind::AsyncTask,
            active_units,
            runnable_units,
            running_units,
            blocked_units,
            available_slots,
        });
        runtime_sink
            .record_runtime_summary(courier_id, summary, self.runtime_tick())
            .map_err(executor_error_from_runtime_sink)
    }

    #[cfg(feature = "std")]
    fn ensure_reactor_driver(&self) -> Result<(), ExecutorError> {
        if !matches!(self.scheduler, SchedulerBinding::ThreadWorkers(_)) {
            return Ok(());
        }
        if self.reactor_driver_ready.load(Ordering::Acquire) {
            return Ok(());
        }
        let driver = self
            .reactor_driver
            .with_ref(|driver| driver.as_ref().cloned())?
            .ok_or(ExecutorError::Unsupported)?;
        driver.ensure_started(&self.reactor_state, &self.reactor_driver_ready)
    }

    #[cfg(feature = "std")]
    fn join_reactor_driver(&self) {
        if let Ok(Some(driver)) = self
            .reactor_driver
            .with_ref(|driver| driver.as_ref().cloned())
        {
            driver.join();
        }
    }

    fn allocate_task_id(&self) -> Result<TaskId, ExecutorError> {
        let sequence = self
            .next_id
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current: usize| {
                current.checked_add(1)
            })
            .map_err(|_| executor_overflow())?;
        Ok(TaskId::new(
            ::core::ptr::from_ref(self) as usize,
            sequence as u64,
        ))
    }

    fn registry(&self) -> Result<&AsyncTaskRegistry, ExecutorError> {
        self.registry.get()
    }

    #[cfg(feature = "debug-insights")]
    fn ensure_task_lifecycle_insight(&self) -> Result<(), ExecutorError> {
        self.task_lifecycle.with(|state| {
            if state.is_none() {
                *state = Some(AsyncTaskLifecycleInsightState::new()?);
            }
            Ok::<(), ExecutorError>(())
        })?
    }

    #[cfg(feature = "debug-insights")]
    fn emit_task_lifecycle(&self, record: AsyncTaskLifecycleRecord) {
        let _ = self.task_lifecycle.with_ref(|state| {
            if let Some(state) = state.as_ref() {
                state.emit_if_observed(record);
            }
        });
    }

    #[cfg_attr(not(feature = "debug-insights"), allow(dead_code))]
    fn scheduler_tag(&self) -> AsyncTaskSchedulerTag {
        AsyncTaskSchedulerTag::from_scheduler(&self.scheduler)
    }

    fn register_readiness_wait(
        &self,
        slot_index: usize,
        generation: u64,
        source: EventSourceHandle,
        interest: EventInterest,
    ) -> Result<(), ExecutorError> {
        if matches!(self.scheduler, SchedulerBinding::GreenPool(_)) {
            return Err(ExecutorError::Unsupported);
        }
        #[cfg(feature = "std")]
        if matches!(self.scheduler, SchedulerBinding::ThreadWorkers(_)) {
            self.ensure_reactor_driver()?;
            return self
                .reactor_state
                .queue_readiness_wait(slot_index, generation, source, interest);
        }
        if self.scheduler.uses_external_carrier()
            && !self.reactor_driver_ready.load(Ordering::Acquire)
        {
            return Err(ExecutorError::Unsupported);
        }
        self.reactor_state.register_readiness_wait(
            self.reactor,
            slot_index,
            generation,
            source,
            interest,
        )
    }

    fn register_sleep_wait(
        &self,
        slot_index: usize,
        generation: u64,
        deadline: CanonicalInstant,
    ) -> Result<(), ExecutorError> {
        if matches!(self.scheduler, SchedulerBinding::GreenPool(_)) {
            return Err(ExecutorError::Unsupported);
        }
        #[cfg(feature = "std")]
        if matches!(self.scheduler, SchedulerBinding::ThreadWorkers(_)) {
            self.ensure_reactor_driver()?;
        }
        if self.scheduler.uses_external_carrier()
            && !self.reactor_driver_ready.load(Ordering::Acquire)
        {
            return Err(ExecutorError::Unsupported);
        }
        self.reactor_state
            .register_sleep_wait(slot_index, generation, deadline)
    }

    fn clear_wait(&self, slot_index: usize, generation: u64) -> Result<(), ExecutorError> {
        #[cfg(feature = "std")]
        if matches!(self.scheduler, SchedulerBinding::ThreadWorkers(_)) {
            return self
                .reactor_state
                .clear_wait_deferred(slot_index, generation);
        }
        self.reactor_state
            .clear_wait(self.reactor, slot_index, generation)
    }

    fn take_wait_outcome(
        &self,
        slot_index: usize,
        generation: u64,
    ) -> Result<Option<AsyncWaitOutcome>, ExecutorError> {
        let _ = generation;
        self.reactor_state.take_wait_outcome(slot_index)
    }

    fn begin_external_schedule(&self) -> Result<(), ExecutorError> {
        if self.shutdown_requested.load(Ordering::Acquire) {
            return Err(ExecutorError::Stopped);
        }
        self.external_inflight
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current: usize| {
                current.checked_add(1)
            })
            .map_err(|_| executor_overflow())?;
        if self.shutdown_requested.load(Ordering::Acquire) {
            self.finish_external_schedule();
            return Err(ExecutorError::Stopped);
        }
        Ok(())
    }

    fn finish_external_schedule(&self) {
        let previous = self.external_inflight.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(
            previous != 0,
            "external inflight count should not underflow"
        );
    }

    #[cfg_attr(not(feature = "std"), allow(dead_code))]
    fn drop_external_scheduled(&self, count: usize) {
        if count == 0 {
            return;
        }
        let previous = self.external_inflight.fetch_sub(count, Ordering::AcqRel);
        debug_assert!(previous >= count, "dropped jobs should be accounted for");
    }

    fn wait_external_idle(&self) {
        while self.external_inflight.load(Ordering::Acquire) != 0 {
            if system_thread().yield_now().is_err() {
                spin_loop();
            }
        }
    }

    fn schedule_slot(&self, slot_index: usize, generation: u64) -> Result<(), ExecutorError> {
        self.schedule_slot_with_lease(slot_index, generation, None)
    }

    fn schedule_slot_with_lease(
        &self,
        slot_index: usize,
        generation: u64,
        scheduled_core: Option<ControlLease<ExecutorCore>>,
    ) -> Result<(), ExecutorError> {
        let registry = self.registry()?;
        let slot = registry.slot(slot_index)?;
        if slot.generation() != generation || slot.state() != SLOT_PENDING {
            return Ok(());
        }
        if !slot.try_mark_scheduled() {
            return Ok(());
        }
        self.dispatch_marked_slot_with_lease(slot_index, generation, scheduled_core)
    }

    fn dispatch_marked_slot_with_lease(
        &self,
        slot_index: usize,
        generation: u64,
        scheduled_core: Option<ControlLease<ExecutorCore>>,
    ) -> Result<(), ExecutorError> {
        let registry = self.registry()?;
        let slot = registry.slot(slot_index)?;
        if slot.generation() != generation || slot.state() != SLOT_PENDING {
            return Ok(());
        }
        let tracked = self.scheduler.uses_external_carrier();
        if tracked && let Err(error) = self.begin_external_schedule() {
            slot.clear_run_state();
            let _ = slot.fail(&registry.spill_store, generation, error);
            let _ = self.recycle_slot_if_possible(slot_index, generation);
            return Err(error);
        }

        let schedule = match &self.scheduler {
            #[cfg(not(feature = "std"))]
            SchedulerBinding::ThreadPool(pool) => {
                let scheduled_core = match scheduled_core {
                    Some(ref lease) => lease.try_clone().map_err(executor_error_from_alloc)?,
                    None => slot.core.with_ref(|core| {
                        core.as_ref()
                            .ok_or(ExecutorError::Stopped)?
                            .try_clone()
                            .map_err(executor_error_from_alloc)
                    })??,
                };
                pool.submit(move || {
                    run_scheduled_slot_lease(scheduled_core, slot_index, generation)
                })
                .map_err(|_| ExecutorError::Stopped)
            }
            SchedulerBinding::GreenPool(pool) => {
                let scheduled_core = match scheduled_core {
                    Some(ref lease) => lease.try_clone().map_err(executor_error_from_alloc)?,
                    None => slot.core.with_ref(|core| {
                        core.as_ref()
                            .ok_or(ExecutorError::Stopped)?
                            .try_clone()
                            .map_err(executor_error_from_alloc)
                    })??,
                };
                pool.spawn_explicit(GreenExecutorDispatchTask {
                    core: scheduled_core,
                    slot_index,
                    generation,
                })
                .map(|_| ())
                .map_err(|_| ExecutorError::Stopped)
            }
            _ => self.scheduler.schedule_slot(self, slot_index, generation),
        };

        if let Err(error) = schedule {
            if tracked {
                self.finish_external_schedule();
            }
            slot.clear_run_state();
            let _ = slot.fail(&registry.spill_store, generation, error);
            let _ = self.recycle_slot_if_possible(slot_index, generation);
            return Err(error);
        }
        Ok(())
    }

    fn run_slot_by_ref(&self, slot_index: usize, generation: u64) -> AsyncSlotRunDisposition {
        let Ok(registry) = self.registry() else {
            return AsyncSlotRunDisposition::Terminal;
        };
        let Ok(slot) = registry.slot(slot_index) else {
            return AsyncSlotRunDisposition::Terminal;
        };
        if slot.generation() != generation || slot.state() != SLOT_PENDING {
            return AsyncSlotRunDisposition::Terminal;
        }
        if !slot.begin_run() {
            return AsyncSlotRunDisposition::Pending;
        }

        #[cfg(feature = "std")]
        let requeue_core = slot
            .core
            .with_ref(|core| core.as_ref().and_then(|lease| lease.try_clone().ok()))
            .ok()
            .flatten();
        #[cfg(feature = "debug-insights")]
        let task_id = slot.task_id();
        #[cfg(feature = "debug-insights")]
        let scheduler = self.scheduler_tag();

        let context_guard = AsyncTaskContextGuard::install(self, slot_index, generation);
        let poll = {
            let Ok(waker) = slot.create_waker(generation) else {
                let _ = slot.finish_pending_run();
                return AsyncSlotRunDisposition::Terminal;
            };
            let mut context = Context::from_waker(&waker);
            slot.poll_in_place(&registry.spill_store, generation, &mut context)
        };
        let self_requeue = take_current_async_requeue();
        #[cfg(feature = "std")]
        let self_requeue_core = requeue_core;
        drop(context_guard);

        match poll {
            Ok(Poll::Ready(())) => {
                #[cfg(feature = "debug-insights")]
                if let Some(task) = task_id {
                    self.emit_task_lifecycle(AsyncTaskLifecycleRecord::PolledReady {
                        task,
                        slot_index,
                        generation,
                        scheduler,
                    });
                }
                let _ = slot.finish_pending_run();
                let _ = slot.complete(&registry.spill_store, generation);
                #[cfg(feature = "debug-insights")]
                if let Some(task) = task_id {
                    self.emit_task_lifecycle(AsyncTaskLifecycleRecord::Completed {
                        task,
                        slot_index,
                        generation,
                        scheduler,
                    });
                }
                let _ = self.recycle_slot_if_possible(slot_index, generation);
                let _ = self.publish_runtime_summary();
                AsyncSlotRunDisposition::Terminal
            }
            Ok(Poll::Pending) => {
                #[cfg(feature = "debug-insights")]
                if let Some(task) = task_id {
                    self.emit_task_lifecycle(AsyncTaskLifecycleRecord::PolledPending {
                        task,
                        slot_index,
                        generation,
                        scheduler,
                    });
                }
                if self_requeue {
                    slot.mark_self_requeue();
                }
                if slot.finish_pending_run() {
                    if matches!(self.scheduler, SchedulerBinding::GreenPool(_)) {
                        return AsyncSlotRunDisposition::PendingRequeue;
                    }
                    // The slot is already marked scheduled. Replay that queued wake without
                    // clearing the marker first so racing external wakes cannot duplicate or lose
                    // the requeue.
                    let _ = self.dispatch_marked_slot_with_lease(slot_index, generation, {
                        #[cfg(feature = "std")]
                        {
                            self_requeue_core
                        }
                        #[cfg(not(feature = "std"))]
                        {
                            None
                        }
                    });
                }
                let _ = self.publish_runtime_summary();
                AsyncSlotRunDisposition::Pending
            }
            Err(error) => {
                let _ = slot.finish_pending_run();
                let _ = slot.fail(&registry.spill_store, generation, error);
                #[cfg(feature = "debug-insights")]
                if let Some(task) = task_id {
                    self.emit_task_lifecycle(AsyncTaskLifecycleRecord::Failed {
                        task,
                        slot_index,
                        generation,
                        scheduler,
                        error,
                    });
                }
                let _ = self.recycle_slot_if_possible(slot_index, generation);
                let _ = self.publish_runtime_summary();
                AsyncSlotRunDisposition::Terminal
            }
        }
    }

    fn drive_current_once(&self) -> Result<bool, ExecutorError> {
        match &self.scheduler {
            SchedulerBinding::Current => self.current_queue.run_next(),
            _ => Ok(false),
        }
    }

    fn drive_reactor_once(&self, blocking: bool) -> Result<bool, ExecutorError> {
        self.reactor_state
            .drive(self, blocking, self.reactor_max_events)
    }

    fn recycle_slot_if_possible(
        &self,
        slot_index: usize,
        generation: u64,
    ) -> Result<(), ExecutorError> {
        let registry = self.registry()?;
        let slot = registry.slot(slot_index)?;
        if !slot.can_recycle(generation)? {
            return Ok(());
        }
        self.clear_wait(slot_index, generation)?;
        slot.reset_empty(&registry.spill_store, generation)?;
        let released = registry.release_slot(slot_index, generation);
        let _ = self.publish_runtime_summary();
        released
    }

    fn detach_handle(&self, slot_index: usize, generation: u64) -> Result<(), ExecutorError> {
        let slot = self.registry()?.slot(slot_index)?;
        slot.mark_handle_released(generation)?;
        self.recycle_slot_if_possible(slot_index, generation)
    }

    fn shutdown(&self) {
        self.shutdown_requested.store(true, Ordering::Release);
        match &self.scheduler {
            SchedulerBinding::Current | SchedulerBinding::Unsupported => {}
            #[cfg(not(feature = "std"))]
            SchedulerBinding::ThreadPool(_) => {}
            #[cfg(feature = "std")]
            SchedulerBinding::ThreadWorkers(queue) => {
                if let Ok(dropped) = queue.request_shutdown() {
                    self.drop_external_scheduled(dropped);
                }
            }
            SchedulerBinding::GreenPool(_) => {}
        }
        self.wait_external_idle();
        let Ok(registry) = self.registry() else {
            return;
        };
        for slot in &registry.slots {
            let generation = slot.generation();
            if generation == 0 {
                continue;
            }
            let slot_index = slot.waker.slot_index;
            let _ = self.clear_wait(slot_index, generation);
            let _ = slot.force_shutdown(&registry.spill_store, generation);
        }
        #[cfg(feature = "std")]
        self.join_reactor_driver();
    }
}

unsafe fn clone_async_task_waker(data: *const ()) -> RawWaker {
    let waker = unsafe { &*data.cast::<AsyncTaskWakerData>() };
    let core_ptr = waker.core_ptr();
    if !core_ptr.is_null() {
        let core = unsafe { &*core_ptr };
        let generation = waker.generation();
        if let Ok(slot) = core
            .registry()
            .and_then(|registry| registry.slot(waker.slot_index))
            && slot.generation() == generation
            && slot
                .waker_refs
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current: usize| {
                    current.checked_add(1)
                })
                .is_ok()
        {
            return RawWaker::new(data, &ASYNC_TASK_WAKER_VTABLE);
        }
    }
    noop_async_task_raw_waker()
}

unsafe fn wake_async_task_waker(data: *const ()) {
    let waker = unsafe { &*data.cast::<AsyncTaskWakerData>() };
    let core_ptr = waker.core_ptr();
    if !core_ptr.is_null() {
        let core = unsafe { &*core_ptr };
        let generation = waker.generation();
        if let Ok(slot) = core
            .registry()
            .and_then(|registry| registry.slot(waker.slot_index))
        {
            let _ = core.schedule_slot(waker.slot_index, generation);
            if slot.release_waker_ref(generation).unwrap_or(false) {
                let _ = core.recycle_slot_if_possible(waker.slot_index, generation);
            }
        }
    }
}

unsafe fn wake_async_task_waker_by_ref(data: *const ()) {
    let waker = unsafe { &*data.cast::<AsyncTaskWakerData>() };
    let core_ptr = waker.core_ptr();
    if !core_ptr.is_null() {
        let core = unsafe { &*core_ptr };
        let generation = waker.generation();
        if let Ok(slot) = core
            .registry()
            .and_then(|registry| registry.slot(waker.slot_index))
            && slot.generation() == generation
        {
            let _ = core.schedule_slot(waker.slot_index, generation);
        }
    }
}

unsafe fn drop_async_task_waker(data: *const ()) {
    let waker = unsafe { &*data.cast::<AsyncTaskWakerData>() };
    let core_ptr = waker.core_ptr();
    if !core_ptr.is_null() {
        let core = unsafe { &*core_ptr };
        let generation = waker.generation();
        if let Ok(slot) = core
            .registry()
            .and_then(|registry| registry.slot(waker.slot_index))
            && slot.release_waker_ref(generation).unwrap_or(false)
        {
            let _ = core.recycle_slot_if_possible(waker.slot_index, generation);
        }
    }
}

const fn noop_async_task_raw_waker() -> RawWaker {
    RawWaker::new(::core::ptr::null(), &NOOP_ASYNC_TASK_WAKER_VTABLE)
}

const unsafe fn clone_noop_async_task_waker(_: *const ()) -> RawWaker {
    noop_async_task_raw_waker()
}

const unsafe fn wake_noop_async_task_waker(_: *const ()) {}

static ASYNC_TASK_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    clone_async_task_waker,
    wake_async_task_waker,
    wake_async_task_waker_by_ref,
    drop_async_task_waker,
);

static NOOP_ASYNC_TASK_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    clone_noop_async_task_waker,
    wake_noop_async_task_waker,
    wake_noop_async_task_waker,
    wake_noop_async_task_waker,
);
