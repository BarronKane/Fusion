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

use core::any::TypeId;
use core::array;
use core::fmt;
use core::future::Future;
use core::hint::spin_loop;
use core::marker::PhantomData;
use core::mem::{MaybeUninit, align_of, size_of};
use core::pin::Pin;
use core::ptr::NonNull;
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use core::time::Duration;

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

use crate::sync::{Mutex as SyncMutex, Semaphore, SyncError, SyncErrorKind};
use fusion_sys::alloc::{
    AllocError,
    AllocErrorKind,
    Allocator,
    ArenaInitError,
    ArenaSlice,
    BoundedArena,
    ControlLease,
};
use fusion_sys::event::EventSystem;
pub use fusion_sys::event::{
    EventCompletion,
    EventCompletionOp,
    EventCompletionOpKind,
    EventError,
    EventErrorKind,
    EventInterest,
    EventKey,
    EventModel,
    EventNotification,
    EventPoller as ReactorPoller,
    EventReadiness,
    EventRecord,
    EventRegistration,
    EventSourceHandle,
    EventSupport,
};
use fusion_sys::fiber::{FiberError, FiberErrorKind};
use fusion_sys::thread::system_thread;

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

/// Stable executor-scoped task identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId {
    executor_scope: usize,
    sequence: u64,
}

impl TaskId {
    const fn new(executor_scope: usize, sequence: u64) -> Self {
        Self {
            executor_scope,
            sequence,
        }
    }

    /// Returns the executor scope that owns this task identifier.
    #[must_use]
    pub const fn executor_scope(self) -> usize {
        self.executor_scope
    }

    /// Returns the per-executor sequence number for this task identifier.
    #[must_use]
    pub const fn sequence(self) -> u64 {
        self.sequence
    }
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

    /// Registers a source with an explicit backend delivery policy.
    ///
    /// # Errors
    ///
    /// Returns any honest backend registration failure.
    pub fn register_with(
        &self,
        poller: &mut ReactorPoller,
        registration: EventRegistration,
    ) -> Result<EventKey, EventError> {
        self.inner.register_with(poller, registration)
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

    /// Updates an existing source registration with an explicit backend delivery policy.
    ///
    /// # Errors
    ///
    /// Returns any honest backend re-registration failure.
    pub fn reregister_with(
        &self,
        poller: &mut ReactorPoller,
        key: EventKey,
        registration: EventRegistration,
    ) -> Result<(), EventError> {
        self.inner.reregister_with(poller, key, registration)
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

const CURRENT_QUEUE_CAPACITY: usize = 256;
const TASK_REGISTRY_CAPACITY: usize = 256;
const JOIN_SET_CAPACITY: usize = 64;
const INLINE_ASYNC_FUTURE_BYTES: usize = 256;
const INLINE_ASYNC_RESULT_BYTES: usize = 256;

const SLOT_EMPTY: u8 = 0;
const SLOT_PENDING: u8 = 1;
const SLOT_READY: u8 = 2;
const SLOT_FAILED: u8 = 3;

const fn executor_invalid() -> ExecutorError {
    ExecutorError::Sync(SyncErrorKind::Invalid)
}

const fn executor_busy() -> ExecutorError {
    ExecutorError::Sync(SyncErrorKind::Busy)
}

const fn executor_overflow() -> ExecutorError {
    ExecutorError::Sync(SyncErrorKind::Overflow)
}

struct CurrentQueue {
    ready: SyncMutex<CurrentQueueState>,
}

#[derive(Debug, Clone, Copy)]
struct CurrentJob {
    run: unsafe fn(usize, usize, u64),
    core: usize,
    slot_index: usize,
    generation: u64,
}

#[derive(Debug)]
struct CurrentQueueState {
    entries: [Option<CurrentJob>; CURRENT_QUEUE_CAPACITY],
    head: usize,
    tail: usize,
    len: usize,
}

impl CurrentQueue {
    const fn new() -> Self {
        Self {
            ready: SyncMutex::new(CurrentQueueState::new()),
        }
    }

    fn schedule_slot(
        &self,
        core: &ExecutorCore,
        slot_index: usize,
        generation: u64,
    ) -> Result<(), ExecutorError> {
        self.ready
            .lock()
            .map_err(executor_error_from_sync)?
            .enqueue(CurrentJob {
                run: run_current_slot,
                core: core::ptr::from_ref(core) as usize,
                slot_index,
                generation,
            })?;
        Ok(())
    }

    fn run_next(&self) -> Result<bool, ExecutorError> {
        let job = self
            .ready
            .lock()
            .map_err(executor_error_from_sync)?
            .dequeue();
        if let Some(job) = job {
            unsafe {
                (job.run)(job.core, job.slot_index, job.generation);
            }
            return Ok(true);
        }
        Ok(false)
    }
}

impl fmt::Debug for CurrentQueue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CurrentQueue").finish_non_exhaustive()
    }
}

impl CurrentQueueState {
    const fn new() -> Self {
        Self {
            entries: [None; CURRENT_QUEUE_CAPACITY],
            head: 0,
            tail: 0,
            len: 0,
        }
    }

