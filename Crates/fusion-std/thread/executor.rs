//! Domain 3: public async executor and reactor surface.
//!
//! # Example
//!
//! ```rust
//! use fusion_std::thread::{Executor, ExecutorConfig, ExecutorMode, ThreadPool, ThreadPoolConfig};
//!
//! # fn demo() {
//! let pool = match ThreadPool::new(&ThreadPoolConfig::new()) {
//!     Ok(pool) => pool,
//!     Err(_) => return,
//! };
//! let executor = Executor::new(ExecutorConfig {
//!     mode: ExecutorMode::ThreadPool,
//!     ..ExecutorConfig::new()
//! });
//! let executor = match executor.on_pool(&pool) {
//!     Ok(executor) => executor,
//!     Err(_) => return,
//! };
//! let handle = match executor.spawn(async { 5_u8 }) {
//!     Ok(handle) => handle,
//!     Err(_) => return,
//! };
//! assert_eq!(handle.join(), Ok(5));
//! # }
//! # demo();
//! ```

use core::future::Future;
use core::marker::PhantomData;
use core::mem::ManuallyDrop;
use core::pin::Pin;
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use core::time::Duration;

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;

use crate::sync::{Mutex as SyncMutex, Semaphore, SyncError, SyncErrorKind};
use fusion_sys::event::EventSystem;
pub use fusion_sys::event::{
    EventCompletion, EventCompletionOp, EventCompletionOpKind, EventError, EventErrorKind,
    EventInterest, EventKey, EventModel, EventNotification, EventPoller as ReactorPoller,
    EventReadiness, EventRecord, EventSourceHandle, EventSupport,
};

use super::{GreenPool, ThreadPool};

/// Public executor operating mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExecutorMode {
    /// Drive futures cooperatively on the current thread.
    CurrentThread,
    /// Drive futures on a carrier thread pool.
    ThreadPool,
    /// Drive futures on a green-thread pool.
    GreenPool,
    /// Drive futures across a hybrid thread-pool and green-thread arrangement.
    Hybrid,
}

/// Public reactor configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReactorConfig {
    /// Maximum events pulled from the reactor in one poll, when bounded manually.
    pub max_events: Option<usize>,
}

impl ReactorConfig {
    /// Returns the default reactor configuration.
    #[must_use]
    pub const fn new() -> Self {
        Self { max_events: None }
    }
}

impl Default for ReactorConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Public executor configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExecutorConfig {
    /// Selected execution mode.
    pub mode: ExecutorMode,
    /// Reactor configuration for I/O readiness or completion integration.
    pub reactor: ReactorConfig,
}

impl ExecutorConfig {
    /// Returns a current-thread executor configuration.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            mode: ExecutorMode::CurrentThread,
            reactor: ReactorConfig::new(),
        }
    }
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Public executor error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExecutorError {
    /// The requested executor mode is unsupported or not implemented for this binding.
    Unsupported,
    /// The underlying scheduler has stopped accepting work.
    Stopped,
    /// Internal scheduler coordination failed.
    Sync(SyncErrorKind),
    /// The spawned future panicked while running.
    TaskPanicked,
}

/// Public reactor wrapper.
#[derive(Debug, Clone, Copy)]
pub struct Reactor {
    inner: EventSystem,
}

impl Reactor {
    /// Creates a reactor wrapper for the selected backend event source.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: EventSystem::new(),
        }
    }

    /// Reports the truthful backend event support surface.
    #[must_use]
    pub fn support(&self) -> EventSupport {
        self.inner.support()
    }

    /// Creates a backend poller for this reactor.
    ///
    /// # Errors
    ///
    /// Returns any honest backend failure, including unsupported event polling.
    pub fn create(&self) -> Result<ReactorPoller, EventError> {
        self.inner.create()
    }

    /// Registers a source with the underlying backend reactor.
    ///
    /// # Errors
    ///
    /// Returns any honest backend registration failure.
    pub fn register(
        &self,
        poller: &mut ReactorPoller,
        source: EventSourceHandle,
        interest: EventInterest,
    ) -> Result<EventKey, EventError> {
        self.inner.register(poller, source, interest)
    }

    /// Updates an existing source registration.
    ///
    /// # Errors
    ///
    /// Returns any honest backend re-registration failure.
    pub fn reregister(
        &self,
        poller: &mut ReactorPoller,
        key: EventKey,
        interest: EventInterest,
    ) -> Result<(), EventError> {
        self.inner.reregister(poller, key, interest)
    }

    /// Removes a source registration from the backend reactor.
    ///
    /// # Errors
    ///
    /// Returns any honest backend deregistration failure.
    pub fn deregister(&self, poller: &mut ReactorPoller, key: EventKey) -> Result<(), EventError> {
        self.inner.deregister(poller, key)
    }

    /// Submits a completion-style operation when the backend supports it.
    ///
    /// # Errors
    ///
    /// Returns any honest backend submission failure.
    pub fn submit(
        &self,
        poller: &mut ReactorPoller,
        operation: EventCompletionOp,
    ) -> Result<EventKey, EventError> {
        self.inner.submit(poller, operation)
    }

    /// Polls the backend for ready or completed events.
    ///
    /// # Errors
    ///
    /// Returns any honest backend polling failure.
    pub fn poll(
        &self,
        poller: &mut ReactorPoller,
        events: &mut [EventRecord],
        timeout: Option<Duration>,
    ) -> Result<usize, EventError> {
        self.inner.poll(poller, events, timeout)
    }
}

