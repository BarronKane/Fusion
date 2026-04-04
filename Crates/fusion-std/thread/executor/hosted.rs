/// Hosted async runtime backed by system-thread carriers.
#[derive(Debug)]
pub struct ThreadAsyncRuntime {
    executor: Option<Executor>,
    carriers: Option<ThreadAsyncCarriers>,
}
impl ThreadAsyncRuntime {
    /// Creates one hosted async runtime.
    ///
    /// # Errors
    ///
    /// Returns any honest carrier bootstrap or executor binding failure.
    pub fn new(config: &super::ThreadPoolConfig<'_>) -> Result<Self, ExecutorError> {
        Self::with_executor_config(config, ExecutorConfig::thread_pool())
    }

    /// Creates one hosted async runtime with one explicit executor configuration.
    ///
    /// # Errors
    ///
    /// Returns any honest carrier bootstrap or executor binding failure.
    pub fn with_executor_config(
        config: &super::ThreadPoolConfig<'_>,
        executor_config: ExecutorConfig,
    ) -> Result<Self, ExecutorError> {
        if HostedThreadWorkers::direct_supported(config) {
            let carriers = HostedThreadScheduler::new_direct(config)?;
            let executor = Executor::with_scheduler(
                executor_config.with_mode(ExecutorMode::ThreadPool),
                SchedulerBinding::ThreadWorkers(Arc::clone(&carriers.scheduler)),
                false,
            );
            return Ok(Self {
                executor: Some(executor),
                carriers: Some(ThreadAsyncCarriers::Direct(carriers)),
            });
        }

        let carriers = ThreadPool::new(config).map_err(executor_error_from_thread_pool)?;
        let executor = Executor::new(executor_config.with_mode(ExecutorMode::ThreadPool))
            .on_pool(&carriers)?;
        Ok(Self {
            executor: Some(executor),
            carriers: Some(ThreadAsyncCarriers::ThreadPool(carriers)),
        })
    }

    /// Returns how this hosted runtime bootstrapped its carriers.
    #[must_use]
    pub const fn bootstrap(&self) -> ThreadAsyncRuntimeBootstrap {
        match self.carriers.as_ref() {
            Some(ThreadAsyncCarriers::Direct(_)) => {
                ThreadAsyncRuntimeBootstrap::DirectHostedWorkers
            }
            Some(ThreadAsyncCarriers::ThreadPool(_)) | None => {
                ThreadAsyncRuntimeBootstrap::ComposedThreadPool
            }
        }
    }

    /// Returns the owned carrier thread pool when this runtime uses the composed hosted bootstrap.
    #[must_use]
    pub fn thread_pool(&self) -> Option<&ThreadPool> {
        match self.carriers.as_ref() {
            Some(ThreadAsyncCarriers::ThreadPool(pool)) => Some(pool),
            _ => None,
        }
    }

    /// Returns the underlying executor.
    #[must_use]
    pub fn executor(&self) -> &Executor {
        self.executor
            .as_ref()
            .expect("thread async runtime executor should exist while borrowed")
    }

    /// Returns the consumer-facing async task lifecycle insight lane for this hosted runtime.
    #[must_use]
    pub fn task_lifecycle_insight(&self) -> AsyncTaskLifecycleInsight<'_> {
        self.executor().task_lifecycle_insight()
    }

    /// Spawns one ordinary Rust future onto the thread-backed runtime.
    ///
    /// # Errors
    ///
    /// Returns any honest executor admission or scheduler failure.
    pub fn spawn<F>(&self, future: F) -> Result<TaskHandle<F::Output>, ExecutorError>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.executor().spawn(future)
    }

    /// Spawns one ordinary Rust future with one explicit poll-stack contract.
    ///
    /// # Errors
    ///
    /// Returns any honest executor admission or scheduler failure.
    pub fn spawn_with_poll_stack_bytes<F>(
        &self,
        poll_stack_bytes: usize,
        future: F,
    ) -> Result<TaskHandle<F::Output>, ExecutorError>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.executor()
            .spawn_with_poll_stack_bytes(poll_stack_bytes, future)
    }

    /// Spawns one ordinary Rust future using one compile-time generated async poll-stack
    /// contract.
    ///
    /// # Errors
    ///
    /// Returns any honest executor admission or scheduler failure.
    pub fn spawn_generated<F>(&self, future: F) -> Result<TaskHandle<F::Output>, ExecutorError>
    where
        F: Future + Send + 'static + GeneratedExplicitAsyncPollStackContract,
        F::Output: Send + 'static,
    {
        self.executor().spawn_generated(future)
    }

    /// Drives one future to completion on the hosted thread-backed runtime.
    ///
    /// # Errors
    ///
    /// Returns any honest executor admission or scheduler failure.
    ///
    /// This hosted path currently realizes `block_on(...)` by spawning one wrapper task and
    /// synchronously joining it, so the executor must have capacity for that one extra task.
    pub fn block_on<F>(&self, future: F) -> Result<F::Output, ExecutorError>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.spawn(future)?.join()
    }

    /// Drives one future to completion on the hosted thread-backed runtime with one explicit
    /// poll-stack contract.
    ///
    /// # Errors
    ///
    /// Returns any honest executor admission or scheduler failure.
    pub fn block_on_with_poll_stack_bytes<F>(
        &self,
        poll_stack_bytes: usize,
        future: F,
    ) -> Result<F::Output, ExecutorError>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.spawn_with_poll_stack_bytes(poll_stack_bytes, future)?
            .join()
    }

    /// Releases the owned carrier bootstrap and executor back to the caller.
    #[must_use]
    pub fn into_parts(mut self) -> (Option<ThreadPool>, Executor) {
        let executor = self
            .executor
            .take()
            .expect("thread async runtime executor should exist during into_parts");
        let carriers = match self.carriers.take() {
            Some(ThreadAsyncCarriers::ThreadPool(pool)) => Some(pool),
            _ => None,
        };
        (carriers, executor)
    }
}
impl Drop for ThreadAsyncRuntime {
    fn drop(&mut self) {
        drop(self.executor.take());
        if let Some(mut carriers) = self.carriers.take() {
            match &mut carriers {
                ThreadAsyncCarriers::Direct(workers) => workers.shutdown_and_join(),
                ThreadAsyncCarriers::ThreadPool(_) => {}
            }
        }
    }
}