    const fn enqueue(&mut self, job: CurrentJob) -> Result<(), ExecutorError> {
        if self.len == self.entries.len() {
            return Err(executor_overflow());
        }
        self.entries[self.tail] = Some(job);
        self.tail = (self.tail + 1) % self.entries.len();
        self.len += 1;
        Ok(())
    }

    const fn dequeue(&mut self) -> Option<CurrentJob> {
        if self.len == 0 {
            return None;
        }
        let job = self.entries[self.head].take();
        self.head = (self.head + 1) % self.entries.len();
        self.len -= 1;
        job
    }
}

#[derive(Debug)]
struct FixedIndexStack {
    entries: ArenaSlice<usize>,
    len: usize,
}

impl FixedIndexStack {
    fn new_in(arena: &BoundedArena, capacity: usize) -> Result<Self, ExecutorError> {
        let entries = arena
            .alloc_array_with(capacity, |index| capacity.saturating_sub(index + 1))
            .map_err(executor_error_from_alloc)?;
        let len = entries.len();
        Ok(Self { entries, len })
    }

    fn pop(&mut self) -> Option<usize> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        Some(self.entries[self.len])
    }

    fn push(&mut self, value: usize) -> Result<(), ExecutorError> {
        if self.len == self.entries.len() {
            return Err(executor_invalid());
        }
        self.entries[self.len] = value;
        self.len += 1;
        Ok(())
    }
}

#[repr(C, align(64))]
struct InlineAsyncFutureBytes {
    bytes: [u8; INLINE_ASYNC_FUTURE_BYTES],
}

type InlineAsyncPollFn = unsafe fn(
    *mut u8,
    &SyncMutex<InlineAsyncResultStorage>,
    &mut Context<'_>,
) -> Result<Poll<()>, ExecutorError>;

struct InlineAsyncFutureStorage {
    storage: MaybeUninit<InlineAsyncFutureBytes>,
    poll: Option<InlineAsyncPollFn>,
    drop: Option<unsafe fn(*mut u8)>,
    occupied: bool,
}

impl fmt::Debug for InlineAsyncFutureStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InlineAsyncFutureStorage")
            .field("occupied", &self.occupied)
            .finish_non_exhaustive()
    }
}

impl InlineAsyncFutureStorage {
    const fn empty() -> Self {
        Self {
            storage: MaybeUninit::uninit(),
            poll: None,
            drop: None,
            occupied: false,
        }
    }

    const fn supports<F>() -> bool
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        size_of::<F>() <= size_of::<InlineAsyncFutureBytes>()
            && align_of::<F>() <= align_of::<InlineAsyncFutureBytes>()
            && InlineAsyncResultStorage::supports::<F::Output>()
    }

    fn store_future<F>(&mut self, future: F) -> Result<(), ExecutorError>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        if self.occupied {
            return Err(executor_invalid());
        }
        if !Self::supports::<F>() {
            return Err(ExecutorError::Unsupported);
        }

        unsafe {
            self.storage.as_mut_ptr().cast::<F>().write(future);
        }
        self.poll = Some(poll_inline_async_future::<F>);
        self.drop = Some(drop_inline_async_value::<F>);
        self.occupied = true;
        Ok(())
    }

    fn poll_in_place(
        &mut self,
        result: &SyncMutex<InlineAsyncResultStorage>,
        context: &mut Context<'_>,
    ) -> Result<Poll<()>, ExecutorError> {
        if !self.occupied {
            return Err(executor_invalid());
        }
        let poll = self.poll.ok_or_else(executor_invalid)?;
        unsafe { poll(self.storage.as_mut_ptr().cast::<u8>(), result, context) }
    }

    fn clear(&mut self) {
        if !self.occupied {
            self.poll = None;
            self.drop = None;
            return;
        }

        if let Some(drop) = self.drop.take() {
            unsafe {
                drop(self.storage.as_mut_ptr().cast::<u8>());
            }
        }
        self.poll = None;
        self.occupied = false;
    }
}

impl Drop for InlineAsyncFutureStorage {
    fn drop(&mut self) {
        self.clear();
    }
}

#[repr(C, align(64))]
struct InlineAsyncResultBytes {
    bytes: [u8; INLINE_ASYNC_RESULT_BYTES],
}

struct InlineAsyncResultStorage {
    storage: MaybeUninit<InlineAsyncResultBytes>,
    drop: Option<unsafe fn(*mut u8)>,
    type_id: Option<TypeId>,
    occupied: bool,
}

impl fmt::Debug for InlineAsyncResultStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InlineAsyncResultStorage")
            .field("occupied", &self.occupied)
            .finish_non_exhaustive()
    }
}

impl InlineAsyncResultStorage {
    const fn empty() -> Self {
        Self {
            storage: MaybeUninit::uninit(),
            drop: None,
            type_id: None,
            occupied: false,
        }
    }