impl Default for Reactor {
    fn default() -> Self {
        Self::new()
    }
}

struct CurrentQueue {
    ready: SyncMutex<VecDeque<Box<dyn FnOnce() + Send + 'static>>>,
}

impl CurrentQueue {
    const fn new() -> Self {
        Self {
            ready: SyncMutex::new(VecDeque::new()),
        }
    }

    fn schedule(&self, job: Box<dyn FnOnce() + Send + 'static>) -> Result<(), ExecutorError> {
        self.ready
            .lock()
            .map_err(executor_error_from_sync)?
            .push_back(job);
        Ok(())
    }

    fn run_next(&self) -> Result<bool, ExecutorError> {
        let job = self
            .ready
            .lock()
            .map_err(executor_error_from_sync)?
            .pop_front();
        Ok(job.is_some_and(|job| {
            job();
            true
        }))
    }
}

impl core::fmt::Debug for CurrentQueue {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CurrentQueue").finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
enum SchedulerBinding {
    Current(Arc<CurrentQueue>),
    ThreadPool(ThreadPool),
    GreenPool(GreenPool),
    Unsupported,
}

impl SchedulerBinding {
    fn schedule<F>(&self, task: Arc<TaskCell<F>>) -> Result<(), ExecutorError>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        match self {
            Self::Current(queue) => queue.schedule(Box::new(move || task.run())),
            Self::ThreadPool(pool) => pool
                .submit(move || task.run())
                .map_err(|_| ExecutorError::Stopped),
            Self::GreenPool(pool) => pool
                .spawn(move || task.run())
                .map(|_| ())
                .map_err(|_| ExecutorError::Stopped),
            Self::Unsupported => Err(ExecutorError::Unsupported),
        }
    }
}

#[derive(Debug)]
struct ExecutorInner {
    config: ExecutorConfig,
    reactor: Reactor,
    scheduler: SchedulerBinding,
    next_id: AtomicU64,
}

#[derive(Debug)]
enum TaskState<T> {
    Pending,
    Ready(T),
    Failed(ExecutorError),
}

#[derive(Debug)]
struct TaskShared<T> {
    state: SyncMutex<TaskState<T>>,
    finished: AtomicBool,
    ready: Semaphore,
}

impl<T> TaskShared<T> {
    fn new() -> Result<Self, ExecutorError> {
        Ok(Self {
            state: SyncMutex::new(TaskState::Pending),
            finished: AtomicBool::new(false),
            ready: Semaphore::new(0, 1).map_err(executor_error_from_sync)?,
        })
    }

    fn complete(&self, value: T) -> Result<(), ExecutorError> {
        *self.state.lock().map_err(executor_error_from_sync)? = TaskState::Ready(value);
        self.finished.store(true, Ordering::Release);
        self.ready.release(1).map_err(executor_error_from_sync)
    }

    fn fail(&self, error: ExecutorError) -> Result<(), ExecutorError> {
        *self.state.lock().map_err(executor_error_from_sync)? = TaskState::Failed(error);
        self.finished.store(true, Ordering::Release);
        self.ready.release(1).map_err(executor_error_from_sync)
    }

    fn is_finished(&self) -> Result<bool, ExecutorError> {
        Ok(!matches!(
            *self.state.lock().map_err(executor_error_from_sync)?,
            TaskState::Pending
        ))
    }