/// Hosted async runtime backed by a hosted fiber carrier runtime.
#[cfg(feature = "std")]
#[derive(Debug)]
pub struct FiberAsyncRuntime {
    executor: Executor,
    fibers: HostedFiberRuntime,
}
impl FiberAsyncRuntime {
    /// Creates one async runtime from an owned hosted fiber runtime.
    ///
    /// # Errors
    ///
    /// Returns any honest executor binding failure.
    pub fn from_hosted_fibers(fibers: HostedFiberRuntime) -> Result<Self, ExecutorError> {
        Self::from_hosted_fibers_with_executor_config(fibers, ExecutorConfig::green_pool())
    }

    /// Creates one async runtime from an owned hosted fiber runtime with one explicit executor
    /// configuration.
    ///
    /// # Errors
    ///
    /// Returns any honest executor binding failure.
    pub fn from_hosted_fibers_with_executor_config(
        fibers: HostedFiberRuntime,
        executor_config: ExecutorConfig,
    ) -> Result<Self, ExecutorError> {
        let executor = Executor::new(executor_config.with_mode(ExecutorMode::GreenPool))
            .on_hosted_fibers(&fibers)?;
        Ok(Self { executor, fibers })
    }

    /// Builds one fixed-capacity hosted fiber async runtime.
    ///
    /// # Errors
    ///
    /// Returns any honest hosted-fiber bootstrap or executor binding failure.
    pub fn fixed(total_fibers: usize) -> Result<Self, ExecutorError> {
        Self::fixed_with_executor_config(total_fibers, ExecutorConfig::green_pool())
    }

    /// Builds one fixed-capacity hosted fiber async runtime with one explicit executor
    /// configuration.
    ///
    /// # Errors
    ///
    /// Returns any honest hosted-fiber bootstrap or executor binding failure.
    pub fn fixed_with_executor_config(
        total_fibers: usize,
        executor_config: ExecutorConfig,
    ) -> Result<Self, ExecutorError> {
        let stack_size = hosted_green_executor_stack_size().map_err(executor_error_from_fiber)?;
        let fibers = HostedFiberRuntime::fixed_with_stack(stack_size, total_fibers)
            .map_err(executor_error_from_fiber)?;
        Self::from_hosted_fibers_with_executor_config(fibers, executor_config)
    }

    /// Returns the owned hosted fiber runtime.
    #[must_use]
    pub const fn fibers(&self) -> &HostedFiberRuntime {
        &self.fibers
    }

    /// Returns the underlying executor.
    #[must_use]
    pub const fn executor(&self) -> &Executor {
        &self.executor
    }

    /// Returns the consumer-facing async task lifecycle insight lane for this fiber-backed
    /// runtime.
    #[must_use]
    pub fn task_lifecycle_insight(&self) -> AsyncTaskLifecycleInsight<'_> {
        self.executor.task_lifecycle_insight()
    }

    /// Spawns one ordinary Rust future onto the fiber-backed runtime.
    ///
    /// # Errors
    ///
    /// Returns any honest executor admission or scheduler failure.
    pub fn spawn<F>(&self, future: F) -> Result<TaskHandle<F::Output>, ExecutorError>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.executor.spawn(future)
    }

    /// Spawns one ordinary Rust future with one explicit poll-stack contract.
    ///
    /// # Errors
    ///
    /// Returns any honest executor admission or scheduler failure.
    pub fn spawn_with_poll_stack_bytes<F>(
        &self,
        poll_stack_bytes: usize,
        future: F,
    ) -> Result<TaskHandle<F::Output>, ExecutorError>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.executor
            .spawn_with_poll_stack_bytes(poll_stack_bytes, future)
    }

    /// Spawns one ordinary Rust future using one compile-time generated async poll-stack
    /// contract.
    ///
    /// # Errors
    ///
    /// Returns any honest executor admission or scheduler failure.
    pub fn spawn_generated<F>(&self, future: F) -> Result<TaskHandle<F::Output>, ExecutorError>
    where
        F: Future + Send + 'static + GeneratedExplicitAsyncPollStackContract,
        F::Output: Send + 'static,
    {
        self.executor.spawn_generated(future)
    }

    /// Releases the owned hosted fiber runtime and executor back to the caller.
    #[must_use]
    pub fn into_parts(self) -> (HostedFiberRuntime, Executor) {
        (self.fibers, self.executor)
    }
}

#[derive(Debug)]
enum ExecutorInner {
    Ready(ControlLease<ExecutorCore>),
    Error(ExecutorError),
}

#[cfg(feature = "std")]
#[derive(Debug)]
struct ExecutorReactorDriverState {
    core: ControlLease<ExecutorCore>,
    thread: SyncMutex<Option<JoinHandle<()>>>,
}