    const fn supports<T: 'static>() -> bool {
        size_of::<T>() <= size_of::<InlineAsyncResultBytes>()
            && align_of::<T>() <= align_of::<InlineAsyncResultBytes>()
    }

    fn store<T: 'static>(&mut self, value: T) -> Result<(), ExecutorError> {
        if self.occupied {
            return Err(executor_invalid());
        }
        if !Self::supports::<T>() {
            return Err(ExecutorError::Unsupported);
        }

        unsafe {
            self.storage.as_mut_ptr().cast::<T>().write(value);
        }
        self.drop = Some(drop_inline_async_value::<T>);
        self.type_id = Some(TypeId::of::<T>());
        self.occupied = true;
        Ok(())
    }

    fn take<T: 'static>(&mut self) -> Result<T, ExecutorError> {
        if !self.occupied || self.type_id != Some(TypeId::of::<T>()) {
            return Err(executor_invalid());
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

impl Drop for InlineAsyncResultStorage {
    fn drop(&mut self) {
        self.clear();
    }
}

unsafe fn poll_inline_async_future<F>(
    ptr: *mut u8,
    result: &SyncMutex<InlineAsyncResultStorage>,
    context: &mut Context<'_>,
) -> Result<Poll<()>, ExecutorError>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    // SAFETY: executor futures live inside arena-backed task slots whose addresses remain stable
    // for the lifetime of the live slot lease; the arena never relocates allocations.
    let future = unsafe { Pin::new_unchecked(&mut *ptr.cast::<F>()) };

    #[cfg(feature = "std")]
    match poll_future_contained(future, context) {
        Ok(Poll::Ready(output)) => {
            result
                .lock()
                .map_err(executor_error_from_sync)?
                .store(output)?;
            Ok(Poll::Ready(()))
        }
        Ok(Poll::Pending) => Ok(Poll::Pending),
        Err(()) => Err(ExecutorError::TaskPanicked),
    }

    #[cfg(not(feature = "std"))]
    match poll_future_contained(future, context) {
        Poll::Ready(output) => {
            result
                .lock()
                .map_err(executor_error_from_sync)?
                .store(output)?;
            Ok(Poll::Ready(()))
        }
        Poll::Pending => Ok(Poll::Pending),
    }
}

unsafe fn drop_inline_async_value<T>(ptr: *mut u8) {
    unsafe {
        ptr.cast::<T>().drop_in_place();
    }
}

#[derive(Debug)]
struct AsyncTaskWakerData {
    core_ptr: AtomicUsize,
    slot_index: usize,
    generation: AtomicUsize,
}

impl AsyncTaskWakerData {
    const fn new(slot_index: usize) -> Self {
        Self {
            core_ptr: AtomicUsize::new(0),
            slot_index,
            generation: AtomicUsize::new(0),
        }
    }

    fn set_core(&self, core: *const ExecutorCore) {
        self.core_ptr.store(core as usize, Ordering::Release);
    }

    fn core_ptr(&self) -> *const ExecutorCore {
        let core_ptr = self.core_ptr.load(Ordering::Acquire);
        if core_ptr == 0 {
            return core::ptr::null();
        }
        core_ptr as *const ExecutorCore
    }

    fn set_generation(&self, generation: u64) {
        self.generation.store(
            usize::try_from(generation).unwrap_or(usize::MAX),
            Ordering::Release,
        );
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire) as u64
    }
}

struct AsyncTaskSlot {
    generation: AtomicUsize,
    core: SyncMutex<Option<ControlLease<ExecutorCore>>>,
    future: SyncMutex<InlineAsyncFutureStorage>,
    result: SyncMutex<InlineAsyncResultStorage>,
    state: AtomicU8,
    error: SyncMutex<Option<ExecutorError>>,
    completed: Semaphore,
    scheduled: AtomicBool,
    handle_live: AtomicBool,
    waker_refs: AtomicUsize,
    waker: AsyncTaskWakerData,
}