    fn join(self: &Arc<Self>) -> Result<T, ExecutorError> {
        if !self.is_finished()? {
            self.ready.acquire().map_err(executor_error_from_sync)?;
        }
        match core::mem::replace(
            &mut *self.state.lock().map_err(executor_error_from_sync)?,
            TaskState::Failed(ExecutorError::Stopped),
        ) {
            TaskState::Ready(value) => Ok(value),
            TaskState::Failed(error) => Err(error),
            TaskState::Pending => Err(ExecutorError::Stopped),
        }
    }
}

struct TaskCell<F>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    future: SyncMutex<Option<Pin<Box<F>>>>,
    shared: Arc<TaskShared<F::Output>>,
    scheduler: SchedulerBinding,
    scheduled: AtomicBool,
}

impl<F> TaskCell<F>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    fn new(future: F, shared: Arc<TaskShared<F::Output>>, scheduler: SchedulerBinding) -> Self {
        Self {
            future: SyncMutex::new(Some(Box::pin(future))),
            shared,
            scheduler,
            scheduled: AtomicBool::new(false),
        }
    }

    fn schedule(self: &Arc<Self>) -> Result<(), ExecutorError> {
        if self.scheduled.swap(true, Ordering::AcqRel) {
            return Ok(());
        }

        if let Err(error) = self.scheduler.schedule(Arc::clone(self)) {
            self.scheduled.store(false, Ordering::Release);
            let _ = self.shared.fail(error);
            if let Ok(mut future) = self.future.lock() {
                *future = None;
            }
            return Err(error);
        }
        Ok(())
    }

    fn run(self: Arc<Self>) {
        self.scheduled.store(false, Ordering::Release);
        match self.shared.is_finished() {
            Ok(true) => return,
            Ok(false) => {}
            Err(error) => {
                let _ = self.shared.fail(error);
                return;
            }
        }

        let waker = task_waker(Arc::clone(&self));
        let mut context = Context::from_waker(&waker);
        let mut future = match self.future.lock().map_err(executor_error_from_sync) {
            Ok(future) => future,
            Err(error) => {
                let _ = self.shared.fail(error);
                return;
            }
        };
        let Some(mut future_slot) = future.take() else {
            return;
        };
        drop(future);

        let poll = catch_unwind(AssertUnwindSafe(|| future_slot.as_mut().poll(&mut context)));
        match poll {
            Ok(Poll::Ready(output)) => {
                let _ = self.shared.complete(output);
            }
            Ok(Poll::Pending) => {
                if let Ok(mut future) = self.future.lock() {
                    *future = Some(future_slot);
                } else {
                    let _ = self.shared.fail(ExecutorError::Stopped);
                }
            }
            Err(_) => {
                let _ = self.shared.fail(ExecutorError::TaskPanicked);
            }
        }
    }
}

unsafe fn clone_task_waker<F>(data: *const ()) -> RawWaker
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let task = ManuallyDrop::new(unsafe { Arc::<TaskCell<F>>::from_raw(data.cast()) });
    let cloned = Arc::clone(&task);
    RawWaker::new(Arc::into_raw(cloned).cast(), task_waker_vtable::<F>())
}

unsafe fn wake_task<F>(data: *const ())
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let task = unsafe { Arc::<TaskCell<F>>::from_raw(data.cast()) };
    let _ = task.schedule();
}

unsafe fn wake_task_by_ref<F>(data: *const ())
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let task = ManuallyDrop::new(unsafe { Arc::<TaskCell<F>>::from_raw(data.cast()) });
    let _ = Arc::clone(&task).schedule();
}

unsafe fn drop_task_waker<F>(data: *const ())
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    drop(unsafe { Arc::<TaskCell<F>>::from_raw(data.cast()) });
}

fn task_waker_vtable<F>() -> &'static RawWakerVTable
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    &RawWakerVTable::new(
        clone_task_waker::<F>,
        wake_task::<F>,
        wake_task_by_ref::<F>,
        drop_task_waker::<F>,
    )
}

fn task_waker<F>(task: Arc<TaskCell<F>>) -> Waker
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let raw = RawWaker::new(Arc::into_raw(task).cast(), task_waker_vtable::<F>());
    unsafe { Waker::from_raw(raw) }
}

/// Public spawned-task handle.
#[derive(Debug)]
pub struct TaskHandle<T> {
    id: u64,
    shared: Arc<TaskShared<T>>,
}