#[cfg(feature = "std")]
impl ExecutorReactorDriverState {
    fn new(core: &ControlLease<ExecutorCore>) -> Result<Arc<Self>, ExecutorError> {
        Ok(Arc::new(Self {
            core: core.try_clone().map_err(executor_error_from_alloc)?,
            thread: SyncMutex::new(None),
        }))
    }

    fn ensure_started(
        &self,
        reactor_state: &ExecutorReactorState,
        ready: &AtomicBool,
    ) -> Result<(), ExecutorError> {
        if ready.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut thread_slot = self.thread.lock().map_err(executor_error_from_sync)?;
        if thread_slot.is_some() {
            ready.store(true, Ordering::Release);
            return Ok(());
        }
        reactor_state.install_driver_wake_signal()?;
        let core = self.core.try_clone().map_err(executor_error_from_alloc)?;
        let thread = StdThreadBuilder::new()
            .name(String::from("fusion-async-reactor"))
            .spawn(move || run_reactor_driver(core))
            .map_err(executor_error_from_std_thread)?;
        *thread_slot = Some(thread);
        ready.store(true, Ordering::Release);
        Ok(())
    }

    fn join(&self) {
        let mut thread_slot = match self.thread.lock().map_err(executor_error_from_sync) {
            Ok(thread_slot) => thread_slot,
            Err(_) => return,
        };
        if let Some(thread) = thread_slot.take() {
            let _ = thread.join();
        }
    }
}

impl Executor {
    fn new_fast_current() -> Self {
        Self::with_scheduler(ExecutorConfig::new(), SchedulerBinding::Current, true)
    }

    fn with_runtime_backing(
        config: ExecutorConfig,
        scheduler: SchedulerBinding,
        fast_current: bool,
        backing: CurrentAsyncRuntimeBacking,
    ) -> Self {
        let reactor = Reactor::new();
        let CurrentAsyncRuntimeBacking {
            control,
            reactor: reactor_resource,
            registry,
            spill,
            slab_owner,
        } = backing;
        let inner =
            match ExecutorBackingAllocators::from_current_backing(CurrentAsyncRuntimeBacking {
                control,
                reactor: reactor_resource,
                registry,
                spill,
                slab_owner: None,
            })
            .and_then(|mut allocators| {
                let (reactor_state, current_queue) =
                    ExecutorReactorState::new(config.capacity, fast_current, &allocators.reactor)?;
                let registry =
                    ExecutorRegistry::new(config.capacity, fast_current, &mut allocators);
                let core = allocators.control.control(ExecutorCore {
                    courier_id: config.courier_id,
                    context_id: config.context_id,
                    runtime_sink: config.runtime_sink,
                    reactor,
                    reactor_max_events: config.reactor.max_events,
                    current_queue,
                    reactor_state,
                    reactor_driver_ready: AtomicBool::new(false),
                    #[cfg(feature = "std")]
                    reactor_driver: ExecutorCell::new(fast_current, None),
                    scheduler,
                    next_id: AtomicUsize::new(1),
                    registry,
                    #[cfg(feature = "debug-insights")]
                    task_lifecycle: ExecutorCell::new(fast_current, None),
                    shutdown_requested: AtomicBool::new(false),
                    external_inflight: AtomicUsize::new(0),
                    _owned_backing: slab_owner,
                })?;
                Ok(core)
            }) {
                Ok(core) => ExecutorInner::Ready(core),
                Err(error) => ExecutorInner::Error(error),
            };
        #[cfg(feature = "std")]
        if let ExecutorInner::Ready(core) = &inner
            && matches!(core.scheduler, SchedulerBinding::ThreadWorkers(_))
            && let Ok(driver) = ExecutorReactorDriverState::new(core)
        {
            let _ = core
                .reactor_driver
                .with(|reactor_driver| *reactor_driver = Some(driver));
        }
        Self {
            config,
            reactor,
            inner,
        }
    }

    fn with_current_backing(
        config: ExecutorConfig,
        fast_current: bool,
        backing: CurrentAsyncRuntimeBacking,
    ) -> Self {
        Self::with_runtime_backing(config, SchedulerBinding::Current, fast_current, backing)
    }