impl fmt::Debug for AsyncTaskSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsyncTaskSlot")
            .field("generation", &self.generation.load(Ordering::Acquire))
            .field("state", &self.state.load(Ordering::Acquire))
            .field("scheduled", &self.scheduled.load(Ordering::Acquire))
            .field("handle_live", &self.handle_live.load(Ordering::Acquire))
            .field("waker_refs", &self.waker_refs.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl AsyncTaskSlot {
    fn new(slot_index: usize) -> Result<Self, ExecutorError> {
        Ok(Self {
            generation: AtomicUsize::new(0),
            core: SyncMutex::new(None),
            future: SyncMutex::new(InlineAsyncFutureStorage::empty()),
            result: SyncMutex::new(InlineAsyncResultStorage::empty()),
            state: AtomicU8::new(SLOT_EMPTY),
            error: SyncMutex::new(None),
            completed: Semaphore::new(0, 1).map_err(executor_error_from_sync)?,
            scheduled: AtomicBool::new(false),
            handle_live: AtomicBool::new(false),
            waker_refs: AtomicUsize::new(0),
            waker: AsyncTaskWakerData::new(slot_index),
        })
    }

    fn bind_core(
        &self,
        core: &ControlLease<ExecutorCore>,
        generation: u64,
    ) -> Result<(), ExecutorError> {
        if self.generation() != generation || self.state() == SLOT_EMPTY {
            return Err(ExecutorError::Stopped);
        }
        *self.core.lock().map_err(executor_error_from_sync)? =
            Some(core.try_clone().map_err(executor_error_from_alloc)?);
        self.waker.set_core(core.as_ptr());
        Ok(())
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire) as u64
    }

    fn state(&self) -> u8 {
        self.state.load(Ordering::Acquire)
    }

    fn initialize_for_allocation(&self) -> Result<u64, ExecutorError> {
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

        self.future
            .lock()
            .map_err(executor_error_from_sync)?
            .clear();
        self.result
            .lock()
            .map_err(executor_error_from_sync)?
            .clear();
        *self.error.lock().map_err(executor_error_from_sync)? = None;
        self.drain_completed()?;
        self.scheduled.store(false, Ordering::Release);
        self.handle_live.store(true, Ordering::Release);
        self.waker_refs.store(0, Ordering::Release);
        self.waker.set_generation(generation);
        self.state.store(SLOT_PENDING, Ordering::Release);
        Ok(generation)
    }

    fn store_future<F>(&self, future: F) -> Result<(), ExecutorError>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.future
            .lock()
            .map_err(executor_error_from_sync)?
            .store_future(future)
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
            core::ptr::from_ref(&self.waker).cast::<()>(),
            &ASYNC_TASK_WAKER_VTABLE,
        );
        Ok(unsafe { Waker::from_raw(raw) })
    }

    fn poll_in_place(
        &self,
        generation: u64,
        context: &mut Context<'_>,
    ) -> Result<Poll<()>, ExecutorError> {
        if self.generation() != generation || self.state() != SLOT_PENDING {
            return Ok(Poll::Ready(()));
        }
        self.future
            .lock()
            .map_err(executor_error_from_sync)?
            .poll_in_place(&self.result, context)
    }

    fn complete(&self, generation: u64) -> Result<(), ExecutorError> {
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

        self.future
            .lock()
            .map_err(executor_error_from_sync)?
            .clear();
        *self.error.lock().map_err(executor_error_from_sync)? = None;
        self.scheduled.store(false, Ordering::Release);
        self.completed.release(1).map_err(executor_error_from_sync)
    }

    fn fail(&self, generation: u64, error: ExecutorError) -> Result<(), ExecutorError> {
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

        self.future
            .lock()
            .map_err(executor_error_from_sync)?
            .clear();
        self.result
            .lock()
            .map_err(executor_error_from_sync)?
            .clear();
        *self.error.lock().map_err(executor_error_from_sync)? = Some(error);
        self.scheduled.store(false, Ordering::Release);
        self.completed.release(1).map_err(executor_error_from_sync)
    }

    fn clear_core_if_no_wakers(&self, generation: u64) -> Result<bool, ExecutorError> {
        if self.generation() != generation {
            return Err(ExecutorError::Stopped);
        }
        if self.waker_refs.load(Ordering::Acquire) != 0 {
            return Ok(false);
        }
        *self.core.lock().map_err(executor_error_from_sync)? = None;
        self.waker.set_core(core::ptr::null());
        Ok(true)
    }

    fn force_shutdown(&self, generation: u64) -> Result<(), ExecutorError> {
        if self.generation() != generation {
            return Ok(());
        }

        match self.state() {
            SLOT_PENDING => {
                let _ = self.fail(generation, ExecutorError::Stopped);
            }
            SLOT_READY | SLOT_FAILED | SLOT_EMPTY => {}
            _ => return Err(executor_invalid()),
        }

        self.scheduled.store(false, Ordering::Release);
        let _ = self.clear_core_if_no_wakers(generation)?;
        Ok(())
    }

    fn is_finished(&self, generation: u64) -> Result<bool, ExecutorError> {
        if self.generation() != generation {
            return Err(ExecutorError::Stopped);
        }
        Ok(matches!(self.state(), SLOT_READY | SLOT_FAILED))
    }

    fn take_result<T: 'static>(&self, generation: u64) -> Result<T, ExecutorError> {
        if self.generation() != generation {
            return Err(ExecutorError::Stopped);
        }
        match self.state() {
            SLOT_READY => self
                .result
                .lock()
                .map_err(executor_error_from_sync)?
                .take::<T>(),
            SLOT_FAILED => Err(self
                .error
                .lock()
                .map_err(executor_error_from_sync)?
                .take()
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
            && matches!(state, SLOT_READY | SLOT_FAILED))
    }

    fn reset_empty(&self, generation: u64) -> Result<(), ExecutorError> {
        if self.generation() != generation {
            return Err(ExecutorError::Stopped);
        }

        self.future
            .lock()
            .map_err(executor_error_from_sync)?
            .clear();
        self.result
            .lock()
            .map_err(executor_error_from_sync)?
            .clear();
        *self.error.lock().map_err(executor_error_from_sync)? = None;
        self.drain_completed()?;
        self.scheduled.store(false, Ordering::Release);
        self.handle_live.store(false, Ordering::Release);
        self.state.store(SLOT_EMPTY, Ordering::Release);
        *self.core.lock().map_err(executor_error_from_sync)? = None;
        self.waker.set_core(core::ptr::null());
        Ok(())
    }

    fn drain_completed(&self) -> Result<(), ExecutorError> {
        while self
            .completed
            .try_acquire()
            .map_err(executor_error_from_sync)?
        {}
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
    free: SyncMutex<FixedIndexStack>,
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
    fn new(capacity: usize) -> Result<Self, ExecutorError> {
        let arena_capacity = executor_registry_capacity(capacity)?;
        let registry_align = align_of::<usize>().max(align_of::<AsyncTaskSlot>());
        let allocator = Allocator::<1, 1>::system_default_with_capacity(arena_capacity)
            .map_err(executor_error_from_alloc)?;
        let default_domain = allocator.default_domain().ok_or_else(executor_invalid)?;
        let arena = allocator
            .arena_with_alignment(default_domain, arena_capacity, registry_align)
            .map_err(executor_error_from_alloc)?;
        let slots = match arena.try_alloc_array_with(capacity, AsyncTaskSlot::new) {
            Ok(slots) => slots,
            Err(ArenaInitError::Alloc(error)) => return Err(executor_error_from_alloc(error)),
            Err(ArenaInitError::Init(error)) => return Err(error),
        };
        Ok(Self {
            slots,
            free: SyncMutex::new(FixedIndexStack::new_in(&arena, capacity)?),
            _arena: arena,
        })
    }

    fn slot(&self, slot_index: usize) -> Result<&AsyncTaskSlot, ExecutorError> {
        self.slots.get(slot_index).ok_or_else(executor_invalid)
    }

    fn allocate_slot(&self) -> Result<(usize, u64), ExecutorError> {
        let slot_index = self
            .free
            .lock()
            .map_err(executor_error_from_sync)?
            .pop()
            .ok_or_else(executor_busy)?;
        let generation = self.slot(slot_index)?.initialize_for_allocation()?;
        Ok((slot_index, generation))
    }

    fn release_slot(&self, slot_index: usize, generation: u64) -> Result<(), ExecutorError> {
        let slot = self.slot(slot_index)?;
        if slot.generation() != generation || slot.state() != SLOT_EMPTY {
            return Err(executor_invalid());
        }
        self.free
            .lock()
            .map_err(executor_error_from_sync)?
            .push(slot_index)
    }
}

