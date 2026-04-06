use super::*;
use super::engine::{
    green_executor_dispatch_stack_size,
};
use std::boxed::Box;
use crate::thread::{
    PoolPlacement,
    ResizePolicy,
    ShutdownPolicy,
    ThreadPoolConfig,
};

#[derive(Debug)]
struct HostedThreadWorkerState {
    ready: SyncMutex<HostedReadyQueueState>,
    signal: Semaphore,
    observation: SyncMutex<Option<fusion_sys::thread::CarrierObservation>>,
}

#[derive(Debug)]
struct HostedThreadWorkerEntryContext {
    scheduler: Arc<HostedThreadScheduler>,
    worker_index: usize,
}

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
    pub fn new(config: &ThreadPoolConfig<'_>) -> Result<Self, ExecutorError> {
        Self::with_executor_config(config, ExecutorConfig::thread_pool())
    }

    /// Creates one hosted async runtime with one explicit executor configuration.
    ///
    /// # Errors
    ///
    /// Returns any honest carrier bootstrap or executor binding failure.
    pub fn with_executor_config(
        config: &ThreadPoolConfig<'_>,
        executor_config: ExecutorConfig,
    ) -> Result<Self, ExecutorError> {
        if HostedThreadWorkers::direct_supported(config) {
            let carriers =
                HostedThreadScheduler::new_direct(config, executor_config.spawn_locality_policy)?;
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
pub(super) struct ExecutorReactorDriverState {
    core: ControlLease<ExecutorCore>,
    thread: SyncMutex<Option<JoinHandle<()>>>,
}

impl ExecutorReactorDriverState {
    pub(super) fn new(core: &ControlLease<ExecutorCore>) -> Result<Arc<Self>, ExecutorError> {
        Ok(Arc::new(Self {
            core: core.try_clone().map_err(executor_error_from_alloc)?,
            thread: SyncMutex::new(None),
        }))
    }

    pub(super) fn ensure_started(
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

    pub(super) fn join(&self) {
        let mut thread_slot = match self.thread.lock().map_err(executor_error_from_sync) {
            Ok(thread_slot) => thread_slot,
            Err(_) => return,
        };
        if let Some(thread) = thread_slot.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Debug)]
pub(super) struct HostedThreadScheduler {
    workers: Box<[HostedThreadWorkerState]>,
    shutdown: AtomicBool,
    next_worker: AtomicUsize,
    worker_count: usize,
    spawn_locality_policy: fusion_sys::thread::CarrierSpawnLocalityPolicy,
}

#[derive(Debug)]
pub(super) struct HostedThreadWorkers {
    pub(super) scheduler: Arc<HostedThreadScheduler>,
    handles: Vec<ThreadHandle>,
    system: ThreadSystem,
}

#[derive(Debug)]
pub(super) enum ThreadAsyncCarriers {
    Direct(HostedThreadWorkers),
    ThreadPool(ThreadPool),
}

/// Hosted thread-async runtime bootstrap realization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadAsyncRuntimeBootstrap {
    /// Hosted async workers are born directly as long-lived OS-thread carriers.
    DirectHostedWorkers,
    /// Hosted async workers are composed on top of one generic thread pool.
    ComposedThreadPool,
}

impl HostedThreadScheduler {
    pub(super) fn new(
        pool: &ThreadPool,
        spawn_locality_policy: fusion_sys::thread::CarrierSpawnLocalityPolicy,
    ) -> Result<Arc<Self>, ExecutorError> {
        let worker_count = pool
            .stats()
            .map_err(executor_error_from_thread_pool)?
            .max_threads
            .max(1);
        let scheduler = Arc::new(Self {
            workers: Self::build_worker_states(worker_count)?,
            shutdown: AtomicBool::new(false),
            next_worker: AtomicUsize::new(0),
            worker_count,
            spawn_locality_policy,
        });
        for worker_index in 0..worker_count {
            let worker = Arc::clone(&scheduler);
            pool.submit(move || run_hosted_thread_scheduler(&worker, worker_index))
                .map_err(executor_error_from_thread_pool)?;
        }
        Ok(scheduler)
    }

    fn new_direct(
        config: &ThreadPoolConfig<'_>,
        spawn_locality_policy: fusion_sys::thread::CarrierSpawnLocalityPolicy,
    ) -> Result<HostedThreadWorkers, ExecutorError> {
        let worker_count = config.max_threads.max(1);
        let scheduler = Arc::new(Self {
            workers: Self::build_worker_states(worker_count)?,
            shutdown: AtomicBool::new(false),
            next_worker: AtomicUsize::new(0),
            worker_count,
            spawn_locality_policy,
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

        for worker_index in 0..worker_count {
            let scheduler_context = Box::into_raw(Box::new(HostedThreadWorkerEntryContext {
                scheduler: Arc::clone(&scheduler),
                worker_index,
            }));
            let handle = unsafe {
                system.spawn_raw(
                    &thread_config,
                    hosted_thread_scheduler_entry,
                    scheduler_context.cast::<()>(),
                )
            };
            match handle {
                Ok(handle) => handles.push(handle),
                Err(error) => {
                    unsafe {
                        drop(Box::from_raw(scheduler_context));
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

    pub(super) fn enqueue(&self, job: CurrentJob) -> Result<(), ExecutorError> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(ExecutorError::Stopped);
        }
        let start = self.next_worker.fetch_add(1, Ordering::AcqRel) % self.worker_count.max(1);
        let preferred = self
            .select_worker_for_current_observation(start)
            .unwrap_or(start);
        for offset in 0..self.worker_count {
            let worker_index = (preferred + offset) % self.worker_count;
            let worker = &self.workers[worker_index];
            let mut ready = worker.ready.lock().map_err(executor_error_from_sync)?;
            if ready.enqueue(job).is_ok() {
                drop(ready);
                worker.signal.release(1).map_err(executor_error_from_sync)?;
                return Ok(());
            }
        }
        Err(ExecutorError::Sync(SyncErrorKind::Overflow))
    }

    pub(super) fn request_shutdown(&self) -> Result<usize, ExecutorError> {
        self.shutdown.store(true, Ordering::Release);
        let mut dropped = 0usize;
        for worker in &*self.workers {
            dropped = dropped.saturating_add(
                worker
                    .ready
                    .lock()
                    .map_err(executor_error_from_sync)?
                    .clear(),
            );
            worker.signal.release(1).map_err(executor_error_from_sync)?;
        }
        Ok(dropped)
    }

    fn build_worker_states(
        worker_count: usize,
    ) -> Result<Box<[HostedThreadWorkerState]>, ExecutorError> {
        let mut workers = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            workers.push(HostedThreadWorkerState {
                ready: SyncMutex::new(HostedReadyQueueState::new()),
                signal: Semaphore::new(
                    0,
                    u32::try_from(CURRENT_QUEUE_CAPACITY.saturating_add(worker_count))
                        .unwrap_or(u32::MAX),
                )
                .map_err(executor_error_from_sync)?,
                observation: SyncMutex::new(None),
            });
        }
        Ok(workers.into_boxed_slice())
    }

    fn publish_current_observation(&self, worker_index: usize) {
        let Ok(observation) = fusion_sys::thread::system_carrier().observe_current() else {
            return;
        };
        if let Ok(mut slot) = self.workers[worker_index].observation.lock() {
            *slot = Some(observation);
        }
    }

    fn select_worker_for_current_observation(&self, start: usize) -> Option<usize> {
        let policy = self.spawn_locality_policy;
        if matches!(
            policy,
            fusion_sys::thread::CarrierSpawnLocalityPolicy::Inherit
                | fusion_sys::thread::CarrierSpawnLocalityPolicy::Any
        ) {
            return None;
        }
        let origin = fusion_sys::thread::system_carrier()
            .observe_current()
            .ok()?;
        let mut best: Option<(u8, usize)> = None;
        for offset in 0..self.worker_count {
            let worker_index = (start + offset) % self.worker_count;
            let observed = self.workers[worker_index]
                .observation
                .lock()
                .ok()
                .and_then(|guard| *guard);
            let Some(observed) = observed else {
                continue;
            };
            if observed.thread_id == origin.thread_id {
                return Some(worker_index);
            }
            let Some(rank) = fusion_sys::thread::carrier_spawn_locality_rank(
                policy,
                origin.location,
                observed.location,
            ) else {
                continue;
            };
            if best.is_none_or(|(best_rank, _)| rank < best_rank) {
                best = Some((rank, worker_index));
            }
        }
        best.map(|(_, worker_index)| worker_index)
    }
}

impl HostedThreadWorkers {
    fn direct_supported(config: &ThreadPoolConfig<'_>) -> bool {
        matches!(config.placement, PoolPlacement::Inherit)
            && matches!(config.resize_policy, ResizePolicy::Fixed)
            && matches!(config.shutdown_policy, ShutdownPolicy::Drain)
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

unsafe fn hosted_thread_scheduler_entry(context: *mut ()) -> ThreadEntryReturn {
    let context = unsafe { Box::from_raw(context.cast::<HostedThreadWorkerEntryContext>()) };
    run_hosted_thread_scheduler(&context.scheduler, context.worker_index);
    ThreadEntryReturn::new(0)
}

fn run_hosted_thread_scheduler(queue: &Arc<HostedThreadScheduler>, worker_index: usize) {
    queue.publish_current_observation(worker_index);
    loop {
        if queue.workers[worker_index]
            .signal
            .acquire()
            .map_err(executor_error_from_sync)
            .is_err()
        {
            return;
        }
        queue.publish_current_observation(worker_index);

        let job = match queue.workers[worker_index]
            .ready
            .lock()
            .map_err(executor_error_from_sync)
        {
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

pub(super) fn hosted_green_executor_stack_size() -> Result<NonZeroUsize, FiberError> {
    green_executor_dispatch_stack_size()
}

fn executor_error_from_std_thread(error: std::io::Error) -> ExecutorError {
    if error.kind() == std::io::ErrorKind::OutOfMemory {
        return ExecutorError::Sync(SyncErrorKind::Overflow);
    }
    ExecutorError::Sync(SyncErrorKind::Invalid)
}

pub(super) const fn executor_error_from_fiber_host(
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