    fn with_scheduler(
        config: ExecutorConfig,
        scheduler: SchedulerBinding,
        fast_current: bool,
    ) -> Self {
        if let Ok(backing) = current_async_runtime_virtual_backing(config) {
            return Self::with_runtime_backing(config, scheduler, fast_current, backing);
        }
        let reactor = Reactor::new();
        let inner = match ControlLease::<ExecutorCore>::extent_request()
            .map_err(executor_error_from_alloc)
            .and_then(ExecutorBackingRequest::from_extent_request)
            .and_then(|request| apply_executor_sizing_strategy(request, config.sizing))
            .and_then(|request| {
                Allocator::<1, 1>::system_default_with_capacity(request.bytes)
                    .map_err(executor_error_from_alloc)
            })
            .and_then(|allocator| {
                let default_domain = allocator.default_domain().ok_or_else(executor_invalid)?;
                let reactor_plan = CurrentAsyncRuntimeBackingPlan::for_config(config)?;
                let reactor_allocator = ExecutorDomainAllocator::acquire_virtual(
                    reactor_plan.reactor,
                    "fusion-executor-fallback-reactor",
                )?;
                let (reactor_state, current_queue) =
                    ExecutorReactorState::new(config.capacity, fast_current, &reactor_allocator)?;
                let mut registry_allocators = ExecutorBackingAllocators::acquire_current(config)?;
                let registry =
                    ExecutorRegistry::new(config.capacity, fast_current, &mut registry_allocators);
                allocator
                    .control(
                        default_domain,
                        ExecutorCore {
                            courier_id: config.courier_id,
                            context_id: config.context_id,
                            runtime_sink: config.runtime_sink,
                            reactor,
                            reactor_max_events: config.reactor.max_events,
                            current_queue,
                            reactor_state,
                            reactor_driver_ready: AtomicBool::new(false),
                            #[cfg(feature = "std")]
                            reactor_driver: ExecutorCell::new(fast_current, None),
                            scheduler,
                            next_id: AtomicUsize::new(1),
                            registry,
                            #[cfg(feature = "debug-insights")]
                            task_lifecycle: ExecutorCell::new(fast_current, None),
                            shutdown_requested: AtomicBool::new(false),
                            external_inflight: AtomicUsize::new(0),
                            _owned_backing: None,
                        },
                    )
                    .map_err(executor_error_from_alloc)
            }) {
            Ok(core) => ExecutorInner::Ready(core),
            Err(error) => ExecutorInner::Error(error),
        };
        #[cfg(feature = "std")]
        if let ExecutorInner::Ready(core) = &inner
            && matches!(core.scheduler, SchedulerBinding::ThreadWorkers(_))
            && let Ok(driver) = ExecutorReactorDriverState::new(core)
        {
            let _ = core
                .reactor_driver
                .with(|reactor_driver| *reactor_driver = Some(driver));
        }
        Self {
            config,
            reactor,
            inner,
        }
    }

    fn core(&self) -> Result<&ExecutorCore, ExecutorError> {
        match &self.inner {
            ExecutorInner::Ready(core) => Ok(core),
            ExecutorInner::Error(error) => Err(*error),
        }
    }

    const fn core_lease(&self) -> Result<&ControlLease<ExecutorCore>, ExecutorError> {
        match &self.inner {
            ExecutorInner::Ready(core) => Ok(core),
            ExecutorInner::Error(error) => Err(*error),
        }
    }

    /// Creates a new executor surface.
    #[must_use]
    pub fn new(config: ExecutorConfig) -> Self {
        let scheduler = match config.mode {
            ExecutorMode::CurrentThread => SchedulerBinding::Current,
            ExecutorMode::ThreadPool | ExecutorMode::GreenPool | ExecutorMode::Hybrid => {
                SchedulerBinding::Unsupported
            }
        };
        Self::with_scheduler(config, scheduler, false)
    }

    /// Returns the configured executor mode.
    #[must_use]
    pub const fn mode(&self) -> ExecutorMode {
        self.config.mode
    }

    fn available_task_slots(&self) -> Result<usize, ExecutorError> {
        self.core()?.registry()?.available_slots()
    }

    fn unfinished_task_count(&self) -> Result<usize, ExecutorError> {
        self.core()?.registry()?.unfinished_task_count()
    }

    /// Returns a courier-facing run summary for this executor-owned async lane.
    ///
    /// # Errors
    ///
    /// Returns an error if the executor registry cannot be observed honestly.
    pub fn runtime_summary(&self) -> Result<CourierRuntimeSummary, ExecutorError> {
        self.runtime_summary_with_responsiveness(CourierResponsiveness::Responsive)
    }

    /// Returns a courier-facing run summary for this executor-owned async lane using one
    /// caller-supplied responsiveness classification.
    ///
    /// # Errors
    ///
    /// Returns an error if the executor registry cannot be observed honestly.
    pub fn runtime_summary_with_responsiveness(
        &self,
        responsiveness: CourierResponsiveness,
    ) -> Result<CourierRuntimeSummary, ExecutorError> {
        let registry = self.core()?.registry()?;
        let active_units = registry.unfinished_task_count()?;
        let runnable_units = registry.scheduled_task_count();
        let running_units = registry.running_task_count();
        let blocked_units =
            active_units.saturating_sub(runnable_units.saturating_add(running_units));
        let available_slots = registry.available_slots()?;
        let policy = match self.config.mode {
            ExecutorMode::CurrentThread | ExecutorMode::GreenPool | ExecutorMode::Hybrid => {
                CourierSchedulingPolicy::CooperativePriority
            }
            ExecutorMode::ThreadPool => CourierSchedulingPolicy::CooperativeRoundRobin,
        };
        Ok(CourierRuntimeSummary {
            policy,
            run_state: if running_units != 0 {
                CourierRunState::Running
            } else if runnable_units != 0 {
                CourierRunState::Runnable
            } else {
                CourierRunState::Idle
            },
            responsiveness,
            fiber_lane: None,
            async_lane: Some(CourierLaneSummary {
                kind: RunnableUnitKind::AsyncTask,
                active_units,
                runnable_units,
                running_units,
                blocked_units,
                available_slots,
            }),
            control_lane: None,
        }
        .with_responsiveness(responsiveness))
    }

    /// Returns the public reactor wrapper.
    #[must_use]
    pub const fn reactor(&self) -> &Reactor {
        &self.reactor
    }