#[derive(Debug)]
enum SchedulerBinding {
    Current,
    ThreadPool(ThreadPool),
    GreenPool(GreenPool),
    Unsupported,
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
    fn new(capacity: usize) -> Self {
        match AsyncTaskRegistry::new(capacity) {
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
    current_queue: CurrentQueue,
    scheduler: SchedulerBinding,
    next_id: AtomicUsize,
    registry: ExecutorRegistry,
}

impl fmt::Debug for ExecutorCore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecutorCore")
            .field("scheduler", &self.scheduler)
            .finish_non_exhaustive()
    }
}

impl ExecutorCore {
    fn allocate_task_id(&self) -> Result<TaskId, ExecutorError> {
        let sequence = self
            .next_id
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current: usize| {
                current.checked_add(1)
            })
            .map_err(|_| executor_overflow())?;
        Ok(TaskId::new(
            core::ptr::from_ref(self) as usize,
            sequence as u64,
        ))
    }

    fn registry(&self) -> Result<&AsyncTaskRegistry, ExecutorError> {
        self.registry.get()
    }

    fn schedule_slot(&self, slot_index: usize, generation: u64) -> Result<(), ExecutorError> {
        let slot = self.registry()?.slot(slot_index)?;
        if slot.generation() != generation || slot.state() != SLOT_PENDING {
            return Ok(());
        }
        if slot.scheduled.swap(true, Ordering::AcqRel) {
            return Ok(());
        }

        if let Err(error) = self.scheduler.schedule_slot(self, slot_index, generation) {
            slot.scheduled.store(false, Ordering::Release);
            let _ = slot.fail(generation, error);
            let _ = self.recycle_slot_if_possible(slot_index, generation);
            return Err(error);
        }
        Ok(())
    }

    fn run_slot_by_ref(&self, slot_index: usize, generation: u64) {
        let Ok(registry) = self.registry() else {
            return;
        };
        let Ok(slot) = registry.slot(slot_index) else {
            return;
        };
        if slot.generation() != generation || slot.state() != SLOT_PENDING {
            return;
        }

        slot.scheduled.store(false, Ordering::Release);

        let poll = {
            let Ok(waker) = slot.create_waker(generation) else {
                return;
            };
            let mut context = Context::from_waker(&waker);
            slot.poll_in_place(generation, &mut context)
        };

        match poll {
            Ok(Poll::Ready(())) => {
                let _ = slot.complete(generation);
                let _ = self.recycle_slot_if_possible(slot_index, generation);
            }
            Ok(Poll::Pending) => {}
            Err(error) => {
                let _ = slot.fail(generation, error);
                let _ = self.recycle_slot_if_possible(slot_index, generation);
            }
        }
    }

    fn drive_current_once(&self) -> Result<bool, ExecutorError> {
        match &self.scheduler {
            SchedulerBinding::Current => self.current_queue.run_next(),
            _ => Ok(false),
        }
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
        slot.reset_empty(generation)?;
        registry.release_slot(slot_index, generation)
    }

    fn detach_handle(&self, slot_index: usize, generation: u64) -> Result<(), ExecutorError> {
        let slot = self.registry()?.slot(slot_index)?;
        slot.mark_handle_released(generation)?;
        self.recycle_slot_if_possible(slot_index, generation)
    }

    fn shutdown(&self) {
        let Ok(registry) = self.registry() else {
            return;
        };
        for slot in &registry.slots {
            let generation = slot.generation();
            if generation == 0 {
                continue;
            }
            let _ = slot.force_shutdown(generation);
        }
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
    RawWaker::new(core::ptr::null(), &NOOP_ASYNC_TASK_WAKER_VTABLE)
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

/// Public spawned-task handle.
pub struct TaskHandle<T> {
    id: TaskId,
    core: ControlLease<ExecutorCore>,
    slot_index: usize,
    generation: u64,
    active: bool,
    _marker: PhantomData<T>,
}

impl<T> fmt::Debug for TaskHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskHandle")
            .field("id", &self.id)
            .field("slot_index", &self.slot_index)
            .field("generation", &self.generation)
            .field("active", &self.active)
            .finish_non_exhaustive()
    }
}