impl<T> TaskHandle<T> {
    /// Returns the stable task identifier.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    /// Returns whether the task has completed.
    ///
    /// # Errors
    ///
    /// Returns an error if the task state cannot be observed honestly.
    pub fn is_finished(&self) -> Result<bool, ExecutorError> {
        self.shared.is_finished()
    }

    /// Blocks until the task completes and returns its result.
    ///
    /// # Errors
    ///
    /// Returns the scheduler failure that stopped the task, if any.
    pub fn join(self) -> Result<T, ExecutorError> {
        self.shared.join()
    }
}

/// Public set of task handles joined as a group.
#[derive(Debug, Default)]
pub struct JoinSet<T> {
    _marker: PhantomData<T>,
}

/// Public async executor wrapper.
#[derive(Debug, Clone)]
pub struct Executor {
    inner: Arc<ExecutorInner>,
}

impl Executor {
    fn with_scheduler(config: ExecutorConfig, scheduler: SchedulerBinding) -> Self {
        Self {
            inner: Arc::new(ExecutorInner {
                config,
                reactor: Reactor::new(),
                scheduler,
                next_id: AtomicU64::new(1),
            }),
        }
    }

    /// Creates a new executor surface.
    #[must_use]
    pub fn new(config: ExecutorConfig) -> Self {
        let scheduler = match config.mode {
            ExecutorMode::CurrentThread => SchedulerBinding::Current(Arc::new(CurrentQueue::new())),
            ExecutorMode::ThreadPool | ExecutorMode::GreenPool | ExecutorMode::Hybrid => {
                SchedulerBinding::Unsupported
            }
        };
        Self::with_scheduler(config, scheduler)
    }

    /// Returns the configured executor mode.
    #[must_use]
    pub fn mode(&self) -> ExecutorMode {
        self.inner.config.mode
    }

    /// Returns the public reactor wrapper.
    #[must_use]
    pub fn reactor(&self) -> &Reactor {
        &self.inner.reactor
    }

    /// Spawns a `Send` future onto the executor.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` when the executor has not been bound to a concrete scheduler
    /// for the selected mode, or `Stopped` when the bound scheduler has shut down.
    pub fn spawn<F>(&self, future: F) -> Result<TaskHandle<F::Output>, ExecutorError>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let shared = Arc::new(TaskShared::new()?);
        let id = self.inner.next_id.fetch_add(1, Ordering::AcqRel);
        let task = Arc::new(TaskCell::new(
            future,
            Arc::clone(&shared),
            self.inner.scheduler.clone(),
        ));
        task.schedule()?;
        Ok(TaskHandle { id, shared })
    }

    /// Spawns a non-`Send` future local to the current execution domain.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` until the local-task path grows a dedicated local scheduler.
    pub fn spawn_local<F>(&self, _future: F) -> Result<TaskHandle<F::Output>, ExecutorError>
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        Err(ExecutorError::Unsupported)
    }

    /// Drives one future to completion on the current-thread executor.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` when this executor is not in current-thread mode.
    pub fn block_on<F>(&self, future: F) -> Result<F::Output, ExecutorError>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let SchedulerBinding::Current(queue) = &self.inner.scheduler else {
            return Err(ExecutorError::Unsupported);
        };

        let handle = self.spawn(future)?;
        while !handle.is_finished()? {
            if !queue.run_next()? {
                thread::yield_now();
            }
        }
        handle.join()
    }

    /// Attaches the executor to a carrier thread pool.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` when the current executor mode is not pool-backed.
    pub fn on_pool(self, pool: &ThreadPool) -> Result<Self, ExecutorError> {
        if !matches!(self.inner.config.mode, ExecutorMode::ThreadPool) {
            return Err(ExecutorError::Unsupported);
        }

        Ok(Self::with_scheduler(
            self.inner.config,
            SchedulerBinding::ThreadPool(pool.clone()),
        ))
    }

    /// Attaches the executor to a green-thread pool.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` when the current executor mode is not green-backed.
    pub fn on_green(self, green: &GreenPool) -> Result<Self, ExecutorError> {
        if !matches!(self.inner.config.mode, ExecutorMode::GreenPool) {
            return Err(ExecutorError::Unsupported);
        }

        Ok(Self::with_scheduler(
            self.inner.config,
            SchedulerBinding::GreenPool(green.clone()),
        ))
    }
}

const fn executor_error_from_sync(error: SyncError) -> ExecutorError {
    ExecutorError::Sync(error.kind)
}