    /// Returns the consumer-facing async task lifecycle insight lane for this executor.
    #[must_use]
    pub fn task_lifecycle_insight(&self) -> AsyncTaskLifecycleInsight<'_> {
        AsyncTaskLifecycleInsight {
            core: self.core().ok(),
        }
    }

    /// Spawns a `Send` future onto the executor.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` when the executor has not been bound to a concrete scheduler
    /// for the selected mode, when the future has no honest generated or explicit poll-stack
    /// contract, or `Stopped` when the bound scheduler has shut down.
    pub fn spawn<F>(&self, future: F) -> Result<TaskHandle<F::Output>, ExecutorError>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.spawn_with_admission(AsyncTaskAdmission::for_future::<F>(self.mode()), future)
    }

    /// Spawns a `Send` future with one explicit poll-stack contract.
    ///
    /// # Errors
    ///
    /// Returns any honest executor admission or scheduler failure.
    pub fn spawn_with_poll_stack_bytes<F>(
        &self,
        poll_stack_bytes: usize,
        future: F,
    ) -> Result<TaskHandle<F::Output>, ExecutorError>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let admission = AsyncTaskAdmission::for_future::<F>(self.mode())
            .with_poll_stack_bytes(poll_stack_bytes);
        self.spawn_with_admission(admission, future)
    }

    /// Spawns a `Send` future using one compile-time generated async poll-stack contract.
    ///
    /// # Errors
    ///
    /// Returns any honest executor admission or scheduler failure.
    pub fn spawn_generated<F>(&self, future: F) -> Result<TaskHandle<F::Output>, ExecutorError>
    where
        F: Future + Send + 'static + GeneratedExplicitAsyncPollStackContract,
        F::Output: Send + 'static,
    {
        self.spawn_with_poll_stack_bytes(generated_explicit_async_poll_stack_bytes::<F>(), future)
    }

    fn spawn_with_admission<F>(
        &self,
        admission: AsyncTaskAdmission,
        future: F,
    ) -> Result<TaskHandle<F::Output>, ExecutorError>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        if matches!(admission.poll_stack, AsyncPollStackContract::Unknown) {
            return Err(ExecutorError::Unsupported);
        }
        let core = self.core()?;
        let handle_core = self
            .core_lease()?
            .try_clone()
            .map_err(executor_error_from_alloc)?;
        let id = core.allocate_task_id()?;
        let registry = core.registry()?;
        let (slot_index, generation) = registry.allocate_slot()?;
        let slot = registry.slot(slot_index)?;
        if let Err(error) = slot.bind_core(self.core_lease()?, generation) {
            slot.mark_handle_released(generation)?;
            slot.reset_empty(&registry.spill_store, generation)?;
            registry.release_slot(slot_index, generation)?;
            return Err(error);
        }
        #[cfg(feature = "debug-insights")]
        slot.set_task_id(id)?;

        if let Err(error) = slot.store_future(&registry.spill_store, future) {
            slot.mark_handle_released(generation)?;
            slot.reset_empty(&registry.spill_store, generation)?;
            registry.release_slot(slot_index, generation)?;
            return Err(error);
        }

        if let Err(error) = core.schedule_slot(slot_index, generation) {
            slot.mark_handle_released(generation)?;
            let _ = core.recycle_slot_if_possible(slot_index, generation);
            return Err(error);
        }
        #[cfg(feature = "debug-insights")]
        core.emit_task_lifecycle(AsyncTaskLifecycleRecord::Spawned {
            task: id,
            slot_index,
            generation,
            scheduler: core.scheduler_tag(),
            admission,
        });
        core.publish_runtime_context()?;
        core.publish_runtime_summary()?;

        Ok(TaskHandle {
            inner: TaskHandleInner {
                id,
                admission,
                core: handle_core,
                slot_index,
                generation,
                active: true,
                _marker: PhantomData,
            },
        })
    }

    /// Spawns a non-`Send` future local to the current execution domain.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` when this executor is not current-thread driven, or when the future
    /// has no honest generated or explicit poll-stack contract.
    pub fn spawn_local<F>(&self, future: F) -> Result<LocalTaskHandle<F::Output>, ExecutorError>
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        self.spawn_local_with_admission(AsyncTaskAdmission::for_future::<F>(self.mode()), future)
    }

    /// Spawns a non-`Send` future local to the current execution domain with one explicit
    /// poll-stack contract.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` when this executor is not current-thread driven.
    pub fn spawn_local_with_poll_stack_bytes<F>(
        &self,
        poll_stack_bytes: usize,
        future: F,
    ) -> Result<LocalTaskHandle<F::Output>, ExecutorError>
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        let admission = AsyncTaskAdmission::for_future::<F>(self.mode())
            .with_poll_stack_bytes(poll_stack_bytes);
        self.spawn_local_with_admission(admission, future)
    }

    /// Spawns a non-`Send` future local to the current execution domain using one compile-time
    /// generated async poll-stack contract.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` when this executor is not current-thread driven.
    pub fn spawn_local_generated<F>(
        &self,
        future: F,
    ) -> Result<LocalTaskHandle<F::Output>, ExecutorError>
    where
        F: Future + 'static + GeneratedExplicitAsyncPollStackContract,
        F::Output: 'static,
    {
        self.spawn_local_with_poll_stack_bytes(
            generated_explicit_async_poll_stack_bytes::<F>(),
            future,
        )
    }

    fn spawn_local_with_admission<F>(
        &self,
        admission: AsyncTaskAdmission,
        future: F,
    ) -> Result<LocalTaskHandle<F::Output>, ExecutorError>
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        if matches!(admission.poll_stack, AsyncPollStackContract::Unknown) {
            return Err(ExecutorError::Unsupported);
        }
        let core = self.core()?;
        let SchedulerBinding::Current = &core.scheduler else {
            return Err(ExecutorError::Unsupported);
        };
        let handle_core = self
            .core_lease()?
            .try_clone()
            .map_err(executor_error_from_alloc)?;
        let id = core.allocate_task_id()?;
        let registry = core.registry()?;
        let (slot_index, generation) = registry.allocate_slot()?;
        let slot = registry.slot(slot_index)?;
        if let Err(error) = slot.bind_core(self.core_lease()?, generation) {
            slot.mark_handle_released(generation)?;
            slot.reset_empty(&registry.spill_store, generation)?;
            registry.release_slot(slot_index, generation)?;
            return Err(error);
        }
        #[cfg(feature = "debug-insights")]
        slot.set_task_id(id)?;

        if let Err(error) = slot.store_future(&registry.spill_store, future) {
            slot.mark_handle_released(generation)?;
            slot.reset_empty(&registry.spill_store, generation)?;
            registry.release_slot(slot_index, generation)?;
            return Err(error);
        }

        if let Err(error) = core.schedule_slot(slot_index, generation) {
            slot.mark_handle_released(generation)?;
            let _ = core.recycle_slot_if_possible(slot_index, generation);
            return Err(error);
        }
        #[cfg(feature = "debug-insights")]
        core.emit_task_lifecycle(AsyncTaskLifecycleRecord::Spawned {
            task: id,
            slot_index,
            generation,
            scheduler: core.scheduler_tag(),
            admission,
        });

        Ok(LocalTaskHandle {
            inner: TaskHandleInner {
                id,
                admission,
                core: handle_core,
                slot_index,
                generation,
                active: true,
                _marker: PhantomData,
            },
            _not_send_sync: PhantomData,
        })
    }

    /// Drives one future to completion on the current-thread executor.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` when this executor is not in current-thread mode, or when the future
    /// has no honest generated or explicit poll-stack contract.
    pub fn block_on<F>(&self, future: F) -> Result<F::Output, ExecutorError>
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        let core = self.core()?;
        let SchedulerBinding::Current = &core.scheduler else {
            return Err(ExecutorError::Unsupported);
        };

        let handle = self.spawn_local(future)?;
        while !handle.is_finished()? {
            if !self.drive_once()?
                && !core.drive_reactor_once(true)?
                && system_thread().yield_now().is_err()
            {
                spin_loop();
            }
        }
        handle.join()
    }

    /// Drives one future to completion on the current-thread executor with one explicit
    /// poll-stack contract.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` when this executor is not in current-thread mode.
    pub fn block_on_with_poll_stack_bytes<F>(
        &self,
        poll_stack_bytes: usize,
        future: F,
    ) -> Result<F::Output, ExecutorError>
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        let core = self.core()?;
        let SchedulerBinding::Current = &core.scheduler else {
            return Err(ExecutorError::Unsupported);
        };

        let handle = self.spawn_local_with_poll_stack_bytes(poll_stack_bytes, future)?;
        while !handle.is_finished()? {
            if !self.drive_once()?
                && !core.drive_reactor_once(true)?
                && system_thread().yield_now().is_err()
            {
                spin_loop();
            }
        }
        handle.join()
    }

    /// Drives one ready task on the current-thread executor.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` when this executor is not current-thread driven.
    pub fn drive_once(&self) -> Result<bool, ExecutorError> {
        let core = self.core()?;
        let SchedulerBinding::Current = &core.scheduler else {
            return Err(ExecutorError::Unsupported);
        };
        if core.drive_current_once()? {
            return Ok(true);
        }
        if core.drive_reactor_once(false)? {
            return Ok(true);
        }
        core.drive_current_once()
    }

    /// Drains the current-thread ready queue until no task remains runnable.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` when this executor is not current-thread driven.
    pub fn run_until_idle(&self) -> Result<usize, ExecutorError> {
        let mut ran = 0_usize;
        while self.drive_once()? {
            ran = ran.saturating_add(1);
        }
        Ok(ran)
    }

    /// Attaches the executor to a carrier thread pool.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` when the current executor mode is not pool-backed.
    pub fn on_pool(self, pool: &ThreadPool) -> Result<Self, ExecutorError> {
        if !matches!(self.config.mode, ExecutorMode::ThreadPool) {
            return Err(ExecutorError::Unsupported);
        }

        let executor = Self::with_scheduler(
            self.config,
            {
                #[cfg(feature = "std")]
                {
                    SchedulerBinding::ThreadWorkers(HostedThreadScheduler::new(pool)?)
                }
                #[cfg(not(feature = "std"))]
                {
                    SchedulerBinding::ThreadPool(
                        pool.try_clone().map_err(executor_error_from_thread_pool)?,
                    )
                }
            },
            false,
        );
        let _ = executor.core()?;
        Ok(executor)
    }

    /// Attaches the executor to a green-thread pool.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` when the current executor mode is not green-backed.
    pub fn on_green(self, green: &GreenPool) -> Result<Self, ExecutorError> {
        if !matches!(self.config.mode, ExecutorMode::GreenPool) {
            return Err(ExecutorError::Unsupported);
        }
        green
            .validate_task_attributes(
                green_executor_dispatch_task_attributes().map_err(executor_error_from_fiber)?,
            )
            .map_err(executor_error_from_fiber)?;

        let executor = Self::with_scheduler(
            self.config,
            SchedulerBinding::GreenPool(green.try_clone().map_err(executor_error_from_fiber)?),
            false,
        );
        let _ = executor.core()?;
        Ok(executor)
    }

    /// Attaches the executor to one hosted fiber runtime.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` when the current executor mode is not green-backed.
    #[cfg(feature = "std")]
    pub fn on_hosted_fibers(self, runtime: &HostedFiberRuntime) -> Result<Self, ExecutorError> {
        self.on_green(runtime.fibers())
    }
}