impl<T> TaskHandle<T> {
    /// Returns the stable task identifier.
    #[must_use]
    pub const fn id(&self) -> TaskId {
        self.id
    }

    /// Returns whether the task has completed.
    ///
    /// # Errors
    ///
    /// Returns an error if the task state cannot be observed honestly.
    pub fn is_finished(&self) -> Result<bool, ExecutorError> {
        self.core
            .registry()?
            .slot(self.slot_index)?
            .is_finished(self.generation)
    }

    /// Blocks until the task completes and returns its result.
    ///
    /// # Errors
    ///
    /// Returns the scheduler failure that stopped the task, if any.
    pub fn join(mut self) -> Result<T, ExecutorError>
    where
        T: 'static,
    {
        let slot = self.core.registry()?.slot(self.slot_index)?;
        match &self.core.scheduler {
            SchedulerBinding::Current => {
                while !slot.is_finished(self.generation)? {
                    if !self.core.drive_current_once()? && system_thread().yield_now().is_err() {
                        spin_loop();
                    }
                }
            }
            _ => {
                if !slot.is_finished(self.generation)? {
                    slot.completed.acquire().map_err(executor_error_from_sync)?;
                }
            }
        }

        let result = slot.take_result::<T>(self.generation);
        self.active = false;
        let _ = self.core.detach_handle(self.slot_index, self.generation);
        result
    }
}

impl<T> Drop for TaskHandle<T> {
    fn drop(&mut self) {
        if self.active {
            let _ = self.core.detach_handle(self.slot_index, self.generation);
        }
    }
}

#[derive(Debug)]
struct JoinSetState<T> {
    entries: [Option<TaskHandle<T>>; JOIN_SET_CAPACITY],
    len: usize,
}

impl<T> JoinSetState<T> {
    fn new() -> Self {
        Self {
            entries: array::from_fn(|_| None),
            len: 0,
        }
    }

    fn first_free_index(&self) -> Option<usize> {
        self.entries.iter().position(Option::is_none)
    }
}

/// Public set of task handles joined as a group.
pub struct JoinSet<T> {
    entries: SyncMutex<JoinSetState<T>>,
}

impl<T> fmt::Debug for JoinSet<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JoinSet").finish_non_exhaustive()
    }
}

impl<T> Default for JoinSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> JoinSet<T> {
    /// Creates an empty fixed-capacity join set.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: SyncMutex::new(JoinSetState::new()),
        }
    }

    /// Returns the number of tracked tasks.
    ///
    /// # Errors
    ///
    /// Returns an error if the set state cannot be observed honestly.
    pub fn len(&self) -> Result<usize, ExecutorError> {
        Ok(self.entries.lock().map_err(executor_error_from_sync)?.len)
    }

    /// Returns whether the set is empty.
    ///
    /// # Errors
    ///
    /// Returns an error if the set state cannot be observed honestly.
    pub fn is_empty(&self) -> Result<bool, ExecutorError> {
        Ok(self.len()? == 0)
    }

    /// Spawns a task through the supplied executor and tracks it in this join set.
    ///
    /// # Errors
    ///
    /// Returns any honest spawn failure, or `Overflow` if the join set is full.
    pub fn spawn<F>(&self, executor: &Executor, future: F) -> Result<TaskId, ExecutorError>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let mut state = self.entries.lock().map_err(executor_error_from_sync)?;
        let entry_index = state.first_free_index().ok_or_else(executor_overflow)?;
        let handle = executor.spawn(future)?;
        let id = handle.id();
        state.entries[entry_index] = Some(handle);
        state.len += 1;
        Ok(id)
    }

    /// Waits for the next tracked task to finish and returns its output.
    ///
    /// # Errors
    ///
    /// Returns `Stopped` when the set is empty, or any honest task/join failure.
    pub fn join_next(&self) -> Result<T, ExecutorError>
    where
        T: 'static,
    {
        loop {
            let mut current_executor = None;
            let ready = {
                let mut state = self.entries.lock().map_err(executor_error_from_sync)?;
                if state.len == 0 {
                    return Err(ExecutorError::Stopped);
                }

                let mut ready_index = None;
                for (index, entry) in state.entries.iter().enumerate() {
                    let Some(handle) = entry.as_ref() else {
                        continue;
                    };
                    if handle.is_finished()? {
                        ready_index = Some(index);
                        break;
                    }
                    if current_executor.is_none()
                        && matches!(handle.core.scheduler, SchedulerBinding::Current)
                    {
                        current_executor =
                            Some(handle.core.try_clone().map_err(executor_error_from_alloc)?);
                    }
                }

                ready_index.map_or_else(
                    || None,
                    |index| {
                        state.len -= 1;
                        state.entries[index].take()
                    },
                )
            };

            if let Some(handle) = ready {
                return handle.join();
            }

            if let Some(core) = current_executor {
                if !core.drive_current_once()? && system_thread().yield_now().is_err() {
                    spin_loop();
                }
                continue;
            }

            if system_thread().yield_now().is_err() {
                spin_loop();
            }
        }
    }
}

/// Public async executor wrapper.
#[derive(Debug)]
pub struct Executor {
    config: ExecutorConfig,
    reactor: Reactor,
    inner: ExecutorInner,
}

#[derive(Debug)]
enum ExecutorInner {
    Ready(ControlLease<ExecutorCore>),
    Error(ExecutorError),
}

impl Executor {
    fn with_scheduler(config: ExecutorConfig, scheduler: SchedulerBinding) -> Self {
        let reactor = Reactor::new();
        let inner = match ControlLease::<ExecutorCore>::extent_request()
            .map_err(executor_error_from_alloc)
            .and_then(|request| {
                Allocator::<1, 1>::system_default_with_capacity(request.len)
                    .map_err(executor_error_from_alloc)
            })
            .and_then(|allocator| {
                let default_domain = allocator.default_domain().ok_or_else(executor_invalid)?;
                allocator
                    .control(
                        default_domain,
                        ExecutorCore {
                            current_queue: CurrentQueue::new(),
                            scheduler,
                            next_id: AtomicUsize::new(1),
                            registry: ExecutorRegistry::new(TASK_REGISTRY_CAPACITY),
                        },
                    )
                    .map_err(executor_error_from_alloc)
            }) {
            Ok(core) => ExecutorInner::Ready(core),
            Err(error) => ExecutorInner::Error(error),
        };
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
        Self::with_scheduler(config, scheduler)
    }

    /// Returns the configured executor mode.
    #[must_use]
    pub const fn mode(&self) -> ExecutorMode {
        self.config.mode
    }

    /// Returns the public reactor wrapper.
    #[must_use]
    pub const fn reactor(&self) -> &Reactor {
        &self.reactor
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
            slot.reset_empty(generation)?;
            registry.release_slot(slot_index, generation)?;
            return Err(error);
        }

        if let Err(error) = slot.store_future(future) {
            slot.mark_handle_released(generation)?;
            slot.reset_empty(generation)?;
            registry.release_slot(slot_index, generation)?;
            return Err(error);
        }

        if let Err(error) = core.schedule_slot(slot_index, generation) {
            slot.mark_handle_released(generation)?;
            let _ = core.recycle_slot_if_possible(slot_index, generation);
            return Err(error);
        }

        Ok(TaskHandle {
            id,
            core: handle_core,
            slot_index,
            generation,
            active: true,
            _marker: PhantomData,
        })
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
        let core = self.core()?;
        let SchedulerBinding::Current = &core.scheduler else {
            return Err(ExecutorError::Unsupported);
        };