impl Drop for Executor {
    fn drop(&mut self) {
        if let ExecutorInner::Ready(core) = &self.inner {
            core.shutdown();
        }
    }
}

unsafe fn run_current_slot(core: usize, slot_index: usize, generation: u64) {
    let core = unsafe { &*(core as *const ExecutorCore) };
    let _ = core.run_slot_by_ref(slot_index, generation);
}

#[cfg(feature = "std")]
unsafe fn hosted_thread_scheduler_entry(context: *mut ()) -> ThreadEntryReturn {
    let scheduler = unsafe { Arc::from_raw(context.cast::<HostedThreadScheduler>()) };
    run_hosted_thread_scheduler(&scheduler);
    ThreadEntryReturn::new(0)
}

#[cfg(feature = "std")]
fn run_hosted_thread_scheduler(queue: &Arc<HostedThreadScheduler>) {
    loop {
        if queue
            .signal
            .acquire()
            .map_err(executor_error_from_sync)
            .is_err()
        {
            return;
        }

        let job = match queue.ready.lock().map_err(executor_error_from_sync) {
            Ok(mut ready) => ready.dequeue(),
            Err(_) => return,
        };
        match job {
            Some(job) => unsafe {
                (job.run)(job.core, job.slot_index, job.generation);
                (&*(job.core as *const ExecutorCore)).finish_external_schedule();
            },
            None if queue.shutdown.load(Ordering::Acquire) => return,
            None => {}
        }
    }
}