        let handle = self.spawn(future)?;
        while !handle.is_finished()? {
            if !core.current_queue.run_next()? && system_thread().yield_now().is_err() {
                spin_loop();
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
        if !matches!(self.config.mode, ExecutorMode::ThreadPool) {
            return Err(ExecutorError::Unsupported);
        }

        Ok(Self::with_scheduler(
            self.config,
            SchedulerBinding::ThreadPool(
                pool.try_clone().map_err(executor_error_from_thread_pool)?,
            ),
        ))
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

        Ok(Self::with_scheduler(
            self.config,
            SchedulerBinding::GreenPool(green.try_clone().map_err(executor_error_from_fiber)?),
        ))
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
    core.run_slot_by_ref(slot_index, generation);
}

fn run_scheduled_slot_ptr(core: ScheduledExecutorCorePtr, slot_index: usize, generation: u64) {
    core.run_slot(slot_index, generation);
}

#[cfg(feature = "std")]
fn poll_future_contained<F>(
    future: Pin<&mut F>,
    context: &mut Context<'_>,
) -> Result<Poll<F::Output>, ()>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    #[cfg(feature = "std")]
    {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        catch_unwind(AssertUnwindSafe(|| future.poll(context))).map_err(|_| ())
    }
}

#[cfg(not(feature = "std"))]
fn poll_future_contained<F>(future: Pin<&mut F>, context: &mut Context<'_>) -> Poll<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    future.poll(context)
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

const fn executor_error_from_fiber(error: FiberError) -> ExecutorError {
    match error.kind() {
        FiberErrorKind::Unsupported => ExecutorError::Unsupported,
        FiberErrorKind::ResourceExhausted => ExecutorError::Sync(SyncErrorKind::Overflow),
        FiberErrorKind::Invalid
        | FiberErrorKind::DeadlineExceeded
        | FiberErrorKind::StateConflict
        | FiberErrorKind::Context(_) => ExecutorError::Sync(SyncErrorKind::Invalid),
    }
}

fn executor_registry_capacity(capacity: usize) -> Result<usize, ExecutorError> {
    if capacity == 0 {
        return Err(executor_invalid());
    }

    let free_bytes = size_of::<usize>()
        .checked_mul(capacity)
        .ok_or_else(executor_overflow)?;
    let slot_bytes = size_of::<AsyncTaskSlot>()
        .checked_mul(capacity)
        .ok_or_else(executor_overflow)?;
    let padding = align_of::<usize>().max(align_of::<AsyncTaskSlot>());
    free_bytes
        .checked_add(slot_bytes)
        .and_then(|total| total.checked_add(padding.saturating_mul(2)))
        .ok_or_else(executor_overflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_reuses_slots_with_new_generations() {
        let executor = Executor::new(ExecutorConfig::new());

        let first = executor
            .spawn(async { 7_u8 })
            .expect("first task should spawn");
        let first_slot = first.slot_index;
        let first_generation = first.generation;
        assert_eq!(first.join().expect("first task should finish"), 7);

        let second = executor
            .spawn(async { 9_u8 })
            .expect("second task should spawn");
        assert_eq!(second.slot_index, first_slot);
        assert!(second.generation > first_generation);
        assert_eq!(second.join().expect("second task should finish"), 9);
    }

    #[test]
    fn join_set_drives_current_thread_tasks_to_completion() {
        let executor = Executor::new(ExecutorConfig::new());
        let join_set = JoinSet::<u8>::new();

        join_set
            .spawn(&executor, async { 3_u8 })
            .expect("first join-set task should spawn");
        join_set
            .spawn(&executor, async { 5_u8 })
            .expect("second join-set task should spawn");

        let first = join_set.join_next().expect("first task should complete");
        let second = join_set.join_next().expect("second task should complete");
        assert!(matches!((first, second), (3, 5) | (5, 3)));
        assert!(matches!(join_set.join_next(), Err(ExecutorError::Stopped)));
    }

    #[test]
    fn oversized_futures_are_rejected_honestly() {
        let executor = Executor::new(ExecutorConfig::new());
        let oversized = [0_u8; 1024];

        assert!(matches!(
            executor.spawn(async move { oversized.len() }),
            Err(ExecutorError::Unsupported)
        ));
    }

    #[test]
    fn dropping_executor_shuts_down_live_pending_slots() {
        let executor = Executor::new(ExecutorConfig::new());
        let handle = executor
            .spawn(core::future::pending::<u8>())
            .expect("pending task should spawn");
        let slot_index = handle.slot_index;
        let generation = handle.generation;
        let core = handle
            .core
            .try_clone()
            .expect("task handle should retain executor core");

        drop(executor);

        let slot = core
            .registry()
            .expect("registry should stay alive through the task handle")
            .slot(slot_index)
            .expect("slot should still be addressable");
        assert_eq!(slot.state(), SLOT_FAILED);
        assert!(
            slot.core
                .lock()
                .expect("slot core lock should succeed")
                .is_none()
        );
        assert!(slot.waker.core_ptr().is_null());
        assert!(matches!(handle.join(), Err(ExecutorError::Stopped)));
        assert_eq!(slot.generation(), generation);
    }
}