#[cfg(feature = "std")]
fn run_reactor_driver(core: ControlLease<ExecutorCore>) {
    loop {
        if core.shutdown_requested.load(Ordering::Acquire) {
            return;
        }
        if core.drive_reactor_once(true).is_err() {
            return;
        }
    }
}

fn run_scheduled_slot_ptr(core: ScheduledExecutorCorePtr, slot_index: usize, generation: u64) {
    core.run_slot(slot_index, generation);
    // SAFETY: executor shutdown now waits until externally scheduled jobs have drained.
    unsafe { core.0.as_ref().finish_external_schedule() };
}

#[cfg(not(feature = "std"))]
fn run_scheduled_slot_lease(core: ControlLease<ExecutorCore>, slot_index: usize, generation: u64) {
    let _ = core.run_slot_by_ref(slot_index, generation);
    core.finish_external_schedule();
}

fn run_scheduled_green_slot_lease(
    core: ControlLease<ExecutorCore>,
    slot_index: usize,
    generation: u64,
) {
    loop {
        match core.run_slot_by_ref(slot_index, generation) {
            AsyncSlotRunDisposition::Terminal | AsyncSlotRunDisposition::Pending => break,
            AsyncSlotRunDisposition::PendingRequeue => {
                if green_yield_now().is_err() {
                    if let Ok(registry) = core.registry()
                        && let Ok(slot) = registry.slot(slot_index)
                    {
                        let _ =
                            slot.fail(&registry.spill_store, generation, ExecutorError::Stopped);
                        let _ = core.recycle_slot_if_possible(slot_index, generation);
                    }
                    break;
                }
            }
        }
    }
    core.finish_external_schedule();
}

#[cfg(feature = "std")]
fn hosted_green_executor_stack_size() -> Result<NonZeroUsize, FiberError> {
    green_executor_dispatch_stack_size()
}

fn green_executor_dispatch_task_attributes() -> Result<FiberTaskAttributes, FiberError> {
    Ok(GreenExecutorDispatchTask::ATTRIBUTES)
}

#[cfg(feature = "std")]
fn green_executor_dispatch_stack_size() -> Result<NonZeroUsize, FiberError> {
    Ok(GreenExecutorDispatchTask::STACK_BYTES)
}

#[cfg(feature = "std")]
fn poll_future_contained<F>(
    future: Pin<&mut F>,
    context: &mut Context<'_>,
) -> Result<Poll<F::Output>, ()>
where
    F: Future + 'static,
    F::Output: 'static,
{
    #[cfg(feature = "std")]
    {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        catch_unwind(AssertUnwindSafe(|| {
            generated_async_poll_stack_root(future, context)
        }))
        .map_err(|_| ())
    }
}

#[cfg(not(feature = "std"))]
fn poll_future_contained<F>(future: Pin<&mut F>, context: &mut Context<'_>) -> Poll<F::Output>
where
    F: Future + 'static,
    F::Output: 'static,
{
    generated_async_poll_stack_root(future, context)
}

const fn executor_error_from_sync(error: SyncError) -> ExecutorError {
    ExecutorError::Sync(error.kind)
}

const fn executor_error_from_alloc(error: AllocError) -> ExecutorError {
    match error.kind {
        AllocErrorKind::Unsupported | AllocErrorKind::PolicyDenied => ExecutorError::Unsupported,
        AllocErrorKind::Busy => ExecutorError::Sync(SyncErrorKind::Busy),
        AllocErrorKind::CapacityExhausted
        | AllocErrorKind::MetadataExhausted
        | AllocErrorKind::OutOfMemory => ExecutorError::Sync(SyncErrorKind::Overflow),
        AllocErrorKind::SynchronizationFailure(kind) => ExecutorError::Sync(kind),
        AllocErrorKind::InvalidRequest
        | AllocErrorKind::InvalidDomain
        | AllocErrorKind::ResourceFailure(_)
        | AllocErrorKind::PoolFailure(_) => ExecutorError::Sync(SyncErrorKind::Invalid),
    }
}

const fn executor_error_from_runtime_sink(
    error: fusion_sys::courier::CourierRuntimeSinkError,
) -> ExecutorError {
    match error {
        fusion_sys::courier::CourierRuntimeSinkError::Unsupported => ExecutorError::Unsupported,
        fusion_sys::courier::CourierRuntimeSinkError::Invalid => {
            ExecutorError::Sync(SyncErrorKind::Invalid)
        }
        fusion_sys::courier::CourierRuntimeSinkError::NotFound
        | fusion_sys::courier::CourierRuntimeSinkError::StateConflict
        | fusion_sys::courier::CourierRuntimeSinkError::Busy => {
            ExecutorError::Sync(SyncErrorKind::Busy)
        }
        fusion_sys::courier::CourierRuntimeSinkError::ResourceExhausted => {
            ExecutorError::Sync(SyncErrorKind::Overflow)
        }
    }
}

const fn executor_error_from_resource(error: ResourceError) -> ExecutorError {
    match error.kind {
        ResourceErrorKind::UnsupportedRequest | ResourceErrorKind::UnsupportedOperation => {
            ExecutorError::Unsupported
        }
        ResourceErrorKind::OutOfMemory => ExecutorError::Sync(SyncErrorKind::Overflow),
        ResourceErrorKind::SynchronizationFailure(kind) => ExecutorError::Sync(kind),
        ResourceErrorKind::InvalidRequest
        | ResourceErrorKind::ContractViolation
        | ResourceErrorKind::InvalidRange
        | ResourceErrorKind::Platform(_) => ExecutorError::Sync(SyncErrorKind::Invalid),
    }
}

const fn executor_error_from_event(error: EventError) -> ExecutorError {
    match error.kind() {
        EventErrorKind::Unsupported => ExecutorError::Unsupported,
        EventErrorKind::Busy | EventErrorKind::Timeout => ExecutorError::Sync(SyncErrorKind::Busy),
        EventErrorKind::Invalid | EventErrorKind::StateConflict | EventErrorKind::Platform(_) => {
            ExecutorError::Sync(SyncErrorKind::Invalid)
        }
        EventErrorKind::ResourceExhausted => ExecutorError::Sync(SyncErrorKind::Overflow),
    }
}

const fn executor_error_from_thread_pool(error: super::ThreadPoolError) -> ExecutorError {
    match error.kind() {
        fusion_sys::thread::ThreadErrorKind::Unsupported => ExecutorError::Unsupported,
        fusion_sys::thread::ThreadErrorKind::ResourceExhausted => {
            ExecutorError::Sync(SyncErrorKind::Overflow)
        }
        fusion_sys::thread::ThreadErrorKind::Busy
        | fusion_sys::thread::ThreadErrorKind::Timeout
        | fusion_sys::thread::ThreadErrorKind::StateConflict => {
            ExecutorError::Sync(SyncErrorKind::Busy)
        }
        fusion_sys::thread::ThreadErrorKind::Invalid
        | fusion_sys::thread::ThreadErrorKind::PermissionDenied
        | fusion_sys::thread::ThreadErrorKind::PlacementDenied
        | fusion_sys::thread::ThreadErrorKind::SchedulerDenied
        | fusion_sys::thread::ThreadErrorKind::StackDenied
        | fusion_sys::thread::ThreadErrorKind::Platform(_) => {
            ExecutorError::Sync(SyncErrorKind::Invalid)
        }
    }
}

const fn executor_error_from_thread(error: fusion_sys::thread::ThreadError) -> ExecutorError {
    match error.kind() {
        fusion_sys::thread::ThreadErrorKind::Unsupported => ExecutorError::Unsupported,
        fusion_sys::thread::ThreadErrorKind::ResourceExhausted => {
            ExecutorError::Sync(SyncErrorKind::Overflow)
        }
        fusion_sys::thread::ThreadErrorKind::Busy
        | fusion_sys::thread::ThreadErrorKind::Timeout
        | fusion_sys::thread::ThreadErrorKind::StateConflict => {
            ExecutorError::Sync(SyncErrorKind::Busy)
        }
        fusion_sys::thread::ThreadErrorKind::Invalid
        | fusion_sys::thread::ThreadErrorKind::PermissionDenied
        | fusion_sys::thread::ThreadErrorKind::PlacementDenied
        | fusion_sys::thread::ThreadErrorKind::SchedulerDenied
        | fusion_sys::thread::ThreadErrorKind::StackDenied
        | fusion_sys::thread::ThreadErrorKind::Platform(_) => {
            ExecutorError::Sync(SyncErrorKind::Invalid)
        }
    }
}

#[cfg(feature = "std")]
const fn executor_error_from_fiber_host(
    error: fusion_pal::sys::fiber::FiberHostError,
) -> ExecutorError {
    match error.kind() {
        fusion_pal::sys::fiber::FiberHostErrorKind::Unsupported => ExecutorError::Unsupported,
        fusion_pal::sys::fiber::FiberHostErrorKind::ResourceExhausted => {
            ExecutorError::Sync(SyncErrorKind::Overflow)
        }
        fusion_pal::sys::fiber::FiberHostErrorKind::StateConflict => {
            ExecutorError::Sync(SyncErrorKind::Busy)
        }
        fusion_pal::sys::fiber::FiberHostErrorKind::Invalid
        | fusion_pal::sys::fiber::FiberHostErrorKind::Platform(_) => {
            ExecutorError::Sync(SyncErrorKind::Invalid)
        }
    }
}
