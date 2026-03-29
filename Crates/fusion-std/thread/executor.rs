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

use core::any::{TypeId, type_name};
use core::array;
use core::cell::UnsafeCell;
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
use fusion_pal::sys::cpu::CachePadded;
use fusion_pal::sys::mem::MemBase;
use fusion_sys::alloc::{
    AllocError,
    AllocErrorKind,
    AllocationStrategy,
    Allocator,
    ArenaInitError,
    ArenaSlice,
    BoundedArena,
    ControlLease,
    Slab,
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
use fusion_sys::mem::resource::{
    AllocatorLayoutPolicy,
    BoundMemoryResource,
    BoundResourceSpec,
    MemoryResource,
    MemoryResourceHandle,
    ResourceBackingKind,
    ResourceError,
    ResourceErrorKind,
    ResourceRange,
    ResourceRequest,
    VirtualMemoryResource,
};
use fusion_sys::sync::Mutex as SysMutex;
use fusion_sys::thread::{
    CanonicalInstant,
    MonotonicRawInstant,
    system_monotonic_time,
    system_thread,
};
#[cfg(feature = "std")]
use fusion_sys::thread::{
    ThreadConfig,
    ThreadEntryReturn,
    ThreadHandle,
    ThreadJoinPolicy,
    ThreadPlacementRequest,
    ThreadStartMode,
    ThreadSystem,
};

#[cfg(feature = "std")]
use super::HostedFiberRuntime;
use super::{
    GreenPool,
    RuntimeSizingStrategy,
    ThreadPool,
    default_runtime_sizing_strategy,
    yield_now as green_yield_now,
};
#[cfg(feature = "std")]
use fusion_pal::sys::fiber::{PlatformFiberWakeSignal, system_fiber_host};
#[cfg(feature = "std")]
use std::string::String;
#[cfg(feature = "std")]
use std::sync::Arc;
#[cfg(feature = "std")]
use std::thread::{Builder as StdThreadBuilder, JoinHandle};
#[cfg(feature = "std")]
use std::vec::Vec;

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

/// Truthful admission snapshot for one spawned async task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AsyncTaskAdmission {
    /// Carrier realization the task was admitted onto.
    pub carrier: ExecutorMode,
    /// Concrete future frame size in bytes.
    pub future_bytes: usize,
    /// Concrete future frame alignment in bytes.
    pub future_align: usize,
    /// Effective future storage class selected for the task.
    pub future_storage_class: AsyncStorageClass,
    /// Concrete output storage size in bytes.
    pub output_bytes: usize,
    /// Concrete output storage alignment in bytes.
    pub output_align: usize,
    /// Effective output storage class selected for the task.
    pub output_storage_class: AsyncStorageClass,
    /// Distinct poll-stack contract carried alongside the future frame layout.
    pub poll_stack: AsyncPollStackContract,
}

impl AsyncTaskAdmission {
    fn for_future<F>(carrier: ExecutorMode) -> Self
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        let future_storage_class = async_storage_class_for_layout(
            size_of::<F>(),
            align_of::<F>(),
            INLINE_ASYNC_FUTURE_BYTES,
            ASYNC_FUTURE_CLASS_MEDIUM_BYTES,
            ASYNC_FUTURE_CLASS_LARGE_BYTES,
        );
        let output_storage_class = async_storage_class_for_layout(
            size_of::<F::Output>(),
            align_of::<F::Output>(),
            INLINE_ASYNC_RESULT_BYTES,
            ASYNC_RESULT_CLASS_MEDIUM_BYTES,
            ASYNC_RESULT_CLASS_LARGE_BYTES,
        );
        let poll_stack = generated_async_poll_stack_contract::<F>().unwrap_or_else(|| {
            AsyncPollStackContract::from_future_storage_class(future_storage_class)
        });
        Self {
            carrier,
            future_bytes: size_of::<F>(),
            future_align: align_of::<F>(),
            future_storage_class,
            output_bytes: size_of::<F::Output>(),
            output_align: align_of::<F::Output>(),
            output_storage_class,
            poll_stack,
        }
    }

    const fn with_poll_stack_bytes(mut self, bytes: usize) -> Self {
        self.poll_stack = AsyncPollStackContract::from_bytes(bytes);
        self
    }
}

/// Effective slab class selected for one async frame or result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AsyncStorageClass {
    /// Stored inline inside the task slot.
    Inline,
    /// Stored in the medium slab-backed class.
    Medium,
    /// Stored in the large slab-backed class.
    Large,
    /// Does not fit one supported storage class honestly.
    Unsupported,
}

/// Separate poll-stack contract for one async task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AsyncPollStackContract {
    /// No honest poll-stack bound has been attached.
    Unknown,
    /// One build-generated poll-stack budget was emitted for this exact future type.
    Generated { bytes: usize },
    /// One generated heuristic poll-stack budget was derived from the admitted future frame class.
    DerivedHeuristic { bytes: usize },
    /// One explicit poll-stack byte budget was attached to the task admission.
    Explicit { bytes: usize },
}

impl AsyncPollStackContract {
    const fn from_future_storage_class(class: AsyncStorageClass) -> Self {
        match class {
            AsyncStorageClass::Inline => Self::DerivedHeuristic { bytes: 512 },
            AsyncStorageClass::Medium => Self::DerivedHeuristic { bytes: 1024 },
            AsyncStorageClass::Large => Self::DerivedHeuristic { bytes: 2048 },
            AsyncStorageClass::Unsupported => Self::Unknown,
        }
    }

    const fn from_bytes(bytes: usize) -> Self {
        if bytes == 0 {
            Self::Unknown
        } else {
            Self::Explicit { bytes }
        }
    }
}

#[doc(hidden)]
pub struct GeneratedAsyncPollStackMetadataEntry {
    pub type_name: &'static str,
    pub poll_stack_bytes: usize,
}

/// Hidden compile-time async poll-stack contract emitted or declared inside the current crate.
#[doc(hidden)]
pub trait GeneratedExplicitAsyncPollStackContract {
    const POLL_STACK_BYTES: usize;
}

include!(concat!(env!("OUT_DIR"), "/async_task_generated.rs"));

/// Returns the compile-time generated async poll-stack budget for one nameable future type.
#[must_use]
pub const fn generated_explicit_async_poll_stack_bytes<
    T: GeneratedExplicitAsyncPollStackContract,
>() -> usize {
    T::POLL_STACK_BYTES
}

/// Includes one generated Rust sidecar emitted by the async poll-stack analyzer pipeline.
#[macro_export]
macro_rules! include_generated_async_poll_stack_contracts {
    ($path:expr $(,)?) => {
        include!($path);
    };
}

/// Declares one build-generated async poll-stack contract for use in downstream crates.
#[macro_export]
macro_rules! declare_generated_async_poll_stack_contract {
    ($future:ty, $poll_stack_bytes:expr $(,)?) => {
        impl $crate::thread::GeneratedExplicitAsyncPollStackContract for $future {
            const POLL_STACK_BYTES: usize = $poll_stack_bytes;
        }
    };
}

#[doc(hidden)]
pub struct GeneratedAsyncPollStackMetadataAnchorFuture;

impl Future for GeneratedAsyncPollStackMetadataAnchorFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(())
    }
}

/// Hidden async-poll anchor used to exercise the build-generated poll-stack metadata pipeline in
/// normal library artifacts before link-time stripping can erase the evidence.
#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "Rust" fn generated_async_poll_stack_metadata_anchor() -> bool {
    let waker = unsafe { Waker::from_raw(noop_async_task_raw_waker()) };
    let mut context = Context::from_waker(&waker);
    let mut future = GeneratedAsyncPollStackMetadataAnchorFuture;
    matches!(
        generated_async_poll_stack_root(unsafe { Pin::new_unchecked(&mut future) }, &mut context),
        Poll::Ready(())
    )
}

#[inline(never)]
fn generated_async_poll_stack_root<F>(
    future: Pin<&mut F>,
    context: &mut Context<'_>,
) -> Poll<F::Output>
where
    F: Future,
{
    Future::poll(future, context)
}

fn generated_async_poll_stack_bytes_by_type_name(type_name: &str) -> Option<usize> {
    GENERATED_ASYNC_POLL_STACK_TASKS
        .binary_search_by(|entry| entry.type_name.cmp(type_name))
        .ok()
        .map(|index| GENERATED_ASYNC_POLL_STACK_TASKS[index].poll_stack_bytes)
}

fn generated_async_poll_stack_contract<F: 'static>() -> Option<AsyncPollStackContract> {
    generated_async_poll_stack_bytes_by_type_name(type_name::<F>())
        .map(|bytes| AsyncPollStackContract::Generated { bytes })
}

fn runtime_monotonic_now_instant() -> Result<CanonicalInstant, ExecutorError> {
    system_monotonic_time()
        .now_instant()
        .map_err(executor_error_from_thread)
}

fn runtime_monotonic_raw_now() -> Result<MonotonicRawInstant, ExecutorError> {
    system_monotonic_time()
        .raw_now()
        .map_err(executor_error_from_thread)
}

fn runtime_monotonic_checked_add(
    base: CanonicalInstant,
    duration: Duration,
) -> Result<CanonicalInstant, ExecutorError> {
    system_monotonic_time()
        .checked_add_duration(base, duration)
        .map_err(executor_error_from_thread)
}

fn runtime_monotonic_duration_until(deadline: CanonicalInstant) -> Result<Duration, ExecutorError> {
    system_monotonic_time()
        .duration_until(deadline)
        .map_err(executor_error_from_thread)
}

#[derive(Debug)]
struct ExecutorDomainAllocator {
    allocator: Allocator<1, 1>,
    domain: fusion_sys::alloc::AllocatorDomainId,
}

impl ExecutorDomainAllocator {
    fn from_resource(handle: MemoryResourceHandle) -> Result<Self, ExecutorError> {
        let allocator =
            Allocator::<1, 1>::from_resource(handle).map_err(executor_error_from_alloc)?;
        let domain = allocator.default_domain().ok_or_else(executor_invalid)?;
        Ok(Self { allocator, domain })
    }

    fn acquire_virtual(
        request: ExecutorBackingRequest,
        name: &'static str,
    ) -> Result<Self, ExecutorError> {
        let mut resource_request = ResourceRequest::anonymous_private(request.bytes);
        resource_request.name = Some(name);
        let resource = VirtualMemoryResource::create(&resource_request)
            .map_err(executor_error_from_resource)?;
        Self::from_resource(MemoryResourceHandle::from(resource))
    }

    fn arena(&self, capacity: usize, max_align: usize) -> Result<BoundedArena, ExecutorError> {
        self.allocator
            .arena_with_alignment(self.domain, capacity, max_align)
            .map_err(executor_error_from_alloc)
    }

    fn control<T>(&self, value: T) -> Result<ControlLease<T>, ExecutorError> {
        self.allocator
            .control(self.domain, value)
            .map_err(executor_error_from_alloc)
    }

    fn slab<const SIZE: usize, const COUNT: usize>(
        &self,
    ) -> Result<Slab<SIZE, COUNT>, ExecutorError> {
        self.allocator
            .slab::<SIZE, COUNT>(self.domain)
            .map_err(executor_error_from_alloc)
    }
}

#[derive(Debug)]
struct ExecutorBackingAllocators {
    control: ExecutorDomainAllocator,
    reactor: ExecutorDomainAllocator,
    registry: ExecutorDomainAllocator,
    future_medium: Option<ExecutorDomainAllocator>,
    future_large: Option<ExecutorDomainAllocator>,
    result_medium: Option<ExecutorDomainAllocator>,
    result_large: Option<ExecutorDomainAllocator>,
}

impl ExecutorBackingAllocators {
    fn acquire_current(config: ExecutorConfig) -> Result<Self, ExecutorError> {
        Self::from_current_backing(current_async_runtime_virtual_backing(config)?)
    }

    fn from_current_backing(backing: CurrentAsyncRuntimeBacking) -> Result<Self, ExecutorError> {
        Ok(Self {
            control: ExecutorDomainAllocator::from_resource(backing.control)?,
            reactor: ExecutorDomainAllocator::from_resource(backing.reactor)?,
            registry: ExecutorDomainAllocator::from_resource(backing.registry)?,
            future_medium: backing
                .future_medium
                .map(ExecutorDomainAllocator::from_resource)
                .transpose()?,
            future_large: backing
                .future_large
                .map(ExecutorDomainAllocator::from_resource)
                .transpose()?,
            result_medium: backing
                .result_medium
                .map(ExecutorDomainAllocator::from_resource)
                .transpose()?,
            result_large: backing
                .result_large
                .map(ExecutorDomainAllocator::from_resource)
                .transpose()?,
        })
    }
}

const fn async_storage_class_for_layout(
    bytes: usize,
    align: usize,
    inline_bytes: usize,
    medium_bytes: usize,
    large_bytes: usize,
) -> AsyncStorageClass {
    if bytes <= inline_bytes && align <= align_of::<InlineAsyncFutureBytes>() {
        return AsyncStorageClass::Inline;
    }
    if bytes <= medium_bytes && align <= async_storage_class_slot_align(medium_bytes) {
        return AsyncStorageClass::Medium;
    }
    if bytes <= large_bytes && align <= async_storage_class_slot_align(large_bytes) {
        return AsyncStorageClass::Large;
    }
    AsyncStorageClass::Unsupported
}

const fn async_storage_class_slot_align(bytes: usize) -> usize {
    1usize << bytes.trailing_zeros()
}

fn apply_executor_sizing_strategy(
    request: ExecutorBackingRequest,
    strategy: RuntimeSizingStrategy,
) -> Result<ExecutorBackingRequest, ExecutorError> {
    let bytes = strategy
        .apply_bytes(request.bytes)
        .ok_or_else(executor_overflow)?;
    Ok(ExecutorBackingRequest {
        bytes,
        align: request.align,
    })
}

/// Cooperative async yield future for the Fusion executor.
#[derive(Debug, Default, Clone, Copy)]
pub struct AsyncYieldNow {
    yielded: bool,
}

impl Future for AsyncYieldNow {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.yielded {
            return Poll::Ready(());
        }
        self.yielded = true;
        if !mark_current_async_requeue() {
            cx.waker().wake_by_ref();
        }
        Poll::Pending
    }
}

/// Returns one future that cooperatively yields back into the Fusion executor once.
#[must_use]
pub const fn async_yield_now() -> AsyncYieldNow {
    AsyncYieldNow { yielded: false }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum AsyncWaitOutcome {
    Readiness(EventReadiness),
    Timer,
    #[cfg(feature = "std")]
    Error(ExecutorError),
}

#[derive(Debug, Clone, Copy)]
struct CurrentAsyncTaskContext {
    core: usize,
    slot_index: usize,
    generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AsyncTaskSchedulerTag {
    Current = 1,
    ThreadWorkers = 2,
    GreenPool = 3,
    Unsupported = 4,
}

impl AsyncTaskSchedulerTag {
    const fn from_scheduler(scheduler: &SchedulerBinding) -> Self {
        match scheduler {
            SchedulerBinding::Current => Self::Current,
            #[cfg(not(feature = "std"))]
            SchedulerBinding::ThreadPool(_) => Self::ThreadWorkers,
            #[cfg(feature = "std")]
            SchedulerBinding::ThreadWorkers(_) => Self::ThreadWorkers,
            SchedulerBinding::GreenPool(_) => Self::GreenPool,
            SchedulerBinding::Unsupported => Self::Unsupported,
        }
    }

    const fn from_raw(raw: usize) -> Option<Self> {
        match raw {
            1 => Some(Self::Current),
            2 => Some(Self::ThreadWorkers),
            3 => Some(Self::GreenPool),
            4 => Some(Self::Unsupported),
            _ => None,
        }
    }
}

#[cfg(feature = "std")]
#[thread_local]
static mut CURRENT_ASYNC_TASK_CORE_STD: usize = 0;
#[cfg(feature = "std")]
#[thread_local]
static mut CURRENT_ASYNC_TASK_SLOT_STD: usize = usize::MAX;
#[cfg(feature = "std")]
#[thread_local]
static mut CURRENT_ASYNC_TASK_GENERATION_STD: usize = 0;
#[cfg(feature = "std")]
#[thread_local]
static mut CURRENT_ASYNC_TASK_REQUEUE_STD: bool = false;
#[cfg(feature = "std")]
#[thread_local]
static mut CURRENT_ASYNC_TASK_SCHEDULER_STD: usize = 0;
#[cfg(not(feature = "std"))]
static CURRENT_ASYNC_TASK_REQUEUE: AtomicBool = AtomicBool::new(false);

#[cfg(not(feature = "std"))]
static CURRENT_ASYNC_TASK_CORE: AtomicUsize = AtomicUsize::new(0);
#[cfg(not(feature = "std"))]
static CURRENT_ASYNC_TASK_SLOT: AtomicUsize = AtomicUsize::new(usize::MAX);
#[cfg(not(feature = "std"))]
static CURRENT_ASYNC_TASK_GENERATION: AtomicUsize = AtomicUsize::new(0);
#[cfg(not(feature = "std"))]
static CURRENT_ASYNC_TASK_SCHEDULER: AtomicUsize = AtomicUsize::new(0);
fn current_async_task_context() -> Option<CurrentAsyncTaskContext> {
    #[cfg(feature = "std")]
    {
        let core = unsafe { CURRENT_ASYNC_TASK_CORE_STD };
        if core == 0 {
            return None;
        }
        Some(CurrentAsyncTaskContext {
            core,
            slot_index: unsafe { CURRENT_ASYNC_TASK_SLOT_STD },
            generation: unsafe { CURRENT_ASYNC_TASK_GENERATION_STD } as u64,
        })
    }

    #[cfg(not(feature = "std"))]
    {
        let core = CURRENT_ASYNC_TASK_CORE.load(Ordering::Acquire);
        if core == 0 {
            return None;
        }
        Some(CurrentAsyncTaskContext {
            core,
            slot_index: CURRENT_ASYNC_TASK_SLOT.load(Ordering::Acquire),
            generation: CURRENT_ASYNC_TASK_GENERATION.load(Ordering::Acquire) as u64,
        })
    }
}

fn set_current_async_task_context(context: Option<CurrentAsyncTaskContext>) {
    #[cfg(feature = "std")]
    {
        unsafe {
            if let Some(context) = context {
                CURRENT_ASYNC_TASK_CORE_STD = context.core;
                CURRENT_ASYNC_TASK_SLOT_STD = context.slot_index;
                CURRENT_ASYNC_TASK_GENERATION_STD =
                    usize::try_from(context.generation).unwrap_or(usize::MAX);
            } else {
                CURRENT_ASYNC_TASK_CORE_STD = 0;
                CURRENT_ASYNC_TASK_SLOT_STD = usize::MAX;
                CURRENT_ASYNC_TASK_GENERATION_STD = 0;
            }
            CURRENT_ASYNC_TASK_REQUEUE_STD = false;
        }
    }

    #[cfg(not(feature = "std"))]
    {
        if let Some(context) = context {
            CURRENT_ASYNC_TASK_GENERATION.store(context.generation as usize, Ordering::Release);
            CURRENT_ASYNC_TASK_SLOT.store(context.slot_index, Ordering::Release);
            CURRENT_ASYNC_TASK_CORE.store(context.core as usize, Ordering::Release);
        } else {
            CURRENT_ASYNC_TASK_CORE.store(0, Ordering::Release);
            CURRENT_ASYNC_TASK_SLOT.store(usize::MAX, Ordering::Release);
            CURRENT_ASYNC_TASK_GENERATION.store(0, Ordering::Release);
        }
        CURRENT_ASYNC_TASK_REQUEUE.store(false, Ordering::Release);
    }
}

fn current_async_task_scheduler() -> Option<AsyncTaskSchedulerTag> {
    #[cfg(feature = "std")]
    {
        AsyncTaskSchedulerTag::from_raw(unsafe { CURRENT_ASYNC_TASK_SCHEDULER_STD })
    }

    #[cfg(not(feature = "std"))]
    {
        AsyncTaskSchedulerTag::from_raw(CURRENT_ASYNC_TASK_SCHEDULER.load(Ordering::Acquire))
    }
}

#[derive(Debug)]
struct AsyncTaskContextGuard;

impl AsyncTaskContextGuard {
    fn install(core: &ExecutorCore, slot_index: usize, generation: u64) -> Self {
        set_current_async_task_context(Some(CurrentAsyncTaskContext {
            core: core::ptr::from_ref(core) as usize,
            slot_index,
            generation,
        }));
        #[cfg(feature = "std")]
        unsafe {
            CURRENT_ASYNC_TASK_SCHEDULER_STD =
                AsyncTaskSchedulerTag::from_scheduler(&core.scheduler) as usize;
        }
        #[cfg(not(feature = "std"))]
        CURRENT_ASYNC_TASK_SCHEDULER.store(
            AsyncTaskSchedulerTag::from_scheduler(&core.scheduler) as usize,
            Ordering::Release,
        );
        Self
    }
}

impl Drop for AsyncTaskContextGuard {
    fn drop(&mut self) {
        set_current_async_task_context(None);
        #[cfg(feature = "std")]
        unsafe {
            CURRENT_ASYNC_TASK_SCHEDULER_STD = 0;
        }
        #[cfg(not(feature = "std"))]
        CURRENT_ASYNC_TASK_SCHEDULER.store(0, Ordering::Release);
    }
}

fn mark_current_async_requeue() -> bool {
    if current_async_task_context().is_none() {
        return false;
    }
    #[cfg(feature = "std")]
    unsafe {
        CURRENT_ASYNC_TASK_REQUEUE_STD = true;
    }
    #[cfg(not(feature = "std"))]
    CURRENT_ASYNC_TASK_REQUEUE.store(true, Ordering::Release);
    true
}

fn take_current_async_requeue() -> bool {
    #[cfg(feature = "std")]
    {
        return unsafe {
            let value = CURRENT_ASYNC_TASK_REQUEUE_STD;
            CURRENT_ASYNC_TASK_REQUEUE_STD = false;
            value
        };
    }

    #[cfg(not(feature = "std"))]
    {
        CURRENT_ASYNC_TASK_REQUEUE.swap(false, Ordering::AcqRel)
    }
}

#[derive(Debug, Clone, Copy)]
struct AsyncWaitRegistration {
    core: usize,
    slot_index: usize,
    generation: u64,
}

impl AsyncWaitRegistration {
    fn from_current() -> Result<Self, ExecutorError> {
        let context = current_async_task_context().ok_or(ExecutorError::Unsupported)?;
        Ok(Self {
            core: context.core,
            slot_index: context.slot_index,
            generation: context.generation,
        })
    }

    fn clear(self) -> Result<(), ExecutorError> {
        // SAFETY: registrations are only created while the owning task is actively being polled.
        unsafe { (self.core as *const ExecutorCore).as_ref() }
            .ok_or(ExecutorError::Stopped)?
            .clear_wait(self.slot_index, self.generation)
    }
}

/// One future that resolves when the selected source reports readiness.
#[derive(Debug, Clone)]
pub struct AsyncWaitForReadiness {
    source: EventSourceHandle,
    interest: EventInterest,
    registration: Option<AsyncWaitRegistration>,
}

impl Future for AsyncWaitForReadiness {
    type Output = Result<EventReadiness, ExecutorError>;

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        if matches!(
            current_async_task_scheduler(),
            Some(AsyncTaskSchedulerTag::GreenPool)
        ) {
            self.registration = None;
            return Poll::Ready(Err(ExecutorError::Unsupported));
        }
        if let Some(registration) = self.registration {
            let core = unsafe { (registration.core as *const ExecutorCore).as_ref() }
                .ok_or(ExecutorError::Stopped);
            match core.and_then(|core| {
                core.take_wait_outcome(registration.slot_index, registration.generation)
            }) {
                Ok(Some(AsyncWaitOutcome::Readiness(readiness))) => {
                    self.registration = None;
                    Poll::Ready(Ok(readiness))
                }
                #[cfg(feature = "std")]
                Ok(Some(AsyncWaitOutcome::Error(error))) => {
                    self.registration = None;
                    Poll::Ready(Err(error))
                }
                Ok(Some(AsyncWaitOutcome::Timer)) => {
                    Poll::Ready(Err(ExecutorError::Sync(SyncErrorKind::Invalid)))
                }
                Ok(None) => Poll::Pending,
                Err(error) => Poll::Ready(Err(error)),
            }
        } else {
            let registration = match AsyncWaitRegistration::from_current() {
                Ok(registration) => registration,
                Err(error) => return Poll::Ready(Err(error)),
            };
            let core = unsafe { (registration.core as *const ExecutorCore).as_ref() }
                .ok_or(ExecutorError::Stopped);
            match core.and_then(|core| {
                core.register_readiness_wait(
                    registration.slot_index,
                    registration.generation,
                    self.source,
                    self.interest,
                )
            }) {
                Ok(()) => {
                    self.registration = Some(registration);
                    Poll::Pending
                }
                Err(error) => Poll::Ready(Err(error)),
            }
        }
    }
}

impl Drop for AsyncWaitForReadiness {
    fn drop(&mut self) {
        if let Some(registration) = self.registration.take() {
            let _ = registration.clear();
        }
    }
}

/// Returns one future that waits for the selected readiness source inside the Fusion executor.
#[must_use]
pub const fn async_wait_for_readiness(
    source: EventSourceHandle,
    interest: EventInterest,
) -> AsyncWaitForReadiness {
    AsyncWaitForReadiness {
        source,
        interest,
        registration: None,
    }
}

/// One future that resolves at the selected monotonic deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum AsyncSleepDeadline {
    Canonical(CanonicalInstant),
    LegacyDuration(Duration),
}

#[derive(Debug, Clone)]
pub struct AsyncSleepUntil {
    deadline: AsyncSleepDeadline,
    registration: Option<AsyncWaitRegistration>,
}

impl Future for AsyncSleepUntil {
    type Output = Result<(), ExecutorError>;

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        if matches!(
            current_async_task_scheduler(),
            Some(AsyncTaskSchedulerTag::GreenPool)
        ) {
            self.registration = None;
            return Poll::Ready(Err(ExecutorError::Unsupported));
        }
        let deadline = match self.deadline {
            AsyncSleepDeadline::Canonical(deadline) => deadline,
            AsyncSleepDeadline::LegacyDuration(duration) => {
                let deadline = match system_monotonic_time().instant_from_duration(duration) {
                    Ok(deadline) => deadline,
                    Err(error) => return Poll::Ready(Err(executor_error_from_thread(error))),
                };
                self.deadline = AsyncSleepDeadline::Canonical(deadline);
                deadline
            }
        };
        let now = match runtime_monotonic_now_instant() {
            Ok(now) => now,
            Err(error) => return Poll::Ready(Err(error)),
        };
        if now >= deadline {
            if let Some(registration) = self.registration.take() {
                let _ = registration.clear();
            }
            return Poll::Ready(Ok(()));
        }

        if let Some(registration) = self.registration {
            let core = unsafe { (registration.core as *const ExecutorCore).as_ref() }
                .ok_or(ExecutorError::Stopped);
            match core.and_then(|core| {
                core.take_wait_outcome(registration.slot_index, registration.generation)
            }) {
                Ok(Some(AsyncWaitOutcome::Timer)) => {
                    self.registration = None;
                    Poll::Ready(Ok(()))
                }
                #[cfg(feature = "std")]
                Ok(Some(AsyncWaitOutcome::Error(error))) => {
                    self.registration = None;
                    Poll::Ready(Err(error))
                }
                Ok(Some(AsyncWaitOutcome::Readiness(_))) => {
                    Poll::Ready(Err(ExecutorError::Sync(SyncErrorKind::Invalid)))
                }
                Ok(None) => Poll::Pending,
                Err(error) => Poll::Ready(Err(error)),
            }
        } else {
            let registration = match AsyncWaitRegistration::from_current() {
                Ok(registration) => registration,
                Err(error) => return Poll::Ready(Err(error)),
            };
            let core = unsafe { (registration.core as *const ExecutorCore).as_ref() }
                .ok_or(ExecutorError::Stopped);
            match core.and_then(|core| {
                core.register_sleep_wait(registration.slot_index, registration.generation, deadline)
            }) {
                Ok(()) => {
                    self.registration = Some(registration);
                    Poll::Pending
                }
                Err(error) => Poll::Ready(Err(error)),
            }
        }
    }
}

impl Drop for AsyncSleepUntil {
    fn drop(&mut self) {
        if let Some(registration) = self.registration.take() {
            let _ = registration.clear();
        }
    }
}

/// Returns one future that resolves at the selected monotonic deadline.
#[must_use]
pub const fn async_sleep_until_instant(deadline: CanonicalInstant) -> AsyncSleepUntil {
    AsyncSleepUntil {
        deadline: AsyncSleepDeadline::Canonical(deadline),
        registration: None,
    }
}

/// Returns one future that resolves at the selected monotonic deadline expressed as elapsed
/// runtime time from the backend-defined monotonic origin.
#[must_use]
pub const fn async_sleep_until(deadline: Duration) -> AsyncSleepUntil {
    AsyncSleepUntil {
        deadline: AsyncSleepDeadline::LegacyDuration(deadline),
        registration: None,
    }
}

/// One future that resolves after the selected duration on the monotonic clock.
#[derive(Debug, Clone)]
pub struct AsyncSleepFor {
    duration: Duration,
    inner: Option<AsyncSleepUntil>,
}

impl Future for AsyncSleepFor {
    type Output = Result<(), ExecutorError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if matches!(
            current_async_task_scheduler(),
            Some(AsyncTaskSchedulerTag::GreenPool)
        ) {
            self.inner = None;
            return Poll::Ready(Err(ExecutorError::Unsupported));
        }
        if self.inner.is_none() {
            let now = match runtime_monotonic_now_instant() {
                Ok(now) => now,
                Err(error) => return Poll::Ready(Err(error)),
            };
            let deadline = match runtime_monotonic_checked_add(now, self.duration) {
                Ok(deadline) => deadline,
                Err(error) => return Poll::Ready(Err(error)),
            };
            self.inner = Some(async_sleep_until_instant(deadline));
        }
        match self.inner.as_mut() {
            Some(inner) => Pin::new(inner).poll(cx),
            None => Poll::Ready(Err(executor_invalid())),
        }
    }
}

/// Returns one future that resolves after the selected duration on the monotonic clock.
#[must_use]
pub const fn async_sleep_for(duration: Duration) -> AsyncSleepFor {
    AsyncSleepFor {
        duration,
        inner: None,
    }
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
    /// Fixed task-registry capacity admitted by this executor.
    pub capacity: usize,
    /// Sizing strategy applied to executor-owned backing plans.
    pub sizing: RuntimeSizingStrategy,
}

impl ExecutorConfig {
    /// Returns a current-thread executor configuration.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            mode: ExecutorMode::CurrentThread,
            reactor: ReactorConfig::new(),
            capacity: TASK_REGISTRY_CAPACITY,
            sizing: default_runtime_sizing_strategy(),
        }
    }

    /// Returns one thread-pool executor configuration.
    #[must_use]
    pub const fn thread_pool() -> Self {
        Self {
            mode: ExecutorMode::ThreadPool,
            reactor: ReactorConfig::new(),
            capacity: TASK_REGISTRY_CAPACITY,
            sizing: default_runtime_sizing_strategy(),
        }
    }

    /// Returns one fiber-carrier executor configuration.
    #[must_use]
    pub const fn green_pool() -> Self {
        Self {
            mode: ExecutorMode::GreenPool,
            reactor: ReactorConfig::new(),
            capacity: TASK_REGISTRY_CAPACITY,
            sizing: default_runtime_sizing_strategy(),
        }
    }

    /// Returns one copy of this configuration with an explicit execution mode.
    #[must_use]
    pub const fn with_mode(mut self, mode: ExecutorMode) -> Self {
        self.mode = mode;
        self
    }

    /// Returns one copy of this configuration with one explicit task-registry capacity.
    #[must_use]
    pub const fn with_capacity(mut self, capacity: usize) -> Self {
        self.capacity = capacity;
        self
    }

    /// Returns one copy of this configuration with an explicit sizing strategy.
    #[must_use]
    pub const fn with_sizing_strategy(mut self, sizing: RuntimeSizingStrategy) -> Self {
        self.sizing = sizing;
        self
    }
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Backing request for one executor-owned logical storage domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExecutorBackingRequest {
    /// Minimum bytes the backing resource should expose to satisfy the domain honestly from an
    /// arbitrarily aligned base address.
    pub bytes: usize,
    /// Maximum alignment this domain may request from the underlying allocator/pool layer.
    pub align: usize,
}

impl ExecutorBackingRequest {
    fn from_extent_request(
        request: fusion_sys::alloc::MemoryPoolExtentRequest,
    ) -> Result<Self, ExecutorError> {
        Self::from_extent_request_with_layout_policy(
            request,
            AllocatorLayoutPolicy::hosted_vm(
                fusion_pal::sys::mem::system_mem().page_info().alloc_granule,
            ),
        )
    }

    fn from_extent_request_with_layout_policy(
        request: fusion_sys::alloc::MemoryPoolExtentRequest,
        layout_policy: AllocatorLayoutPolicy,
    ) -> Result<Self, ExecutorError> {
        let request = Allocator::<1, 1>::resource_request_for_extent_request_with_layout_policy(
            request,
            layout_policy,
        )
        .map_err(executor_error_from_alloc)?;
        Ok(Self {
            bytes: request.provisioning_len().ok_or_else(executor_overflow)?,
            align: request.align,
        })
    }
}

/// Planning-time layout surface for executor-owned current-thread slabs.
///
/// This exists for the same reason `FiberPlanningSupport` exists: exact build-time slab planning
/// must be able to use target/runtime layout truth without pretending the host binary's `size_of`
/// answers are universal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExecutorPlanningSupport {
    /// Concrete control-block extent bytes for the executor core lease.
    pub control_bytes: usize,
    /// Concrete control-block extent alignment for the executor core lease.
    pub control_align: usize,
    /// Concrete reactor wait-entry bytes.
    pub reactor_wait_entry_bytes: usize,
    /// Concrete reactor wait-entry alignment.
    pub reactor_wait_entry_align: usize,
    /// Concrete reactor outcome-entry bytes.
    pub reactor_outcome_entry_bytes: usize,
    /// Concrete reactor outcome-entry alignment.
    pub reactor_outcome_entry_align: usize,
    /// Concrete current-queue entry bytes.
    pub reactor_queue_entry_bytes: usize,
    /// Concrete current-queue entry alignment.
    pub reactor_queue_entry_align: usize,
    /// Concrete pending-deregister entry bytes.
    pub reactor_pending_entry_bytes: usize,
    /// Concrete pending-deregister entry alignment.
    pub reactor_pending_entry_align: usize,
    /// Concrete registry free-index entry bytes.
    pub registry_free_entry_bytes: usize,
    /// Concrete registry free-index entry alignment.
    pub registry_free_entry_align: usize,
    /// Concrete async task-slot bytes.
    pub registry_slot_bytes: usize,
    /// Concrete async task-slot alignment.
    pub registry_slot_align: usize,
}

impl ExecutorPlanningSupport {
    /// Returns executor planning support for the currently compiled binary.
    #[must_use]
    pub const fn compiled_binary() -> Self {
        let control = match ControlLease::<ExecutorCore>::extent_request() {
            Ok(request) => request,
            Err(_) => fusion_sys::alloc::MemoryPoolExtentRequest { len: 0, align: 1 },
        };
        Self {
            control_bytes: control.len,
            control_align: control.align,
            reactor_wait_entry_bytes: size_of::<AsyncReactorWaitEntry>(),
            reactor_wait_entry_align: align_of::<AsyncReactorWaitEntry>(),
            reactor_outcome_entry_bytes: size_of::<Option<AsyncWaitOutcome>>(),
            reactor_outcome_entry_align: align_of::<Option<AsyncWaitOutcome>>(),
            reactor_queue_entry_bytes: size_of::<Option<CurrentJob>>(),
            reactor_queue_entry_align: align_of::<Option<CurrentJob>>(),
            #[cfg(feature = "std")]
            reactor_pending_entry_bytes: size_of::<Option<EventKey>>(),
            #[cfg(feature = "std")]
            reactor_pending_entry_align: align_of::<Option<EventKey>>(),
            #[cfg(not(feature = "std"))]
            reactor_pending_entry_bytes: 0,
            #[cfg(not(feature = "std"))]
            reactor_pending_entry_align: 1,
            registry_free_entry_bytes: size_of::<usize>(),
            registry_free_entry_align: align_of::<usize>(),
            registry_slot_bytes: size_of::<AsyncTaskSlot>(),
            registry_slot_align: align_of::<AsyncTaskSlot>(),
        }
    }

    /// Returns the truthful Cortex-M no-std executor planning support.
    ///
    /// These values are intentionally explicit so host-side build planning can use target truth
    /// instead of laundering host/std layout through a fake “exact” path. Target Cortex-M builds
    /// validate this descriptor below so it cannot quietly rot.
    #[must_use]
    pub const fn cortex_m() -> Self {
        Self {
            control_bytes: 5016,
            control_align: 8,
            reactor_wait_entry_bytes: 32,
            reactor_wait_entry_align: 8,
            reactor_outcome_entry_bytes: 8,
            reactor_outcome_entry_align: 4,
            reactor_queue_entry_bytes: 24,
            reactor_queue_entry_align: 8,
            reactor_pending_entry_bytes: 0,
            reactor_pending_entry_align: 1,
            registry_free_entry_bytes: 4,
            registry_free_entry_align: 4,
            registry_slot_bytes: 1024,
            registry_slot_align: 64,
        }
    }

    const fn reactor_align(self) -> usize {
        let mut align = self.reactor_wait_entry_align;
        if self.reactor_outcome_entry_align > align {
            align = self.reactor_outcome_entry_align;
        }
        if self.reactor_queue_entry_align > align {
            align = self.reactor_queue_entry_align;
        }
        if self.reactor_pending_entry_align > align {
            align = self.reactor_pending_entry_align;
        }
        align
    }

    fn reactor_capacity(self, capacity: usize) -> Result<usize, ExecutorError> {
        if capacity == 0 {
            return Err(executor_invalid());
        }

        let waits_bytes = self
            .reactor_wait_entry_bytes
            .checked_mul(capacity)
            .ok_or_else(executor_overflow)?;
        let outcomes_bytes = self
            .reactor_outcome_entry_bytes
            .checked_mul(capacity)
            .ok_or_else(executor_overflow)?;
        let queue_bytes = self
            .reactor_queue_entry_bytes
            .checked_mul(capacity)
            .ok_or_else(executor_overflow)?;
        let pending_bytes = self
            .reactor_pending_entry_bytes
            .checked_mul(capacity)
            .ok_or_else(executor_overflow)?;
        let padding = self.reactor_align();
        let segments = if self.reactor_pending_entry_bytes == 0 {
            3
        } else {
            4
        };
        waits_bytes
            .checked_add(outcomes_bytes)
            .and_then(|total| total.checked_add(queue_bytes))
            .and_then(|total| total.checked_add(pending_bytes))
            .and_then(|total| total.checked_add(padding.saturating_mul(segments)))
            .ok_or_else(executor_overflow)
    }

    const fn registry_align(self) -> usize {
        if self.registry_free_entry_align > self.registry_slot_align {
            self.registry_free_entry_align
        } else {
            self.registry_slot_align
        }
    }

    fn registry_capacity(self, capacity: usize) -> Result<usize, ExecutorError> {
        if capacity == 0 {
            return Err(executor_invalid());
        }

        let free_bytes = self
            .registry_free_entry_bytes
            .checked_mul(capacity)
            .ok_or_else(executor_overflow)?;
        let slot_bytes = self
            .registry_slot_bytes
            .checked_mul(capacity)
            .ok_or_else(executor_overflow)?;
        let padding = self.registry_align();
        free_bytes
            .checked_add(slot_bytes)
            .and_then(|total| total.checked_add(padding.saturating_mul(2)))
            .ok_or_else(executor_overflow)
    }
}

#[cfg(all(not(feature = "std"), feature = "sys-cortex-m", target_arch = "arm"))]
const _: [(); 5016] = [(); match ControlLease::<ExecutorCore>::extent_request() {
    Ok(request) => request.len,
    Err(_) => 0,
}];
#[cfg(all(not(feature = "std"), feature = "sys-cortex-m", target_arch = "arm"))]
const _: [(); 8] = [(); match ControlLease::<ExecutorCore>::extent_request() {
    Ok(request) => request.align,
    Err(_) => 0,
}];
#[cfg(all(not(feature = "std"), feature = "sys-cortex-m", target_arch = "arm"))]
const _: [(); 32] = [(); size_of::<AsyncReactorWaitEntry>()];
#[cfg(all(not(feature = "std"), feature = "sys-cortex-m", target_arch = "arm"))]
const _: [(); 8] = [(); align_of::<AsyncReactorWaitEntry>()];
#[cfg(all(not(feature = "std"), feature = "sys-cortex-m", target_arch = "arm"))]
const _: [(); 8] = [(); size_of::<Option<AsyncWaitOutcome>>()];
#[cfg(all(not(feature = "std"), feature = "sys-cortex-m", target_arch = "arm"))]
const _: [(); 4] = [(); align_of::<Option<AsyncWaitOutcome>>()];
#[cfg(all(not(feature = "std"), feature = "sys-cortex-m", target_arch = "arm"))]
const _: [(); 24] = [(); size_of::<Option<CurrentJob>>()];
#[cfg(all(not(feature = "std"), feature = "sys-cortex-m", target_arch = "arm"))]
const _: [(); 8] = [(); align_of::<Option<CurrentJob>>()];
#[cfg(all(not(feature = "std"), feature = "sys-cortex-m", target_arch = "arm"))]
const _: [(); 4] = [(); size_of::<usize>()];
#[cfg(all(not(feature = "std"), feature = "sys-cortex-m", target_arch = "arm"))]
const _: [(); 4] = [(); align_of::<usize>()];
#[cfg(all(not(feature = "std"), feature = "sys-cortex-m", target_arch = "arm"))]
const _: [(); 1024] = [(); size_of::<AsyncTaskSlot>()];
#[cfg(all(not(feature = "std"), feature = "sys-cortex-m", target_arch = "arm"))]
const _: [(); 64] = [(); align_of::<AsyncTaskSlot>()];

/// Compile-time/backing-time footprint plan for one current-thread async runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurrentAsyncRuntimeBackingPlan {
    /// Executor control/state backing.
    pub control: ExecutorBackingRequest,
    /// Reactor bookkeeping backing.
    pub reactor: ExecutorBackingRequest,
    /// Task registry backing.
    pub registry: ExecutorBackingRequest,
    /// Optional medium future-frame slab backing.
    pub future_medium: ExecutorBackingRequest,
    /// Optional large future-frame slab backing.
    pub future_large: ExecutorBackingRequest,
    /// Optional medium result-frame slab backing.
    pub result_medium: ExecutorBackingRequest,
    /// Optional large result-frame slab backing.
    pub result_large: ExecutorBackingRequest,
}

/// Packed one-slab layout for one current-thread async runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurrentAsyncRuntimeCombinedBackingPlan {
    /// Total owning slab request for all current-thread async domains.
    pub slab: ExecutorBackingRequest,
    /// Executor control/state range inside the slab.
    pub control: ResourceRange,
    /// Reactor bookkeeping range inside the slab.
    pub reactor: ResourceRange,
    /// Task registry range inside the slab.
    pub registry: ResourceRange,
    /// Optional medium future slab range inside the slab.
    pub future_medium: Option<ResourceRange>,
    /// Optional large future slab range inside the slab.
    pub future_large: Option<ResourceRange>,
    /// Optional medium result slab range inside the slab.
    pub result_medium: Option<ResourceRange>,
    /// Optional large result slab range inside the slab.
    pub result_large: Option<ResourceRange>,
}

fn executor_align_up_packed(offset: usize, align: usize) -> Result<usize, ExecutorError> {
    if align == 0 || !align.is_power_of_two() {
        return Err(executor_invalid());
    }
    let mask = align - 1;
    offset
        .checked_add(mask)
        .map(|value| value & !mask)
        .ok_or_else(executor_overflow)
}

const fn executor_partition_backing_kind(
    kind: ResourceBackingKind,
) -> Result<ResourceBackingKind, ExecutorError> {
    match kind {
        ResourceBackingKind::Borrowed
        | ResourceBackingKind::StaticRegion
        | ResourceBackingKind::Partition => Ok(ResourceBackingKind::Partition),
        _ => Err(ExecutorError::Unsupported),
    }
}

fn partition_executor_bound_resource(
    handle: &MemoryResourceHandle,
    range: ResourceRange,
) -> Result<MemoryResourceHandle, ExecutorError> {
    let info = match handle {
        MemoryResourceHandle::Bound(resource) => resource.info(),
        MemoryResourceHandle::Virtual(_) => return Err(ExecutorError::Unsupported),
    };
    let region = handle
        .subview(range)
        .map(|view| unsafe { view.raw_region() })
        .map_err(executor_error_from_resource)?;
    let resource = BoundMemoryResource::new(BoundResourceSpec::new(
        region,
        info.domain,
        executor_partition_backing_kind(info.backing)?,
        info.attrs,
        info.geometry,
        info.layout,
        info.contract,
        info.support,
        handle.state(),
    ))
    .map_err(executor_error_from_resource)?;
    Ok(MemoryResourceHandle::from(resource))
}

const fn executor_resource_base_alignment_from_addr(addr: usize) -> usize {
    if addr == 0 {
        1
    } else {
        1usize << addr.trailing_zeros()
    }
}

fn executor_resource_base_alignment(handle: &MemoryResourceHandle) -> usize {
    executor_resource_base_alignment_from_addr(handle.view().base_addr().get())
}

impl CurrentAsyncRuntimeBackingPlan {
    fn combined_with_base_alignment(
        self,
        base_align: usize,
    ) -> Result<CurrentAsyncRuntimeCombinedBackingPlan, ExecutorError> {
        let mut max_align = self.control.align;
        if self.reactor.align > max_align {
            max_align = self.reactor.align;
        }
        if self.registry.align > max_align {
            max_align = self.registry.align;
        }
        if self.future_medium.align > max_align {
            max_align = self.future_medium.align;
        }
        if self.future_large.align > max_align {
            max_align = self.future_large.align;
        }
        if self.result_medium.align > max_align {
            max_align = self.result_medium.align;
        }
        if self.result_large.align > max_align {
            max_align = self.result_large.align;
        }

        let mut cursor = if base_align >= max_align {
            0
        } else {
            max_align.saturating_sub(1)
        };
        let control_offset = executor_align_up_packed(cursor, self.control.align)?;
        cursor = control_offset
            .checked_add(self.control.bytes)
            .ok_or_else(executor_overflow)?;
        let reactor_offset = executor_align_up_packed(cursor, self.reactor.align)?;
        cursor = reactor_offset
            .checked_add(self.reactor.bytes)
            .ok_or_else(executor_overflow)?;
        let registry_offset = executor_align_up_packed(cursor, self.registry.align)?;
        cursor = registry_offset
            .checked_add(self.registry.bytes)
            .ok_or_else(executor_overflow)?;
        let future_medium_offset = executor_align_up_packed(cursor, self.future_medium.align)?;
        cursor = future_medium_offset
            .checked_add(self.future_medium.bytes)
            .ok_or_else(executor_overflow)?;
        let future_large_offset = executor_align_up_packed(cursor, self.future_large.align)?;
        cursor = future_large_offset
            .checked_add(self.future_large.bytes)
            .ok_or_else(executor_overflow)?;
        let result_medium_offset = executor_align_up_packed(cursor, self.result_medium.align)?;
        cursor = result_medium_offset
            .checked_add(self.result_medium.bytes)
            .ok_or_else(executor_overflow)?;
        let result_large_offset = executor_align_up_packed(cursor, self.result_large.align)?;
        let total_bytes = result_large_offset
            .checked_add(self.result_large.bytes)
            .ok_or_else(executor_overflow)?;

        Ok(CurrentAsyncRuntimeCombinedBackingPlan {
            slab: ExecutorBackingRequest {
                bytes: total_bytes,
                align: max_align,
            },
            control: ResourceRange::new(control_offset, self.control.bytes),
            reactor: ResourceRange::new(reactor_offset, self.reactor.bytes),
            registry: ResourceRange::new(registry_offset, self.registry.bytes),
            future_medium: Some(ResourceRange::new(
                future_medium_offset,
                self.future_medium.bytes,
            )),
            future_large: Some(ResourceRange::new(
                future_large_offset,
                self.future_large.bytes,
            )),
            result_medium: Some(ResourceRange::new(
                result_medium_offset,
                self.result_medium.bytes,
            )),
            result_large: Some(ResourceRange::new(
                result_large_offset,
                self.result_large.bytes,
            )),
        })
    }

    fn combined_eager_with_base_alignment(
        self,
        base_align: usize,
    ) -> Result<CurrentAsyncRuntimeCombinedBackingPlan, ExecutorError> {
        let mut max_align = self.control.align;
        if self.reactor.align > max_align {
            max_align = self.reactor.align;
        }
        if self.registry.align > max_align {
            max_align = self.registry.align;
        }

        let mut cursor = if base_align >= max_align {
            0
        } else {
            max_align.saturating_sub(1)
        };
        let control_offset = executor_align_up_packed(cursor, self.control.align)?;
        cursor = control_offset
            .checked_add(self.control.bytes)
            .ok_or_else(executor_overflow)?;
        let reactor_offset = executor_align_up_packed(cursor, self.reactor.align)?;
        cursor = reactor_offset
            .checked_add(self.reactor.bytes)
            .ok_or_else(executor_overflow)?;
        let registry_offset = executor_align_up_packed(cursor, self.registry.align)?;
        let total_bytes = registry_offset
            .checked_add(self.registry.bytes)
            .ok_or_else(executor_overflow)?;

        Ok(CurrentAsyncRuntimeCombinedBackingPlan {
            slab: ExecutorBackingRequest {
                bytes: total_bytes,
                align: max_align,
            },
            control: ResourceRange::new(control_offset, self.control.bytes),
            reactor: ResourceRange::new(reactor_offset, self.reactor.bytes),
            registry: ResourceRange::new(registry_offset, self.registry.bytes),
            future_medium: None,
            future_large: None,
            result_medium: None,
            result_large: None,
        })
    }

    /// Returns the explicit backing plan for one current-thread runtime configuration.
    ///
    /// The medium/large future/result slab requests are part of the plan even though those slabs
    /// still materialize lazily; explicit-backed callers can reserve them up front so runtime
    /// construction never has to improvise another acquisition story later.
    pub fn for_config(config: ExecutorConfig) -> Result<Self, ExecutorError> {
        Self::for_config_with_layout_policy(
            config,
            AllocatorLayoutPolicy::hosted_vm(
                fusion_pal::sys::mem::system_mem().page_info().alloc_granule,
            ),
        )
    }

    /// Returns the explicit backing plan for one current-thread runtime configuration under one
    /// explicit allocator layout policy.
    pub fn for_config_with_layout_policy(
        config: ExecutorConfig,
        layout_policy: AllocatorLayoutPolicy,
    ) -> Result<Self, ExecutorError> {
        Self::for_config_with_layout_policy_and_planning_support(
            config,
            layout_policy,
            ExecutorPlanningSupport::compiled_binary(),
        )
    }

    /// Returns the explicit backing plan for one current-thread runtime configuration under one
    /// explicit allocator layout policy and one explicit executor-planning surface.
    pub fn for_config_with_layout_policy_and_planning_support(
        config: ExecutorConfig,
        layout_policy: AllocatorLayoutPolicy,
        planning: ExecutorPlanningSupport,
    ) -> Result<Self, ExecutorError> {
        let sizing = config.sizing;
        let control = apply_executor_sizing_strategy(
            ExecutorBackingRequest::from_extent_request_with_layout_policy(
                fusion_sys::alloc::MemoryPoolExtentRequest {
                    len: planning.control_bytes,
                    align: planning.control_align,
                },
                layout_policy,
            )?,
            sizing,
        )?;
        let reactor = apply_executor_sizing_strategy(
            ExecutorBackingRequest::from_extent_request_with_layout_policy(
                BoundedArena::<fusion_sys::alloc::Mortal>::extent_request_with_layout_policy(
                    executor_reactor_capacity_with_planning_support(config.capacity, planning)?,
                    executor_reactor_align_with_planning_support(planning),
                    layout_policy,
                )
                .map_err(executor_error_from_alloc)?,
                layout_policy,
            )?,
            sizing,
        )?;
        let registry = apply_executor_sizing_strategy(
            ExecutorBackingRequest::from_extent_request_with_layout_policy(
                BoundedArena::<fusion_sys::alloc::Mortal>::extent_request_with_layout_policy(
                    executor_registry_capacity_with_planning_support(config.capacity, planning)?,
                    executor_registry_align_with_planning_support(planning),
                    layout_policy,
                )
                .map_err(executor_error_from_alloc)?,
                layout_policy,
            )?,
            sizing,
        )?;
        let future_medium = apply_executor_sizing_strategy(
            ExecutorBackingRequest::from_extent_request_with_layout_policy(
                Slab::<ASYNC_FUTURE_CLASS_MEDIUM_BYTES, TASK_REGISTRY_CAPACITY>::extent_request_with_layout_policy(
                    async_storage_class_slot_align(ASYNC_FUTURE_CLASS_MEDIUM_BYTES),
                    layout_policy,
                )
                .map_err(executor_error_from_alloc)?,
                layout_policy,
            )?,
            sizing,
        )?;
        let future_large = apply_executor_sizing_strategy(
            ExecutorBackingRequest::from_extent_request_with_layout_policy(
                Slab::<ASYNC_FUTURE_CLASS_LARGE_BYTES, TASK_REGISTRY_CAPACITY>::extent_request_with_layout_policy(
                    async_storage_class_slot_align(ASYNC_FUTURE_CLASS_LARGE_BYTES),
                    layout_policy,
                )
                .map_err(executor_error_from_alloc)?,
                layout_policy,
            )?,
            sizing,
        )?;
        let result_medium = apply_executor_sizing_strategy(
            ExecutorBackingRequest::from_extent_request_with_layout_policy(
                Slab::<ASYNC_RESULT_CLASS_MEDIUM_BYTES, TASK_REGISTRY_CAPACITY>::extent_request_with_layout_policy(
                    async_storage_class_slot_align(ASYNC_RESULT_CLASS_MEDIUM_BYTES),
                    layout_policy,
                )
                .map_err(executor_error_from_alloc)?,
                layout_policy,
            )?,
            sizing,
        )?;
        let result_large = apply_executor_sizing_strategy(
            ExecutorBackingRequest::from_extent_request_with_layout_policy(
                Slab::<ASYNC_RESULT_CLASS_LARGE_BYTES, TASK_REGISTRY_CAPACITY>::extent_request_with_layout_policy(
                    async_storage_class_slot_align(ASYNC_RESULT_CLASS_LARGE_BYTES),
                    layout_policy,
                )
                .map_err(executor_error_from_alloc)?,
                layout_policy,
            )?,
            sizing,
        )?;

        Ok(Self {
            control,
            reactor,
            registry,
            future_medium,
            future_large,
            result_medium,
            result_large,
        })
    }

    /// Packs the per-domain requests into one conservative owning-slab layout.
    ///
    /// The total byte count includes worst-case padding for an arbitrarily aligned caller-owned
    /// slab base.
    pub fn combined(self) -> Result<CurrentAsyncRuntimeCombinedBackingPlan, ExecutorError> {
        self.combined_with_base_alignment(1)
    }

    /// Packs the per-domain requests into one owning slab for a caller that can guarantee the
    /// slab base is aligned to at least `base_align`.
    pub fn combined_for_base_alignment(
        self,
        base_align: usize,
    ) -> Result<CurrentAsyncRuntimeCombinedBackingPlan, ExecutorError> {
        self.combined_with_base_alignment(base_align)
    }

    /// Packs only the eagerly required current-thread async domains into one owning slab.
    ///
    /// Lazy medium/large future and result slabs stay omitted so explicit-backed bare-metal
    /// runtimes do not reserve those bytes unless the caller deliberately wants them.
    pub fn combined_eager(self) -> Result<CurrentAsyncRuntimeCombinedBackingPlan, ExecutorError> {
        self.combined_eager_with_base_alignment(1)
    }

    /// Packs only the eagerly required domains into one owning slab for a caller that can
    /// guarantee the slab base is aligned to at least `base_align`.
    pub fn combined_eager_for_base_alignment(
        self,
        base_align: usize,
    ) -> Result<CurrentAsyncRuntimeCombinedBackingPlan, ExecutorError> {
        self.combined_eager_with_base_alignment(base_align)
    }
}

fn current_async_runtime_virtual_resource(
    request: ExecutorBackingRequest,
    name: &'static str,
) -> Result<MemoryResourceHandle, ExecutorError> {
    let mut resource_request = ResourceRequest::anonymous_private(request.bytes);
    resource_request.name = Some(name);
    VirtualMemoryResource::create(&resource_request)
        .map(MemoryResourceHandle::from)
        .map_err(executor_error_from_resource)
}

fn current_async_runtime_virtual_backing(
    config: ExecutorConfig,
) -> Result<CurrentAsyncRuntimeBacking, ExecutorError> {
    let plan = CurrentAsyncRuntimeBackingPlan::for_config(config)?;
    Ok(CurrentAsyncRuntimeBacking {
        control: current_async_runtime_virtual_resource(
            plan.control,
            "fusion-executor-current-control",
        )?,
        reactor: current_async_runtime_virtual_resource(
            plan.reactor,
            "fusion-executor-current-reactor",
        )?,
        registry: current_async_runtime_virtual_resource(
            plan.registry,
            "fusion-executor-current-registry",
        )?,
        future_medium: Some(current_async_runtime_virtual_resource(
            plan.future_medium,
            "fusion-executor-current-future-medium",
        )?),
        future_large: Some(current_async_runtime_virtual_resource(
            plan.future_large,
            "fusion-executor-current-future-large",
        )?),
        result_medium: Some(current_async_runtime_virtual_resource(
            plan.result_medium,
            "fusion-executor-current-result-medium",
        )?),
        result_large: Some(current_async_runtime_virtual_resource(
            plan.result_large,
            "fusion-executor-current-result-large",
        )?),
    })
}

/// Explicit backing resources for one current-thread async runtime.
#[derive(Debug)]
pub struct CurrentAsyncRuntimeBacking {
    /// Executor control/state resource.
    pub control: MemoryResourceHandle,
    /// Reactor bookkeeping resource.
    pub reactor: MemoryResourceHandle,
    /// Task registry resource.
    pub registry: MemoryResourceHandle,
    /// Medium future-frame slab resource.
    pub future_medium: Option<MemoryResourceHandle>,
    /// Large future-frame slab resource.
    pub future_large: Option<MemoryResourceHandle>,
    /// Medium result-frame slab resource.
    pub result_medium: Option<MemoryResourceHandle>,
    /// Large result-frame slab resource.
    pub result_large: Option<MemoryResourceHandle>,
}

/// Public executor error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExecutorError {
    /// The requested executor mode is unsupported or not implemented for this binding.
    Unsupported,
    /// The underlying scheduler has stopped accepting work.
    Stopped,
    /// The task was explicitly cancelled before completion.
    Cancelled,
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
const ASYNC_FUTURE_CLASS_MEDIUM_BYTES: usize = 512;
const ASYNC_FUTURE_CLASS_LARGE_BYTES: usize = 1024;
const INLINE_ASYNC_RESULT_BYTES: usize = 256;
const ASYNC_RESULT_CLASS_MEDIUM_BYTES: usize = 512;
const ASYNC_RESULT_CLASS_LARGE_BYTES: usize = 1024;
const REACTOR_EVENT_BATCH: usize = 16;

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

const EMPTY_EVENT_RECORD: EventRecord = EventRecord {
    key: EventKey(0),
    notification: EventNotification::Readiness(EventReadiness::empty()),
};

struct ExecutorCell<T> {
    fast: bool,
    value: UnsafeCell<T>,
    lock: SysMutex<()>,
}

unsafe impl<T: Send> Send for ExecutorCell<T> {}
unsafe impl<T: Send> Sync for ExecutorCell<T> {}

impl<T: fmt::Debug> fmt::Debug for ExecutorCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecutorCell")
            .field("fast", &self.fast)
            .finish_non_exhaustive()
    }
}

impl<T> ExecutorCell<T> {
    const fn new(fast: bool, value: T) -> Self {
        Self {
            fast,
            value: UnsafeCell::new(value),
            lock: SysMutex::new(()),
        }
    }

    fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> Result<R, ExecutorError> {
        if self.fast {
            // SAFETY: fast-mode cells are only installed by the thread-affine current runtime.
            return Ok(unsafe { f(&mut *self.value.get()) });
        }
        let _guard = self.lock.lock().map_err(executor_error_from_sync)?;
        // SAFETY: the lock serializes mutable access in shared modes.
        Ok(unsafe { f(&mut *self.value.get()) })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn with_ref<R>(&self, f: impl FnOnce(&T) -> R) -> Result<R, ExecutorError> {
        if self.fast {
            // SAFETY: fast-mode cells are only installed by the thread-affine current runtime.
            return Ok(unsafe { f(&*self.value.get()) });
        }
        let _guard = self.lock.lock().map_err(executor_error_from_sync)?;
        // SAFETY: the lock serializes shared access in shared modes.
        Ok(unsafe { f(&*self.value.get()) })
    }
}

struct CurrentQueue {
    ready: ExecutorCell<CurrentQueueState>,
}

struct ExecutorReactorState {
    poller: ExecutorCell<Option<ReactorPoller>>,
    events: ExecutorCell<[EventRecord; REACTOR_EVENT_BATCH]>,
    waits: ExecutorCell<ArenaSlice<AsyncReactorWaitEntry>>,
    outcomes: ExecutorCell<ArenaSlice<Option<AsyncWaitOutcome>>>,
    #[cfg(feature = "std")]
    pending_deregister: ExecutorCell<ArenaSlice<Option<EventKey>>>,
    #[cfg(feature = "std")]
    wake: ExecutorCell<Option<ExecutorReactorWakeSignal>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum AsyncReactorWaitKind {
    None,
    #[cfg(feature = "std")]
    ReadinessPending {
        generation: u64,
        source: EventSourceHandle,
        interest: EventInterest,
    },
    ReadinessRegistered {
        generation: u64,
        key: EventKey,
    },
    Sleep {
        generation: u64,
        deadline: CanonicalInstant,
        raw_deadline: Option<MonotonicRawInstant>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct AsyncReactorWaitEntry {
    kind: AsyncReactorWaitKind,
}

impl AsyncReactorWaitEntry {
    const EMPTY: Self = Self {
        kind: AsyncReactorWaitKind::None,
    };

    const fn readiness(generation: u64, key: EventKey) -> Self {
        Self {
            kind: AsyncReactorWaitKind::ReadinessRegistered { generation, key },
        }
    }

    #[cfg(feature = "std")]
    const fn readiness_pending(
        generation: u64,
        source: EventSourceHandle,
        interest: EventInterest,
    ) -> Self {
        Self {
            kind: AsyncReactorWaitKind::ReadinessPending {
                generation,
                source,
                interest,
            },
        }
    }

    const fn sleep(
        generation: u64,
        deadline: CanonicalInstant,
        raw_deadline: Option<MonotonicRawInstant>,
    ) -> Self {
        Self {
            kind: AsyncReactorWaitKind::Sleep {
                generation,
                deadline,
                raw_deadline,
            },
        }
    }
}

#[cfg(feature = "std")]
struct ExecutorReactorWakeSignal {
    signal: PlatformFiberWakeSignal,
    key: Option<EventKey>,
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
    entries: ArenaSlice<Option<CurrentJob>>,
    head: usize,
    tail: usize,
    len: usize,
}

#[derive(Debug)]
struct HostedReadyQueueState {
    entries: [Option<CurrentJob>; CURRENT_QUEUE_CAPACITY],
    head: usize,
    tail: usize,
    len: usize,
}
impl CurrentQueue {
    fn new_in(arena: &BoundedArena, capacity: usize, fast: bool) -> Result<Self, ExecutorError> {
        let entries = arena
            .alloc_array_with(capacity.max(1), |_| None)
            .map_err(executor_error_from_alloc)?;
        Ok(Self {
            ready: ExecutorCell::new(
                fast,
                CurrentQueueState {
                    entries,
                    head: 0,
                    tail: 0,
                    len: 0,
                },
            ),
        })
    }

    fn schedule_slot(
        &self,
        core: &ExecutorCore,
        slot_index: usize,
        generation: u64,
    ) -> Result<(), ExecutorError> {
        self.ready.with(|ready| {
            ready.enqueue(CurrentJob {
                run: run_current_slot,
                core: core::ptr::from_ref(core) as usize,
                slot_index,
                generation,
            })
        })?
    }

    fn run_next(&self) -> Result<bool, ExecutorError> {
        let job = self.ready.with(CurrentQueueState::dequeue)?;
        if let Some(job) = job {
            unsafe {
                (job.run)(job.core, job.slot_index, job.generation);
            }
            return Ok(true);
        }
        Ok(false)
    }
}

impl ExecutorReactorState {
    fn new(
        capacity: usize,
        fast: bool,
        allocator: &ExecutorDomainAllocator,
    ) -> Result<(Self, CurrentQueue), ExecutorError> {
        let arena_capacity = executor_reactor_capacity(capacity)?;
        let arena = allocator.arena(arena_capacity, executor_reactor_align())?;
        let current_queue = CurrentQueue::new_in(&arena, capacity, fast)?;
        let waits = arena
            .alloc_array_with(capacity, |_| AsyncReactorWaitEntry::EMPTY)
            .map_err(executor_error_from_alloc)?;
        let outcomes = arena
            .alloc_array_with(capacity, |_| None)
            .map_err(executor_error_from_alloc)?;
        #[cfg(feature = "std")]
        let pending_deregister = arena
            .alloc_array_with(capacity, |_| None)
            .map_err(executor_error_from_alloc)?;

        Ok((
            Self {
                poller: ExecutorCell::new(fast, None),
                events: ExecutorCell::new(fast, [EMPTY_EVENT_RECORD; REACTOR_EVENT_BATCH]),
                waits: ExecutorCell::new(fast, waits),
                outcomes: ExecutorCell::new(fast, outcomes),
                #[cfg(feature = "std")]
                pending_deregister: ExecutorCell::new(fast, pending_deregister),
                #[cfg(feature = "std")]
                wake: ExecutorCell::new(fast, None),
            },
            current_queue,
        ))
    }

    #[cfg(feature = "std")]
    fn install_driver_wake_signal(&self) -> Result<(), ExecutorError> {
        let host = system_fiber_host();
        if self.wake.with_ref(Option::is_some)? {
            return Ok(());
        }
        let signal = host
            .create_wake_signal()
            .map_err(executor_error_from_fiber_host)?;
        self.wake.with(|wake| {
            if wake.is_none() {
                *wake = Some(ExecutorReactorWakeSignal { signal, key: None });
            }
        })?;
        Ok(())
    }

    #[cfg(feature = "std")]
    fn signal_driver(&self) -> Result<(), ExecutorError> {
        let Some(()) = self.wake.with_ref(|wake| wake.as_ref().map(|_| ()))? else {
            return Ok(());
        };
        self.wake.with_ref(|wake| {
            if let Some(wake) = wake.as_ref() {
                wake.signal.signal().map_err(executor_error_from_fiber_host)
            } else {
                Ok(())
            }
        })??;
        Ok(())
    }

    fn ensure_poller(&self, reactor: Reactor) -> Result<bool, ExecutorError> {
        self.poller.with(|poller_slot| {
            if poller_slot.is_some() {
                return Ok(true);
            }
            match reactor.create() {
                Ok(poller) => {
                    *poller_slot = Some(poller);
                    Ok(true)
                }
                Err(error) if error.kind() == EventErrorKind::Unsupported => Ok(false),
                Err(error) => Err(executor_error_from_event(error)),
            }
        })?
    }

    #[cfg(feature = "std")]
    fn ensure_wake_registration(&self, reactor: Reactor) -> Result<bool, ExecutorError> {
        if !self.ensure_poller(reactor)? {
            return Ok(false);
        }
        let Some(()) = self.wake.with_ref(|wake| wake.as_ref().map(|_| ()))? else {
            return Ok(true);
        };
        let already_registered = self
            .wake
            .with_ref(|wake| wake.as_ref().and_then(|wake| wake.key).is_some())?;
        if already_registered {
            return Ok(true);
        }

        let source = self.wake.with_ref(|wake| {
            wake.as_ref()
                .ok_or(ExecutorError::Stopped)?
                .signal
                .source_handle()
                .map(EventSourceHandle)
                .map_err(executor_error_from_fiber_host)
        })??;
        let key = self.poller.with(|poller_slot| {
            let poller = poller_slot.as_mut().ok_or(ExecutorError::Stopped)?;
            reactor
                .register(
                    poller,
                    source,
                    EventInterest::READABLE | EventInterest::ERROR | EventInterest::HANGUP,
                )
                .map_err(executor_error_from_event)
        })??;
        self.wake.with(|wake| {
            if let Some(wake) = wake.as_mut() {
                wake.key = Some(key);
            }
        })?;
        Ok(true)
    }

    fn register_readiness_wait(
        &self,
        reactor: Reactor,
        slot_index: usize,
        generation: u64,
        source: EventSourceHandle,
        interest: EventInterest,
    ) -> Result<(), ExecutorError> {
        #[cfg(feature = "std")]
        self.ensure_wake_registration(reactor)?;
        #[cfg(not(feature = "std"))]
        if !self.ensure_poller(reactor)? {
            return Err(ExecutorError::Unsupported);
        }

        let key = self.poller.with(|poller_slot| {
            let poller = poller_slot.as_mut().ok_or(ExecutorError::Unsupported)?;
            reactor
                .register(
                    poller,
                    source,
                    interest | EventInterest::ERROR | EventInterest::HANGUP,
                )
                .map_err(executor_error_from_event)
        })??;
        self.waits.with(|waits| {
            waits[slot_index] = AsyncReactorWaitEntry::readiness(generation, key);
        })?;
        self.outcomes.with(|outcomes| outcomes[slot_index] = None)?;
        #[cfg(feature = "std")]
        self.signal_driver()?;
        Ok(())
    }

    #[cfg(feature = "std")]
    fn queue_readiness_wait(
        &self,
        slot_index: usize,
        generation: u64,
        source: EventSourceHandle,
        interest: EventInterest,
    ) -> Result<(), ExecutorError> {
        self.waits.with(|waits| {
            waits[slot_index] =
                AsyncReactorWaitEntry::readiness_pending(generation, source, interest);
        })?;
        self.outcomes.with(|outcomes| outcomes[slot_index] = None)?;
        self.signal_driver()?;
        Ok(())
    }

    fn register_sleep_wait(
        &self,
        slot_index: usize,
        generation: u64,
        deadline: CanonicalInstant,
    ) -> Result<(), ExecutorError> {
        let raw_deadline = system_monotonic_time()
            .raw_deadline_for_sleep(deadline)
            .map_err(executor_error_from_thread)?;
        self.waits.with(|waits| {
            waits[slot_index] = AsyncReactorWaitEntry::sleep(generation, deadline, raw_deadline);
        })?;
        self.outcomes.with(|outcomes| outcomes[slot_index] = None)?;
        #[cfg(feature = "std")]
        self.signal_driver()?;
        Ok(())
    }

    fn clear_wait(
        &self,
        reactor: Reactor,
        slot_index: usize,
        generation: u64,
    ) -> Result<(), ExecutorError> {
        let removed = self.waits.with(|waits| {
            let entry = waits[slot_index];
            match entry.kind {
                AsyncReactorWaitKind::ReadinessRegistered {
                    generation: live_generation,
                    key,
                } if live_generation == generation => {
                    waits[slot_index] = AsyncReactorWaitEntry::EMPTY;
                    Some(key)
                }
                #[cfg(feature = "std")]
                AsyncReactorWaitKind::ReadinessPending {
                    generation: live_generation,
                    ..
                } if live_generation == generation => {
                    waits[slot_index] = AsyncReactorWaitEntry::EMPTY;
                    None
                }
                AsyncReactorWaitKind::Sleep {
                    generation: live_generation,
                    ..
                } if live_generation == generation => {
                    waits[slot_index] = AsyncReactorWaitEntry::EMPTY;
                    None
                }
                _ => None,
            }
        })?;
        if let Some(key) = removed {
            self.best_effort_deregister(reactor, key)?;
        }
        self.outcomes.with(|outcomes| outcomes[slot_index] = None)?;
        #[cfg(feature = "std")]
        self.signal_driver()?;
        Ok(())
    }

    #[cfg(feature = "std")]
    fn clear_wait_deferred(&self, slot_index: usize, generation: u64) -> Result<(), ExecutorError> {
        let removed = self.waits.with(|waits| {
            let entry = waits[slot_index];
            match entry.kind {
                AsyncReactorWaitKind::ReadinessPending {
                    generation: live_generation,
                    ..
                } if live_generation == generation => {
                    waits[slot_index] = AsyncReactorWaitEntry::EMPTY;
                    None
                }
                AsyncReactorWaitKind::ReadinessRegistered {
                    generation: live_generation,
                    key,
                } if live_generation == generation => {
                    waits[slot_index] = AsyncReactorWaitEntry::EMPTY;
                    Some(key)
                }
                AsyncReactorWaitKind::Sleep {
                    generation: live_generation,
                    ..
                } if live_generation == generation => {
                    waits[slot_index] = AsyncReactorWaitEntry::EMPTY;
                    None
                }
                AsyncReactorWaitKind::None => None,
                _ => None,
            }
        })?;
        if let Some(key) = removed {
            self.pending_deregister.with(|pending| {
                let Some(entry) = pending.iter_mut().find(|entry| entry.is_none()) else {
                    return Err(executor_overflow());
                };
                *entry = Some(key);
                Ok::<(), ExecutorError>(())
            })??;
        }
        self.outcomes.with(|outcomes| outcomes[slot_index] = None)?;
        self.signal_driver()?;
        Ok(())
    }

    fn store_wait_outcome(
        &self,
        slot_index: usize,
        outcome: AsyncWaitOutcome,
    ) -> Result<(), ExecutorError> {
        self.outcomes
            .with(|outcomes| outcomes[slot_index] = Some(outcome))
    }

    fn take_wait_outcome(
        &self,
        slot_index: usize,
    ) -> Result<Option<AsyncWaitOutcome>, ExecutorError> {
        self.outcomes.with(|outcomes| outcomes[slot_index].take())
    }

    fn next_timer_deadline(&self) -> Result<Option<CanonicalInstant>, ExecutorError> {
        self.waits.with_ref(|waits| {
            waits.iter().fold(
                None::<CanonicalInstant>,
                |next_deadline, entry| match entry.kind {
                    AsyncReactorWaitKind::Sleep { deadline, .. } => Some(match next_deadline {
                        Some(current) => current.min(deadline),
                        None => deadline,
                    }),
                    _ => next_deadline,
                },
            )
        })
    }

    fn has_readiness_waiters(&self) -> Result<bool, ExecutorError> {
        self.waits.with_ref(|waits| {
            waits.iter().any(|entry| match entry.kind {
                #[cfg(feature = "std")]
                AsyncReactorWaitKind::ReadinessPending { .. } => true,
                AsyncReactorWaitKind::ReadinessRegistered { .. } => true,
                AsyncReactorWaitKind::None | AsyncReactorWaitKind::Sleep { .. } => false,
            })
        })
    }

    #[cfg(feature = "std")]
    fn flush_pending_deregistrations(&self, reactor: Reactor) -> Result<(), ExecutorError> {
        loop {
            let key = self.pending_deregister.with(|queue| {
                Ok::<Option<EventKey>, ExecutorError>(
                    queue.iter_mut().find_map(|entry| entry.take()),
                )
            })??;
            let Some(key) = key else {
                break;
            };
            self.best_effort_deregister(reactor, key)?;
        }
        Ok(())
    }

    #[cfg(feature = "std")]
    fn activate_pending_readiness_waits(
        &self,
        core: &ExecutorCore,
        reactor: Reactor,
    ) -> Result<bool, ExecutorError> {
        let mut progressed = false;
        let slot_count = self.waits.with_ref(|waits| waits.len())?;
        for slot_index in 0..slot_count {
            let pending = self.waits.with_ref(|waits| match waits[slot_index].kind {
                AsyncReactorWaitKind::ReadinessPending {
                    generation,
                    source,
                    interest,
                } => Some((generation, source, interest)),
                _ => None,
            })?;
            let Some((generation, source, interest)) = pending else {
                continue;
            };

            let key = match self.poller.with(|poller_slot| {
                let poller = poller_slot.as_mut().ok_or(ExecutorError::Unsupported)?;
                reactor
                    .register(
                        poller,
                        source,
                        interest | EventInterest::ERROR | EventInterest::HANGUP,
                    )
                    .map_err(executor_error_from_event)
            })? {
                Ok(key) => key,
                Err(error) => {
                    self.waits.with(|waits| {
                        if matches!(
                            waits[slot_index].kind,
                            AsyncReactorWaitKind::ReadinessPending {
                                generation: live_generation,
                                ..
                            } if live_generation == generation
                        ) {
                            waits[slot_index] = AsyncReactorWaitEntry::EMPTY;
                        }
                    })?;
                    self.store_wait_outcome(slot_index, AsyncWaitOutcome::Error(error))?;
                    core.schedule_slot(slot_index, generation)?;
                    progressed = true;
                    continue;
                }
            };

            self.waits.with(|waits| {
                if matches!(
                    waits[slot_index].kind,
                    AsyncReactorWaitKind::ReadinessPending {
                        generation: live_generation,
                        ..
                    } if live_generation == generation
                ) {
                    waits[slot_index] = AsyncReactorWaitEntry::readiness(generation, key);
                }
            })?;
        }
        Ok(progressed)
    }

    fn collect_due_timers(
        &self,
        core: &ExecutorCore,
        now: CanonicalInstant,
        now_raw: Option<MonotonicRawInstant>,
    ) -> Result<bool, ExecutorError> {
        let mut progressed = false;
        let slot_count = self.waits.with_ref(|waits| waits.len())?;
        for slot_index in 0..slot_count {
            let generation = self.waits.with(|waits| {
                let AsyncReactorWaitKind::Sleep {
                    generation,
                    deadline,
                    raw_deadline,
                } = waits[slot_index].kind
                else {
                    return Ok::<Option<u64>, ExecutorError>(None);
                };
                let due = match (now_raw, raw_deadline) {
                    (Some(now_raw), Some(raw_deadline)) => now_raw.deadline_reached(raw_deadline),
                    _ => now >= deadline,
                };
                if !due {
                    return Ok(None);
                }
                waits[slot_index] = AsyncReactorWaitEntry::EMPTY;
                Ok(Some(generation))
            })??;
            let Some(generation) = generation else {
                continue;
            };
            self.store_wait_outcome(slot_index, AsyncWaitOutcome::Timer)?;
            core.schedule_slot(slot_index, generation)?;
            progressed = true;
        }
        Ok(progressed)
    }

    fn resolve_reactor_events(
        &self,
        core: &ExecutorCore,
        reactor: Reactor,
        count: usize,
    ) -> Result<bool, ExecutorError> {
        if count == 0 {
            return Ok(false);
        }
        #[cfg(feature = "std")]
        let mut wake_event = false;
        #[cfg(feature = "std")]
        let wake_key = self
            .wake
            .with_ref(|wake| wake.as_ref().and_then(|wake| wake.key))?;

        let mut progressed = false;
        for event_index in 0..count {
            let event = self.events.with_ref(|events| events[event_index])?;
            #[cfg(feature = "std")]
            if Some(event.key) == wake_key {
                wake_event = true;
                continue;
            }
            let EventNotification::Readiness(readiness) = event.notification else {
                continue;
            };
            let ready = self.waits.with(|waits| {
                for (slot_index, entry) in waits.iter_mut().enumerate() {
                    let AsyncReactorWaitKind::ReadinessRegistered { generation, key } = entry.kind
                    else {
                        continue;
                    };
                    if key != event.key {
                        continue;
                    }
                    entry.kind = AsyncReactorWaitKind::None;
                    return Ok::<Option<(usize, u64)>, ExecutorError>(Some((
                        slot_index, generation,
                    )));
                }
                Ok(None)
            })??;
            let Some((slot_index, generation)) = ready else {
                continue;
            };
            self.best_effort_deregister(reactor, event.key)?;
            self.store_wait_outcome(slot_index, AsyncWaitOutcome::Readiness(readiness))?;
            core.schedule_slot(slot_index, generation)?;
            progressed = true;
        }

        #[cfg(feature = "std")]
        if wake_event {
            self.wake.with_ref(|wake| {
                if let Some(wake) = wake.as_ref() {
                    wake.signal.drain().map_err(executor_error_from_fiber_host)
                } else {
                    Ok(())
                }
            })??;
        }
        Ok(progressed)
    }

    fn best_effort_deregister(&self, reactor: Reactor, key: EventKey) -> Result<(), ExecutorError> {
        if !self.ensure_poller(reactor)? {
            return Ok(());
        }
        let result = self.poller.with(|poller_slot| {
            let Some(poller) = poller_slot.as_mut() else {
                return Ok(());
            };
            match reactor.deregister(poller, key) {
                Ok(()) => Ok(()),
                Err(error)
                    if matches!(
                        error.kind(),
                        EventErrorKind::Invalid | EventErrorKind::StateConflict
                    ) =>
                {
                    Ok(())
                }
                Err(error) => Err(executor_error_from_event(error)),
            }
        })?;
        result
    }

    fn drive(
        &self,
        core: &ExecutorCore,
        blocking: bool,
        max_events: Option<usize>,
    ) -> Result<bool, ExecutorError> {
        let mut progressed = false;
        #[cfg(feature = "std")]
        if blocking {
            self.ensure_wake_registration(core.reactor)?;
        }
        if !self.ensure_poller(core.reactor)? {
            return Ok(progressed);
        }
        #[cfg(feature = "std")]
        {
            self.flush_pending_deregistrations(core.reactor)?;
            progressed |= self.activate_pending_readiness_waits(core, core.reactor)?;
        }
        let now = if self.next_timer_deadline()?.is_some() {
            Some(runtime_monotonic_now_instant()?)
        } else {
            None
        };
        if let Some(now) = now {
            let now_raw = runtime_monotonic_raw_now().ok();
            progressed |= self.collect_due_timers(core, now, now_raw)?;
        }

        let has_readiness_waiters = self.has_readiness_waiters()?;
        let next_deadline = self.next_timer_deadline()?;
        let should_poll = has_readiness_waiters || (blocking && next_deadline.is_some());
        if !should_poll {
            return Ok(progressed);
        }

        if blocking
            && !has_readiness_waiters
            && let Some(deadline) = next_deadline
        {
            system_monotonic_time()
                .sleep_until(deadline)
                .map_err(executor_error_from_thread)?;
            let now = runtime_monotonic_now_instant()?;
            let now_raw = runtime_monotonic_raw_now().ok();
            progressed |= self.collect_due_timers(core, now, now_raw)?;
            return Ok(progressed);
        }

        let timeout = if blocking {
            match next_deadline {
                Some(deadline) => Some(runtime_monotonic_duration_until(deadline)?),
                None => None,
            }
        } else {
            Some(Duration::from_millis(0))
        };

        let count = self.poller.with(|poller_slot| {
            let Some(poller) = poller_slot.as_mut() else {
                return Ok(0);
            };
            self.events.with(|events| {
                let limit = max_events.unwrap_or(events.len()).min(events.len());
                core.reactor
                    .poll(poller, &mut events[..limit], timeout)
                    .map_err(executor_error_from_event)
            })?
        })??;
        progressed |= self.resolve_reactor_events(core, core.reactor, count)?;

        if self.next_timer_deadline()?.is_some() {
            let now = runtime_monotonic_now_instant()?;
            let now_raw = runtime_monotonic_raw_now().ok();
            progressed |= self.collect_due_timers(core, now, now_raw)?;
        }
        Ok(progressed)
    }
}

impl fmt::Debug for CurrentQueue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CurrentQueue").finish_non_exhaustive()
    }
}

impl CurrentQueueState {
    fn enqueue(&mut self, job: CurrentJob) -> Result<(), ExecutorError> {
        if self.len == self.entries.len() {
            return Err(executor_overflow());
        }
        self.entries[self.tail] = Some(job);
        self.tail = (self.tail + 1) % self.entries.len();
        self.len += 1;
        Ok(())
    }

    fn dequeue(&mut self) -> Option<CurrentJob> {
        if self.len == 0 {
            return None;
        }
        let job = self.entries[self.head].take();
        self.head = (self.head + 1) % self.entries.len();
        self.len -= 1;
        job
    }
}

impl HostedReadyQueueState {
    const fn new() -> Self {
        Self {
            entries: [None; CURRENT_QUEUE_CAPACITY],
            head: 0,
            tail: 0,
            len: 0,
        }
    }

    fn enqueue(&mut self, job: CurrentJob) -> Result<(), ExecutorError> {
        if self.len == self.entries.len() {
            return Err(executor_overflow());
        }
        self.entries[self.tail] = Some(job);
        self.tail = (self.tail + 1) % self.entries.len();
        self.len += 1;
        Ok(())
    }

    fn dequeue(&mut self) -> Option<CurrentJob> {
        if self.len == 0 {
            return None;
        }
        let job = self.entries[self.head].take();
        self.head = (self.head + 1) % self.entries.len();
        self.len -= 1;
        job
    }

    #[allow(dead_code)]
    fn clear(&mut self) -> usize {
        let dropped = self.len;
        while self.dequeue().is_some() {}
        dropped
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

type InlineAsyncFutureBytes = CachePadded<[u8; INLINE_ASYNC_FUTURE_BYTES]>;

type InlineAsyncPollFn = unsafe fn(
    *mut u8,
    &ExecutorCell<InlineAsyncResultStorage>,
    &AsyncTaskResultStore,
    &mut Context<'_>,
) -> Result<Poll<()>, ExecutorError>;

struct InlineAsyncFutureStorage {
    storage: MaybeUninit<InlineAsyncFutureBytes>,
    allocation: Option<AsyncFutureFrameAllocation>,
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
            allocation: None,
            poll: None,
            drop: None,
            occupied: false,
        }
    }

    const fn supports_inline<F>() -> bool
    where
        F: Future + 'static,
    {
        size_of::<F>() <= size_of::<InlineAsyncFutureBytes>()
            && align_of::<F>() <= align_of::<InlineAsyncFutureBytes>()
    }

    fn store_future<F>(
        &mut self,
        future_store: &AsyncTaskFutureStore,
        result_store: &AsyncTaskResultStore,
        future: F,
    ) -> Result<(), ExecutorError>
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        if self.occupied {
            return Err(executor_invalid());
        }
        if !InlineAsyncResultStorage::supports::<F::Output>(result_store) {
            return Err(ExecutorError::Unsupported);
        }

        let target = if Self::supports_inline::<F>() {
            self.storage.as_mut_ptr().cast::<F>()
        } else {
            let mut allocation = future_store.allocate_for::<F>()?;
            let ptr = allocation.ptr().cast::<F>();
            self.allocation = Some(allocation);
            ptr
        };
        unsafe {
            target.write(future);
        }
        self.poll = Some(poll_inline_async_future::<F>);
        self.drop = Some(drop_inline_async_value::<F>);
        self.occupied = true;
        Ok(())
    }

    fn poll_in_place(
        &mut self,
        result: &ExecutorCell<InlineAsyncResultStorage>,
        result_store: &AsyncTaskResultStore,
        context: &mut Context<'_>,
    ) -> Result<Poll<()>, ExecutorError> {
        if !self.occupied {
            return Err(executor_invalid());
        }
        let poll = self.poll.ok_or_else(executor_invalid)?;
        unsafe { poll(self.storage_ptr(), result, result_store, context) }
    }

    fn clear(&mut self, future_store: &AsyncTaskFutureStore) -> Result<(), ExecutorError> {
        self.drop_value_only();
        if let Some(allocation) = self.allocation.take() {
            future_store.deallocate(allocation)?;
        }
        self.poll = None;
        Ok(())
    }

    fn storage_ptr(&mut self) -> *mut u8 {
        match self.allocation.as_mut() {
            Some(allocation) => allocation.ptr(),
            None => self.storage.as_mut_ptr().cast::<u8>(),
        }
    }

    fn drop_value_only(&mut self) {
        if !self.occupied {
            self.poll = None;
            self.drop = None;
            self.allocation = None;
            return;
        }

        if let Some(drop) = self.drop.take() {
            unsafe {
                drop(self.storage_ptr());
            }
        }
        self.poll = None;
        self.occupied = false;
    }
}

impl Drop for InlineAsyncFutureStorage {
    fn drop(&mut self) {
        self.drop_value_only();
    }
}

#[derive(Debug)]
struct AsyncTaskFutureStore {
    medium_allocator: Option<ExecutorDomainAllocator>,
    medium: ExecutorCell<Option<Slab<ASYNC_FUTURE_CLASS_MEDIUM_BYTES, TASK_REGISTRY_CAPACITY>>>,
    large_allocator: Option<ExecutorDomainAllocator>,
    large: ExecutorCell<Option<Slab<ASYNC_FUTURE_CLASS_LARGE_BYTES, TASK_REGISTRY_CAPACITY>>>,
}

impl AsyncTaskFutureStore {
    fn new(
        fast: bool,
        medium_allocator: Option<ExecutorDomainAllocator>,
        large_allocator: Option<ExecutorDomainAllocator>,
    ) -> Self {
        Self {
            medium_allocator,
            medium: ExecutorCell::new(fast, None),
            large_allocator,
            large: ExecutorCell::new(fast, None),
        }
    }

    fn allocate_for<F>(&self) -> Result<AsyncFutureFrameAllocation, ExecutorError>
    where
        F: Future + 'static,
    {
        let len = size_of::<F>();
        let align = align_of::<F>();
        let request = fusion_sys::alloc::AllocRequest {
            len,
            align,
            zeroed: false,
        };
        if len <= ASYNC_FUTURE_CLASS_MEDIUM_BYTES
            && align <= async_storage_class_slot_align(ASYNC_FUTURE_CLASS_MEDIUM_BYTES)
        {
            return self.medium.with(|medium| {
                if medium.is_none() {
                    *medium = Some(
                        self.medium_allocator
                            .as_ref()
                            .ok_or(ExecutorError::Unsupported)?
                            .slab::<ASYNC_FUTURE_CLASS_MEDIUM_BYTES, TASK_REGISTRY_CAPACITY>()?,
                    );
                }
                medium
                    .as_ref()
                    .ok_or_else(executor_invalid)?
                    .allocate(&request)
                    .map(AsyncFutureFrameAllocation::Medium)
                    .map_err(executor_error_from_alloc)
            })?;
        }
        if len <= ASYNC_FUTURE_CLASS_LARGE_BYTES
            && align <= async_storage_class_slot_align(ASYNC_FUTURE_CLASS_LARGE_BYTES)
        {
            return self.large.with(|large| {
                if large.is_none() {
                    *large = Some(
                        self.large_allocator
                            .as_ref()
                            .ok_or(ExecutorError::Unsupported)?
                            .slab::<ASYNC_FUTURE_CLASS_LARGE_BYTES, TASK_REGISTRY_CAPACITY>()?,
                    );
                }
                large
                    .as_ref()
                    .ok_or_else(executor_invalid)?
                    .allocate(&request)
                    .map(AsyncFutureFrameAllocation::Large)
                    .map_err(executor_error_from_alloc)
            })?;
        }
        Err(ExecutorError::Unsupported)
    }

    fn deallocate(&self, allocation: AsyncFutureFrameAllocation) -> Result<(), ExecutorError> {
        match allocation {
            AsyncFutureFrameAllocation::Medium(allocation) => self.medium.with(|medium| {
                medium
                    .as_ref()
                    .ok_or_else(executor_invalid)?
                    .deallocate(allocation)
                    .map_err(executor_error_from_alloc)
            })?,
            AsyncFutureFrameAllocation::Large(allocation) => self.large.with(|large| {
                large
                    .as_ref()
                    .ok_or_else(executor_invalid)?
                    .deallocate(allocation)
                    .map_err(executor_error_from_alloc)
            })?,
        }
    }
}

#[derive(Debug)]
enum AsyncFutureFrameAllocation {
    Medium(fusion_sys::alloc::AllocResult),
    Large(fusion_sys::alloc::AllocResult),
}

impl AsyncFutureFrameAllocation {
    fn ptr(&mut self) -> *mut u8 {
        match self {
            Self::Medium(allocation) | Self::Large(allocation) => allocation.ptr.as_ptr(),
        }
    }
}

// SAFETY: frame allocations are owned linearly by one task slot, and moving the allocation token
// between scheduler threads does not invalidate the underlying slab-backed storage.
unsafe impl Send for AsyncFutureFrameAllocation {}
// SAFETY: shared references do not permit mutation; slot-level synchronization still governs use.
unsafe impl Sync for AsyncFutureFrameAllocation {}

type InlineAsyncResultBytes = CachePadded<[u8; INLINE_ASYNC_RESULT_BYTES]>;

struct InlineAsyncResultStorage {
    storage: MaybeUninit<InlineAsyncResultBytes>,
    allocation: Option<AsyncResultFrameAllocation>,
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
            allocation: None,
            drop: None,
            type_id: None,
            occupied: false,
        }
    }

    const fn supports_inline<T: 'static>() -> bool {
        size_of::<T>() <= size_of::<InlineAsyncResultBytes>()
            && align_of::<T>() <= align_of::<InlineAsyncResultBytes>()
    }

    fn supports<T: 'static>(result_store: &AsyncTaskResultStore) -> bool {
        Self::supports_inline::<T>() || result_store.supports::<T>()
    }

    fn store<T: 'static>(
        &mut self,
        result_store: &AsyncTaskResultStore,
        value: T,
    ) -> Result<(), ExecutorError> {
        if self.occupied {
            return Err(executor_invalid());
        }
        if !Self::supports::<T>(result_store) {
            return Err(ExecutorError::Unsupported);
        }

        let target = if Self::supports_inline::<T>() {
            self.storage.as_mut_ptr().cast::<T>()
        } else {
            let mut allocation = result_store.allocate_for::<T>()?;
            let ptr = allocation.ptr().cast::<T>();
            self.allocation = Some(allocation);
            ptr
        };
        unsafe {
            target.write(value);
        }
        self.drop = Some(drop_inline_async_value::<T>);
        self.type_id = Some(TypeId::of::<T>());
        self.occupied = true;
        Ok(())
    }

    fn take<T: 'static>(
        &mut self,
        result_store: &AsyncTaskResultStore,
    ) -> Result<T, ExecutorError> {
        if !self.occupied || self.type_id != Some(TypeId::of::<T>()) {
            return Err(executor_invalid());
        }

        self.drop = None;
        self.type_id = None;
        self.occupied = false;
        let value = unsafe { self.storage_ptr().cast::<T>().read() };
        if let Some(allocation) = self.allocation.take() {
            result_store.deallocate(allocation)?;
        }
        Ok(value)
    }

    fn clear(&mut self, result_store: &AsyncTaskResultStore) -> Result<(), ExecutorError> {
        self.drop_value_only();
        if let Some(allocation) = self.allocation.take() {
            result_store.deallocate(allocation)?;
        }
        self.type_id = None;
        Ok(())
    }

    fn storage_ptr(&mut self) -> *mut u8 {
        match self.allocation.as_mut() {
            Some(allocation) => allocation.ptr(),
            None => self.storage.as_mut_ptr().cast::<u8>(),
        }
    }

    fn drop_value_only(&mut self) {
        if !self.occupied {
            self.drop = None;
            self.type_id = None;
            return;
        }

        if let Some(drop) = self.drop.take() {
            unsafe {
                drop(self.storage_ptr());
            }
        }
        self.type_id = None;
        self.occupied = false;
    }
}

impl Drop for InlineAsyncResultStorage {
    fn drop(&mut self) {
        self.drop_value_only();
    }
}

#[derive(Debug)]
struct AsyncTaskResultStore {
    medium_allocator: Option<ExecutorDomainAllocator>,
    medium: ExecutorCell<Option<Slab<ASYNC_RESULT_CLASS_MEDIUM_BYTES, TASK_REGISTRY_CAPACITY>>>,
    large_allocator: Option<ExecutorDomainAllocator>,
    large: ExecutorCell<Option<Slab<ASYNC_RESULT_CLASS_LARGE_BYTES, TASK_REGISTRY_CAPACITY>>>,
}

impl AsyncTaskResultStore {
    fn new(
        fast: bool,
        medium_allocator: Option<ExecutorDomainAllocator>,
        large_allocator: Option<ExecutorDomainAllocator>,
    ) -> Self {
        Self {
            medium_allocator,
            medium: ExecutorCell::new(fast, None),
            large_allocator,
            large: ExecutorCell::new(fast, None),
        }
    }

    fn supports<T: 'static>(&self) -> bool {
        let len = size_of::<T>();
        let align = align_of::<T>();
        (len <= ASYNC_RESULT_CLASS_MEDIUM_BYTES
            && align <= async_storage_class_slot_align(ASYNC_RESULT_CLASS_MEDIUM_BYTES))
            || (len <= ASYNC_RESULT_CLASS_LARGE_BYTES
                && align <= async_storage_class_slot_align(ASYNC_RESULT_CLASS_LARGE_BYTES))
    }

    fn allocate_for<T: 'static>(&self) -> Result<AsyncResultFrameAllocation, ExecutorError> {
        let request = fusion_sys::alloc::AllocRequest {
            len: size_of::<T>(),
            align: align_of::<T>(),
            zeroed: false,
        };
        if request.len <= ASYNC_RESULT_CLASS_MEDIUM_BYTES
            && request.align <= async_storage_class_slot_align(ASYNC_RESULT_CLASS_MEDIUM_BYTES)
        {
            return self.medium.with(|medium| {
                if medium.is_none() {
                    *medium = Some(
                        self.medium_allocator
                            .as_ref()
                            .ok_or(ExecutorError::Unsupported)?
                            .slab::<ASYNC_RESULT_CLASS_MEDIUM_BYTES, TASK_REGISTRY_CAPACITY>()?,
                    );
                }
                medium
                    .as_ref()
                    .ok_or_else(executor_invalid)?
                    .allocate(&request)
                    .map(AsyncResultFrameAllocation::Medium)
                    .map_err(executor_error_from_alloc)
            })?;
        }
        if request.len <= ASYNC_RESULT_CLASS_LARGE_BYTES
            && request.align <= async_storage_class_slot_align(ASYNC_RESULT_CLASS_LARGE_BYTES)
        {
            return self.large.with(|large| {
                if large.is_none() {
                    *large = Some(
                        self.large_allocator
                            .as_ref()
                            .ok_or(ExecutorError::Unsupported)?
                            .slab::<ASYNC_RESULT_CLASS_LARGE_BYTES, TASK_REGISTRY_CAPACITY>()?,
                    );
                }
                large
                    .as_ref()
                    .ok_or_else(executor_invalid)?
                    .allocate(&request)
                    .map(AsyncResultFrameAllocation::Large)
                    .map_err(executor_error_from_alloc)
            })?;
        }
        Err(ExecutorError::Unsupported)
    }

    fn deallocate(&self, allocation: AsyncResultFrameAllocation) -> Result<(), ExecutorError> {
        match allocation {
            AsyncResultFrameAllocation::Medium(allocation) => self.medium.with(|medium| {
                medium
                    .as_ref()
                    .ok_or_else(executor_invalid)?
                    .deallocate(allocation)
                    .map_err(executor_error_from_alloc)
            })?,
            AsyncResultFrameAllocation::Large(allocation) => self.large.with(|large| {
                large
                    .as_ref()
                    .ok_or_else(executor_invalid)?
                    .deallocate(allocation)
                    .map_err(executor_error_from_alloc)
            })?,
        }
    }
}

#[derive(Debug)]
enum AsyncResultFrameAllocation {
    Medium(fusion_sys::alloc::AllocResult),
    Large(fusion_sys::alloc::AllocResult),
}

impl AsyncResultFrameAllocation {
    fn ptr(&mut self) -> *mut u8 {
        match self {
            Self::Medium(allocation) | Self::Large(allocation) => allocation.ptr.as_ptr(),
        }
    }
}

unsafe impl Send for AsyncResultFrameAllocation {}
unsafe impl Sync for AsyncResultFrameAllocation {}

unsafe fn poll_inline_async_future<F>(
    ptr: *mut u8,
    result: &ExecutorCell<InlineAsyncResultStorage>,
    result_store: &AsyncTaskResultStore,
    context: &mut Context<'_>,
) -> Result<Poll<()>, ExecutorError>
where
    F: Future + 'static,
    F::Output: 'static,
{
    // SAFETY: executor futures live inside arena-backed task slots whose addresses remain stable
    // for the lifetime of the live slot lease; the arena never relocates allocations.
    let future = unsafe { Pin::new_unchecked(&mut *ptr.cast::<F>()) };

    #[cfg(feature = "std")]
    match poll_future_contained(future, context) {
        Ok(Poll::Ready(output)) => {
            result.with(|result| result.store(result_store, output))??;
            Ok(Poll::Ready(()))
        }
        Ok(Poll::Pending) => Ok(Poll::Pending),
        Err(()) => Err(ExecutorError::TaskPanicked),
    }

    #[cfg(not(feature = "std"))]
    match poll_future_contained(future, context) {
        Poll::Ready(output) => {
            result.with(|result| result.store(result_store, output))??;
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
    core: ExecutorCell<Option<ControlLease<ExecutorCore>>>,
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
        future_store: &AsyncTaskFutureStore,
        result_store: &AsyncTaskResultStore,
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

        self.future.with(|future| future.clear(future_store))??;
        self.result.with(|result| result.clear(result_store))??;
        self.error.with(|error| *error = None)?;
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
        future_store: &AsyncTaskFutureStore,
        result_store: &AsyncTaskResultStore,
        future: F,
    ) -> Result<(), ExecutorError>
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        self.future
            .with(|slot| slot.store_future(future_store, result_store, future))?
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
        result_store: &AsyncTaskResultStore,
        generation: u64,
        context: &mut Context<'_>,
    ) -> Result<Poll<()>, ExecutorError> {
        if self.generation() != generation || self.state() != SLOT_PENDING {
            return Ok(Poll::Ready(()));
        }
        self.future
            .with(|future| future.poll_in_place(&self.result, result_store, context))?
    }

    fn complete(
        &self,
        future_store: &AsyncTaskFutureStore,
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

        self.future.with(|future| future.clear(future_store))??;
        self.error.with(|error| *error = None)?;
        self.clear_run_state();
        self.wake_join_waker()?;
        self.signal_completed()?;
        Ok(())
    }

    fn fail(
        &self,
        future_store: &AsyncTaskFutureStore,
        result_store: &AsyncTaskResultStore,
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

        self.future.with(|future| future.clear(future_store))??;
        self.result.with(|result| result.clear(result_store))??;
        self.error.with(|slot| *slot = Some(error))?;
        self.clear_run_state();
        self.wake_join_waker()?;
        self.signal_completed()?;
        Ok(())
    }

    fn cancel(
        &self,
        future_store: &AsyncTaskFutureStore,
        result_store: &AsyncTaskResultStore,
        generation: u64,
    ) -> Result<(), ExecutorError> {
        self.fail(
            future_store,
            result_store,
            generation,
            ExecutorError::Cancelled,
        )
    }

    fn clear_core_if_no_wakers(&self, generation: u64) -> Result<bool, ExecutorError> {
        if self.generation() != generation {
            return Err(ExecutorError::Stopped);
        }
        if self.waker_refs.load(Ordering::Acquire) != 0 {
            return Ok(false);
        }
        self.core.with(|core| *core = None)?;
        self.waker.set_core(core::ptr::null());
        Ok(true)
    }

    fn force_shutdown(
        &self,
        future_store: &AsyncTaskFutureStore,
        result_store: &AsyncTaskResultStore,
        generation: u64,
    ) -> Result<(), ExecutorError> {
        if self.generation() != generation {
            return Ok(());
        }

        match self.state() {
            SLOT_PENDING => {
                let _ = self.fail(
                    future_store,
                    result_store,
                    generation,
                    ExecutorError::Stopped,
                );
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
        result_store: &AsyncTaskResultStore,
        generation: u64,
    ) -> Result<T, ExecutorError> {
        if self.generation() != generation {
            return Err(ExecutorError::Stopped);
        }
        match self.state() {
            SLOT_READY => self.result.with(|result| result.take::<T>(result_store))?,
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
        future_store: &AsyncTaskFutureStore,
        result_store: &AsyncTaskResultStore,
        generation: u64,
    ) -> Result<(), ExecutorError> {
        if self.generation() != generation {
            return Err(ExecutorError::Stopped);
        }

        self.future.with(|future| future.clear(future_store))??;
        self.result.with(|result| result.clear(result_store))??;
        self.error.with(|error| *error = None)?;
        self.join_waker.with(|waker| *waker = None)?;
        self.drain_completed()?;
        self.clear_run_state();
        self.handle_live.store(false, Ordering::Release);
        self.state.store(SLOT_EMPTY, Ordering::Release);
        self.core.with(|core| *core = None)?;
        self.waker.set_core(core::ptr::null());
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
                .map(|completed| core::ptr::from_ref(completed))
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
    future_store: AsyncTaskFutureStore,
    result_store: AsyncTaskResultStore,
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
            future_store: AsyncTaskFutureStore::new(
                fast,
                allocators.future_medium.take(),
                allocators.future_large.take(),
            ),
            result_store: AsyncTaskResultStore::new(
                fast,
                allocators.result_medium.take(),
                allocators.result_large.take(),
            ),
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
            .initialize_for_allocation(&self.future_store, &self.result_store)?;
        Ok((slot_index, generation))
    }

    fn release_slot(&self, slot_index: usize, generation: u64) -> Result<(), ExecutorError> {
        let slot = self.slot(slot_index)?;
        if slot.generation() != generation || slot.state() != SLOT_EMPTY {
            return Err(executor_invalid());
        }
        self.free.with(|free| free.push(slot_index))?
    }
}

impl Drop for AsyncTaskRegistry {
    fn drop(&mut self) {
        for slot in &self.slots {
            let generation = slot.generation();
            if generation == 0 {
                continue;
            }
            let _ = slot.force_shutdown(&self.future_store, &self.result_store, generation);
            let _ = slot.reset_empty(&self.future_store, &self.result_store, generation);
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
                core: core::ptr::from_ref(core) as usize,
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
    shutdown_requested: AtomicBool,
    external_inflight: AtomicUsize,
}

impl fmt::Debug for ExecutorCore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecutorCore")
            .field("scheduler", &self.scheduler)
            .finish_non_exhaustive()
    }
}

impl ExecutorCore {
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
            core::ptr::from_ref(self) as usize,
            sequence as u64,
        ))
    }

    fn registry(&self) -> Result<&AsyncTaskRegistry, ExecutorError> {
        self.registry.get()
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
            let _ = slot.fail(
                &registry.future_store,
                &registry.result_store,
                generation,
                error,
            );
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
                pool.spawn(move || {
                    run_scheduled_green_slot_lease(scheduled_core, slot_index, generation)
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
            let _ = slot.fail(
                &registry.future_store,
                &registry.result_store,
                generation,
                error,
            );
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

        let context_guard = AsyncTaskContextGuard::install(self, slot_index, generation);
        let poll = {
            let Ok(waker) = slot.create_waker(generation) else {
                let _ = slot.finish_pending_run();
                return AsyncSlotRunDisposition::Terminal;
            };
            let mut context = Context::from_waker(&waker);
            slot.poll_in_place(&registry.result_store, generation, &mut context)
        };
        let self_requeue = take_current_async_requeue();
        #[cfg(feature = "std")]
        let self_requeue_core = requeue_core;
        drop(context_guard);

        match poll {
            Ok(Poll::Ready(())) => {
                let _ = slot.finish_pending_run();
                let _ = slot.complete(&registry.future_store, generation);
                let _ = self.recycle_slot_if_possible(slot_index, generation);
                AsyncSlotRunDisposition::Terminal
            }
            Ok(Poll::Pending) => {
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
                AsyncSlotRunDisposition::Pending
            }
            Err(error) => {
                let _ = slot.finish_pending_run();
                let _ = slot.fail(
                    &registry.future_store,
                    &registry.result_store,
                    generation,
                    error,
                );
                let _ = self.recycle_slot_if_possible(slot_index, generation);
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
        slot.reset_empty(&registry.future_store, &registry.result_store, generation)?;
        registry.release_slot(slot_index, generation)
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
            let _ = slot.force_shutdown(&registry.future_store, &registry.result_store, generation);
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

struct TaskHandleInner<T> {
    id: TaskId,
    admission: AsyncTaskAdmission,
    core: ControlLease<ExecutorCore>,
    slot_index: usize,
    generation: u64,
    active: bool,
    _marker: PhantomData<T>,
}

impl<T> fmt::Debug for TaskHandleInner<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskHandleInner")
            .field("id", &self.id)
            .field("admission", &self.admission)
            .field("slot_index", &self.slot_index)
            .field("generation", &self.generation)
            .field("active", &self.active)
            .finish_non_exhaustive()
    }
}

impl<T> TaskHandleInner<T> {
    const fn id(&self) -> TaskId {
        self.id
    }

    const fn admission(&self) -> AsyncTaskAdmission {
        self.admission
    }

    fn is_finished(&self) -> Result<bool, ExecutorError> {
        self.core
            .registry()?
            .slot(self.slot_index)?
            .is_finished(self.generation)
    }

    fn join(&mut self) -> Result<T, ExecutorError>
    where
        T: 'static,
    {
        let registry = self.core.registry()?;
        let slot = registry.slot(self.slot_index)?;
        match &self.core.scheduler {
            SchedulerBinding::Current => {
                while !slot.is_finished(self.generation)? {
                    if !self.core.drive_current_once()?
                        && !self.core.drive_reactor_once(true)?
                        && system_thread().yield_now().is_err()
                    {
                        spin_loop();
                    }
                }
            }
            _ => {
                if !slot.is_finished(self.generation)? {
                    slot.ensure_completed_semaphore()?;
                    if !slot.is_finished(self.generation)? {
                        slot.wait_completed()?;
                    }
                }
            }
        }

        let result = slot.take_result::<T>(&registry.result_store, self.generation);
        self.active = false;
        let _ = self.core.detach_handle(self.slot_index, self.generation);
        result
    }

    fn abort(&self) -> Result<(), ExecutorError> {
        let slot = self.core.registry()?.slot(self.slot_index)?;
        let _ = self.core.clear_wait(self.slot_index, self.generation);
        let registry = self.core.registry()?;
        slot.cancel(
            &registry.future_store,
            &registry.result_store,
            self.generation,
        )?;
        self.core
            .recycle_slot_if_possible(self.slot_index, self.generation)
    }

    fn poll_join(&mut self, cx: &Context<'_>) -> Poll<Result<T, ExecutorError>>
    where
        T: 'static,
    {
        if !self.active {
            return Poll::Ready(Err(ExecutorError::Stopped));
        }
        let slot = match self
            .core
            .registry()
            .and_then(|registry| registry.slot(self.slot_index))
        {
            Ok(slot) => slot,
            Err(error) => return Poll::Ready(Err(error)),
        };
        if let Err(error) = slot.register_join_waker(self.generation, cx.waker()) {
            return Poll::Ready(Err(error));
        }
        match slot.is_finished(self.generation) {
            Ok(true) => {
                let registry = match self.core.registry() {
                    Ok(registry) => registry,
                    Err(error) => return Poll::Ready(Err(error)),
                };
                let result = slot.take_result::<T>(&registry.result_store, self.generation);
                self.active = false;
                let _ = self.core.detach_handle(self.slot_index, self.generation);
                Poll::Ready(result)
            }
            Ok(false) => Poll::Pending,
            Err(error) => Poll::Ready(Err(error)),
        }
    }

    fn detach_if_active(&self) {
        if self.active {
            let _ = self.core.detach_handle(self.slot_index, self.generation);
        }
    }
}

/// Public spawned-task handle for `Send` async work.
pub struct TaskHandle<T> {
    inner: TaskHandleInner<T>,
}

impl<T> fmt::Debug for TaskHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskHandle")
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

impl<T> TaskHandle<T> {
    /// Returns the stable task identifier.
    #[must_use]
    pub const fn id(&self) -> TaskId {
        self.inner.id()
    }

    /// Returns the truthful admission snapshot for this task.
    #[must_use]
    pub const fn admission(&self) -> AsyncTaskAdmission {
        self.inner.admission()
    }

    /// Returns whether the task has completed.
    ///
    /// # Errors
    ///
    /// Returns an error if the task state cannot be observed honestly.
    pub fn is_finished(&self) -> Result<bool, ExecutorError> {
        self.inner.is_finished()
    }

    /// Aborts the task and causes subsequent joins to resolve to `Cancelled`.
    ///
    /// # Errors
    ///
    /// Returns an error if the task cannot be cancelled honestly.
    pub fn abort(&self) -> Result<(), ExecutorError> {
        self.inner.abort()
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
        self.inner.join()
    }
}

impl<T> Unpin for TaskHandle<T> {}

impl<T: 'static> Future for TaskHandle<T> {
    type Output = Result<T, ExecutorError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.get_mut().inner.poll_join(cx)
    }
}

impl<T> Drop for TaskHandle<T> {
    fn drop(&mut self) {
        self.inner.detach_if_active();
    }
}

/// Public spawned-task handle for local non-`Send` async work.
pub struct LocalTaskHandle<T> {
    inner: TaskHandleInner<T>,
    _not_send_sync: PhantomData<*mut ()>,
}

impl<T> fmt::Debug for LocalTaskHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalTaskHandle")
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

impl<T> LocalTaskHandle<T> {
    /// Returns the stable task identifier.
    #[must_use]
    pub const fn id(&self) -> TaskId {
        self.inner.id()
    }

    /// Returns the truthful admission snapshot for this task.
    #[must_use]
    pub const fn admission(&self) -> AsyncTaskAdmission {
        self.inner.admission()
    }

    /// Returns whether the task has completed.
    ///
    /// # Errors
    ///
    /// Returns an error if the task state cannot be observed honestly.
    pub fn is_finished(&self) -> Result<bool, ExecutorError> {
        self.inner.is_finished()
    }

    /// Aborts the local task and causes subsequent joins to resolve to `Cancelled`.
    ///
    /// # Errors
    ///
    /// Returns an error if the task cannot be cancelled honestly.
    pub fn abort(&self) -> Result<(), ExecutorError> {
        self.inner.abort()
    }

    /// Blocks until the local task completes and returns its result.
    ///
    /// # Errors
    ///
    /// Returns the scheduler failure that stopped the task, if any.
    pub fn join(mut self) -> Result<T, ExecutorError>
    where
        T: 'static,
    {
        self.inner.join()
    }
}

impl<T> Unpin for LocalTaskHandle<T> {}

impl<T: 'static> Future for LocalTaskHandle<T> {
    type Output = Result<T, ExecutorError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.get_mut().inner.poll_join(cx)
    }
}

impl<T> Drop for LocalTaskHandle<T> {
    fn drop(&mut self) {
        self.inner.detach_if_active();
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

    /// Aborts every tracked task while preserving their handles for later observation.
    ///
    /// # Errors
    ///
    /// Returns an error if any tracked task cannot be cancelled honestly.
    pub fn abort_all(&self) -> Result<(), ExecutorError> {
        let state = self.entries.lock().map_err(executor_error_from_sync)?;
        for handle in state.entries.iter().flatten() {
            handle.abort()?;
        }
        Ok(())
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
                        && matches!(handle.inner.core.scheduler, SchedulerBinding::Current)
                    {
                        current_executor = Some(
                            handle
                                .inner
                                .core
                                .try_clone()
                                .map_err(executor_error_from_alloc)?,
                        );
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
                if !core.drive_current_once()?
                    && !core.drive_reactor_once(true)?
                    && system_thread().yield_now().is_err()
                {
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

/// Current-thread async runtime using ordinary Rust futures as the front door.
#[derive(Debug)]
pub struct CurrentAsyncRuntime {
    executor: Executor,
    _not_send_sync: PhantomData<*mut ()>,
}

impl Default for CurrentAsyncRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl CurrentAsyncRuntime {
    /// Returns the explicit current-thread runtime backing plan for one executor configuration.
    ///
    /// # Errors
    ///
    /// Returns any honest sizing or metadata-overflow failure while shaping the runtime domains.
    pub fn backing_plan(
        config: ExecutorConfig,
    ) -> Result<CurrentAsyncRuntimeBackingPlan, ExecutorError> {
        CurrentAsyncRuntimeBackingPlan::for_config(config.with_mode(ExecutorMode::CurrentThread))
    }

    /// Returns the explicit current-thread runtime backing plan under one explicit allocator
    /// layout policy.
    pub fn backing_plan_with_layout_policy(
        config: ExecutorConfig,
        layout_policy: AllocatorLayoutPolicy,
    ) -> Result<CurrentAsyncRuntimeBackingPlan, ExecutorError> {
        CurrentAsyncRuntimeBackingPlan::for_config_with_layout_policy(
            config.with_mode(ExecutorMode::CurrentThread),
            layout_policy,
        )
    }

    /// Returns the explicit current-thread runtime backing plan under one explicit allocator
    /// layout policy and one explicit executor-planning surface.
    pub fn backing_plan_with_layout_policy_and_planning_support(
        config: ExecutorConfig,
        layout_policy: AllocatorLayoutPolicy,
        planning: ExecutorPlanningSupport,
    ) -> Result<CurrentAsyncRuntimeBackingPlan, ExecutorError> {
        CurrentAsyncRuntimeBackingPlan::for_config_with_layout_policy_and_planning_support(
            config.with_mode(ExecutorMode::CurrentThread),
            layout_policy,
            planning,
        )
    }

    /// Creates one current-thread async runtime.
    #[must_use]
    pub fn new() -> Self {
        Self {
            executor: Executor::new_fast_current(),
            _not_send_sync: PhantomData,
        }
    }

    /// Creates one current-thread async runtime with one explicit executor configuration.
    #[must_use]
    pub fn with_executor_config(config: ExecutorConfig) -> Self {
        Self {
            executor: Executor::with_scheduler(
                config.with_mode(ExecutorMode::CurrentThread),
                SchedulerBinding::Current,
                true,
            ),
            _not_send_sync: PhantomData,
        }
    }

    /// Creates one current-thread async runtime from explicit backing resources.
    ///
    /// # Errors
    ///
    /// Returns any honest resource-shape or executor bootstrap failure.
    pub fn from_backing(
        config: ExecutorConfig,
        backing: CurrentAsyncRuntimeBacking,
    ) -> Result<Self, ExecutorError> {
        let executor = Executor::with_current_backing(
            config.with_mode(ExecutorMode::CurrentThread),
            true,
            backing,
        );
        executor.core()?;
        Ok(Self {
            executor,
            _not_send_sync: PhantomData,
        })
    }

    /// Creates one current-thread async runtime from one caller-owned bound slab.
    ///
    /// # Errors
    ///
    /// Returns any honest sizing, partitioning, or bootstrap failure.
    pub fn from_bound_slab(
        config: ExecutorConfig,
        slab: MemoryResourceHandle,
    ) -> Result<Self, ExecutorError> {
        let layout = Self::backing_plan_with_layout_policy(config, slab.info().layout)?
            .combined_eager_for_base_alignment(executor_resource_base_alignment(&slab))?;
        if slab.view().len() < layout.slab.bytes {
            return Err(ExecutorError::Sync(SyncErrorKind::Overflow));
        }
        let backing = CurrentAsyncRuntimeBacking {
            control: partition_executor_bound_resource(&slab, layout.control)?,
            reactor: partition_executor_bound_resource(&slab, layout.reactor)?,
            registry: partition_executor_bound_resource(&slab, layout.registry)?,
            future_medium: None,
            future_large: None,
            result_medium: None,
            result_large: None,
        };
        Self::from_backing(config, backing)
    }

    /// Creates one current-thread async runtime from one caller-owned static byte slab.
    ///
    /// This is the ergonomic deterministic board-facing path above `from_bound_slab(...)` for
    /// SRAM-backed static runtime storage.
    ///
    /// # Safety
    ///
    /// The caller must guarantee the supplied pointer/length pair names one valid writable static
    /// memory extent for the whole lifetime of the runtime.
    ///
    /// # Errors
    ///
    /// Returns any honest binding, sizing, partitioning, or bootstrap failure.
    pub unsafe fn from_static_slab(
        config: ExecutorConfig,
        ptr: *mut u8,
        len: usize,
    ) -> Result<Self, ExecutorError> {
        let slab = MemoryResourceHandle::from(
            unsafe { BoundMemoryResource::static_allocatable_bytes(ptr, len) }
                .map_err(executor_error_from_resource)?,
        );
        Self::from_bound_slab(config, slab)
    }

    /// Returns the underlying executor.
    #[must_use]
    pub const fn executor(&self) -> &Executor {
        &self.executor
    }

    /// Spawns one ordinary Rust future onto the current-thread runtime.
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

    /// Spawns one local non-`Send` future onto the current-thread runtime.
    ///
    /// # Errors
    ///
    /// Returns any honest executor admission or scheduler failure.
    pub fn spawn_local<F>(&self, future: F) -> Result<LocalTaskHandle<F::Output>, ExecutorError>
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        self.executor.spawn_local(future)
    }

    /// Spawns one local non-`Send` future with one explicit poll-stack contract.
    ///
    /// # Errors
    ///
    /// Returns any honest executor admission or scheduler failure.
    pub fn spawn_local_with_poll_stack_bytes<F>(
        &self,
        poll_stack_bytes: usize,
        future: F,
    ) -> Result<LocalTaskHandle<F::Output>, ExecutorError>
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        self.executor
            .spawn_local_with_poll_stack_bytes(poll_stack_bytes, future)
    }

    /// Spawns one local non-`Send` future using one compile-time generated async poll-stack
    /// contract.
    ///
    /// # Errors
    ///
    /// Returns any honest executor admission or scheduler failure.
    pub fn spawn_local_generated<F>(
        &self,
        future: F,
    ) -> Result<LocalTaskHandle<F::Output>, ExecutorError>
    where
        F: Future + 'static + GeneratedExplicitAsyncPollStackContract,
        F::Output: 'static,
    {
        self.executor.spawn_local_generated(future)
    }

    /// Drives one ready async task.
    ///
    /// # Errors
    ///
    /// Returns any honest executor failure.
    pub fn drive_once(&self) -> Result<bool, ExecutorError> {
        self.executor.drive_once()
    }

    /// Drains the current-thread async queue until idle.
    ///
    /// # Errors
    ///
    /// Returns any honest executor failure.
    pub fn run_until_idle(&self) -> Result<usize, ExecutorError> {
        self.executor.run_until_idle()
    }

    /// Drives one future to completion on the current thread.
    ///
    /// # Errors
    ///
    /// Returns any honest executor failure.
    pub fn block_on<F>(&self, future: F) -> Result<F::Output, ExecutorError>
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        self.executor.block_on(future)
    }
}

/// Hosted async runtime backed by system-thread carriers.
#[cfg(feature = "std")]
#[derive(Debug)]
pub struct ThreadAsyncRuntime {
    executor: Option<Executor>,
    carriers: Option<ThreadAsyncCarriers>,
}

#[cfg(feature = "std")]
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

#[cfg(feature = "std")]
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

#[cfg(feature = "std")]
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
        let fibers = HostedFiberRuntime::fixed(total_fibers).map_err(executor_error_from_fiber)?;
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
        let inner = match ExecutorBackingAllocators::from_current_backing(backing).and_then(
            |mut allocators| {
                let (reactor_state, current_queue) =
                    ExecutorReactorState::new(config.capacity, fast_current, &allocators.reactor)?;
                let registry =
                    ExecutorRegistry::new(config.capacity, fast_current, &mut allocators);
                let core = allocators.control.control(ExecutorCore {
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
                    shutdown_requested: AtomicBool::new(false),
                    external_inflight: AtomicUsize::new(0),
                })?;
                Ok(core)
            },
        ) {
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
                            shutdown_requested: AtomicBool::new(false),
                            external_inflight: AtomicUsize::new(0),
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
            slot.reset_empty(&registry.future_store, &registry.result_store, generation)?;
            registry.release_slot(slot_index, generation)?;
            return Err(error);
        }

        if let Err(error) =
            slot.store_future(&registry.future_store, &registry.result_store, future)
        {
            slot.mark_handle_released(generation)?;
            slot.reset_empty(&registry.future_store, &registry.result_store, generation)?;
            registry.release_slot(slot_index, generation)?;
            return Err(error);
        }

        if let Err(error) = core.schedule_slot(slot_index, generation) {
            slot.mark_handle_released(generation)?;
            let _ = core.recycle_slot_if_possible(slot_index, generation);
            return Err(error);
        }

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
    /// Returns `Unsupported` when this executor is not current-thread driven.
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
            slot.reset_empty(&registry.future_store, &registry.result_store, generation)?;
            registry.release_slot(slot_index, generation)?;
            return Err(error);
        }

        if let Err(error) =
            slot.store_future(&registry.future_store, &registry.result_store, future)
        {
            slot.mark_handle_released(generation)?;
            slot.reset_empty(&registry.future_store, &registry.result_store, generation)?;
            registry.release_slot(slot_index, generation)?;
            return Err(error);
        }

        if let Err(error) = core.schedule_slot(slot_index, generation) {
            slot.mark_handle_released(generation)?;
            let _ = core.recycle_slot_if_possible(slot_index, generation);
            return Err(error);
        }

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
    /// Returns `Unsupported` when this executor is not in current-thread mode.
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
                        let _ = slot.fail(
                            &registry.future_store,
                            &registry.result_store,
                            generation,
                            ExecutorError::Stopped,
                        );
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

#[cfg(feature = "std")]
fn executor_error_from_std_thread(error: std::io::Error) -> ExecutorError {
    if error.kind() == std::io::ErrorKind::OutOfMemory {
        return ExecutorError::Sync(SyncErrorKind::Overflow);
    }
    ExecutorError::Sync(SyncErrorKind::Invalid)
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
    executor_registry_capacity_with_planning_support(
        capacity,
        ExecutorPlanningSupport::compiled_binary(),
    )
}

fn executor_registry_align() -> usize {
    executor_registry_align_with_planning_support(ExecutorPlanningSupport::compiled_binary())
}

fn executor_registry_capacity_with_planning_support(
    capacity: usize,
    planning: ExecutorPlanningSupport,
) -> Result<usize, ExecutorError> {
    planning.registry_capacity(capacity)
}

fn executor_registry_align_with_planning_support(planning: ExecutorPlanningSupport) -> usize {
    planning.registry_align()
}

fn executor_reactor_align() -> usize {
    executor_reactor_align_with_planning_support(ExecutorPlanningSupport::compiled_binary())
}

fn executor_reactor_align_with_planning_support(planning: ExecutorPlanningSupport) -> usize {
    planning.reactor_align()
}

fn executor_reactor_capacity_with_planning_support(
    capacity: usize,
    planning: ExecutorPlanningSupport,
) -> Result<usize, ExecutorError> {
    planning.reactor_capacity(capacity)
}

fn executor_reactor_capacity(capacity: usize) -> Result<usize, ExecutorError> {
    executor_reactor_capacity_with_planning_support(
        capacity,
        ExecutorPlanningSupport::compiled_binary(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread::{PoolPlacement, ThreadPoolConfig};
    use core::num::NonZeroUsize;
    use core::sync::atomic::{AtomicUsize, Ordering};
    use fusion_pal::sys::mem::{Address, CachePolicy, MemAdviceCaps, Protect, Region};
    use fusion_sys::mem::resource::{
        BoundMemoryResource,
        BoundResourceSpec,
        MemoryDomain,
        MemoryGeometry,
        MemoryResourceHandle,
        OvercommitPolicy,
        ResourceAttrs,
        ResourceBackingKind,
        ResourceContract,
        ResourceOpSet,
        ResourceResidencySupport,
        ResourceState,
        ResourceSupport,
        SharingPolicy,
        StateValue,
    };
    use fusion_sys::thread::{ThreadLogicalCpuId, ThreadProcessorGroupId};
    use std::sync::Arc;
    #[cfg(feature = "std")]
    use std::sync::atomic::AtomicBool;
    #[cfg(feature = "std")]
    use std::task::Wake;
    #[cfg(feature = "std")]
    use std::thread;
    #[cfg(feature = "std")]
    use std::time::Duration;

    fn aligned_bound_resource(len: usize, align: usize) -> MemoryResourceHandle {
        use std::alloc::{Layout, alloc_zeroed};

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
    fn compiled_executor_planning_support_matches_compiled_layout() {
        let support = ExecutorPlanningSupport::compiled_binary();
        let control = ControlLease::<ExecutorCore>::extent_request()
            .expect("executor control extent request should build");
        assert_eq!(support.control_bytes, control.len);
        assert_eq!(support.control_align, control.align);
        assert_eq!(
            support.reactor_wait_entry_bytes,
            size_of::<AsyncReactorWaitEntry>()
        );
        assert_eq!(
            support.reactor_wait_entry_align,
            align_of::<AsyncReactorWaitEntry>()
        );
        assert_eq!(
            support.reactor_outcome_entry_bytes,
            size_of::<Option<AsyncWaitOutcome>>()
        );
        assert_eq!(
            support.reactor_outcome_entry_align,
            align_of::<Option<AsyncWaitOutcome>>()
        );
        assert_eq!(
            support.reactor_queue_entry_bytes,
            size_of::<Option<CurrentJob>>()
        );
        assert_eq!(
            support.reactor_queue_entry_align,
            align_of::<Option<CurrentJob>>()
        );
        #[cfg(feature = "std")]
        {
            assert_eq!(
                support.reactor_pending_entry_bytes,
                size_of::<Option<EventKey>>()
            );
            assert_eq!(
                support.reactor_pending_entry_align,
                align_of::<Option<EventKey>>()
            );
        }
        #[cfg(not(feature = "std"))]
        {
            assert_eq!(support.reactor_pending_entry_bytes, 0);
            assert_eq!(support.reactor_pending_entry_align, 1);
        }
        assert_eq!(support.registry_free_entry_bytes, size_of::<usize>());
        assert_eq!(support.registry_free_entry_align, align_of::<usize>());
        assert_eq!(support.registry_slot_bytes, size_of::<AsyncTaskSlot>());
        assert_eq!(support.registry_slot_align, align_of::<AsyncTaskSlot>());
    }

    #[test]
    fn explicit_executor_planning_support_shapes_current_runtime_backing() {
        let config = ExecutorConfig::new().with_capacity(1);
        let compiled = CurrentAsyncRuntime::backing_plan_with_layout_policy_and_planning_support(
            config,
            AllocatorLayoutPolicy::exact_static(),
            ExecutorPlanningSupport::compiled_binary(),
        )
        .expect("compiled planning support should shape a current runtime");
        let custom_support = ExecutorPlanningSupport {
            control_bytes: 8192,
            ..ExecutorPlanningSupport::compiled_binary()
        };
        let custom = CurrentAsyncRuntime::backing_plan_with_layout_policy_and_planning_support(
            config,
            AllocatorLayoutPolicy::exact_static(),
            custom_support,
        )
        .expect("custom planning support should shape a current runtime");
        assert!(custom.control.bytes >= compiled.control.bytes);
        assert!(custom.control.bytes > compiled.control.bytes);
    }

    struct ExplicitGeneratedPollStackFuture;

    impl Future for ExplicitGeneratedPollStackFuture {
        type Output = u8;

        fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
            Poll::Ready(55)
        }
    }

    crate::declare_generated_async_poll_stack_contract!(ExplicitGeneratedPollStackFuture, 1792);

    #[cfg(feature = "std")]
    #[derive(Debug)]
    struct TestPipe {
        read_fd: i32,
        write_fd: i32,
    }

    #[cfg(feature = "std")]
    impl TestPipe {
        fn new() -> Self {
            let mut fds = [0_i32; 2];
            let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) };
            assert_eq!(rc, 0, "test pipe should create");
            Self {
                read_fd: fds[0],
                write_fd: fds[1],
            }
        }

        fn source(&self) -> EventSourceHandle {
            EventSourceHandle(
                usize::try_from(self.read_fd).expect("pipe fd should be non-negative"),
            )
        }

        fn write_byte(&self, value: u8) {
            let rc = unsafe {
                libc::write(
                    self.write_fd,
                    (&raw const value).cast::<libc::c_void>(),
                    core::mem::size_of::<u8>(),
                )
            };
            assert_eq!(rc, 1, "test pipe should become readable");
        }

        fn read_byte(&self) -> u8 {
            let mut byte = 0_u8;
            loop {
                let rc = unsafe {
                    libc::read(
                        self.read_fd,
                        (&raw mut byte).cast::<libc::c_void>(),
                        core::mem::size_of::<u8>(),
                    )
                };
                if rc == 1 {
                    return byte;
                }
                assert_eq!(rc, -1, "pipe read should either succeed or set errno");
                let errno = unsafe { *libc::__errno_location() };
                if errno == libc::EINTR {
                    continue;
                }
                panic!("pipe read should complete after readiness, errno={errno}");
            }
        }
    }

    #[cfg(feature = "std")]
    impl Drop for TestPipe {
        fn drop(&mut self) {
            unsafe {
                libc::close(self.read_fd);
                libc::close(self.write_fd);
            }
        }
    }

    #[cfg(feature = "std")]
    #[derive(Debug)]
    struct TestThreadNotify {
        thread: thread::Thread,
        notified: AtomicBool,
    }

    #[cfg(feature = "std")]
    impl Wake for TestThreadNotify {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.notified.store(true, Ordering::Release);
            self.thread.unpark();
        }
    }

    #[cfg(feature = "std")]
    fn test_block_on<F>(future: F) -> F::Output
    where
        F: Future,
    {
        let notify = Arc::new(TestThreadNotify {
            thread: thread::current(),
            notified: AtomicBool::new(false),
        });
        let waker = Waker::from(Arc::clone(&notify));
        let mut cx = Context::from_waker(&waker);
        let mut future = core::pin::pin!(future);
        loop {
            if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
                return output;
            }
            while !notify.notified.swap(false, Ordering::AcqRel) {
                thread::park();
            }
        }
    }

    #[test]
    fn registry_reuses_slots_with_new_generations() {
        let executor = Executor::new(ExecutorConfig::new());

        let first = executor
            .spawn(async { 7_u8 })
            .expect("first task should spawn");
        let first_slot = first.inner.slot_index;
        let first_generation = first.inner.generation;
        assert_eq!(first.join().expect("first task should finish"), 7);

        let second = executor
            .spawn(async { 9_u8 })
            .expect("second task should spawn");
        assert_eq!(second.inner.slot_index, first_slot);
        assert!(second.inner.generation > first_generation);
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
    fn async_yield_now_reschedules_current_thread_task() {
        let executor = Executor::new(ExecutorConfig::new());
        let polls = Arc::new(AtomicUsize::new(0));
        let task_polls = Arc::clone(&polls);

        let handle = executor
            .spawn(async move {
                task_polls.fetch_add(1, Ordering::AcqRel);
                async_yield_now().await;
                task_polls.fetch_add(1, Ordering::AcqRel);
                7_u8
            })
            .expect("task should spawn");

        assert!(executor.drive_once().expect("drive should succeed"));
        assert_eq!(polls.load(Ordering::Acquire), 1);
        assert!(!handle.is_finished().expect("task state should read"));

        assert!(executor.drive_once().expect("drive should succeed"));
        assert_eq!(polls.load(Ordering::Acquire), 2);
        assert_eq!(handle.join().expect("task should complete"), 7);
    }

    #[test]
    fn task_handle_reports_concrete_admission_layout() {
        let executor =
            Executor::new(ExecutorConfig::thread_pool().with_mode(ExecutorMode::CurrentThread));
        let sample = async { [1_u16, 2, 3, 4] };
        let handle = executor
            .spawn(async { [1_u16, 2, 3, 4] })
            .expect("task should spawn");
        let admission = handle.admission();
        assert_eq!(admission.carrier, ExecutorMode::CurrentThread);
        assert_eq!(admission.future_bytes, size_of_val(&sample));
        assert_eq!(admission.future_align, core::mem::align_of_val(&sample));
        assert_eq!(admission.output_bytes, size_of::<[u16; 4]>());
        assert_eq!(admission.output_align, align_of::<[u16; 4]>());
        assert_eq!(
            admission.poll_stack,
            AsyncPollStackContract::DerivedHeuristic { bytes: 512 }
        );
        assert_eq!(
            handle.join().expect("task should complete"),
            [1_u16, 2, 3, 4]
        );
    }

    #[test]
    fn task_handle_reports_storage_classes_and_poll_stack_contract() {
        let executor = Executor::new(ExecutorConfig::new());
        let sample_payload = [0_u8; 384];
        let sample = async move {
            let _ = sample_payload[0];
            [7_u8; 384]
        };
        assert!(size_of_val(&sample) > INLINE_ASYNC_FUTURE_BYTES);

        let payload = [0_u8; 384];
        let handle = executor
            .spawn_with_poll_stack_bytes(1536, async move {
                let _ = payload[0];
                [7_u8; 384]
            })
            .expect("task should spawn");
        let admission = handle.admission();
        assert_eq!(admission.future_storage_class, AsyncStorageClass::Medium);
        assert_eq!(admission.output_storage_class, AsyncStorageClass::Medium);
        assert_eq!(
            admission.poll_stack,
            AsyncPollStackContract::Explicit { bytes: 1536 }
        );
        assert_eq!(handle.join().expect("task should complete"), [7_u8; 384]);
    }

    #[test]
    fn generated_async_poll_stack_contract_overrides_default_heuristic() {
        let executor = Executor::new(ExecutorConfig::new());
        assert_eq!(
            generated_async_poll_stack_bytes_by_type_name(type_name::<
                GeneratedAsyncPollStackMetadataAnchorFuture,
            >()),
            Some(1536)
        );

        let handle = executor
            .spawn(GeneratedAsyncPollStackMetadataAnchorFuture)
            .expect("anchor future should spawn");
        assert_eq!(
            handle.admission().poll_stack,
            AsyncPollStackContract::Generated { bytes: 1536 }
        );
        handle.join().expect("anchor future should complete");
    }

    #[test]
    fn build_generated_async_poll_stack_trait_supports_spawn_generated() {
        let executor = Executor::new(ExecutorConfig::new());
        let handle = executor
            .spawn_generated(GeneratedAsyncPollStackMetadataAnchorFuture)
            .expect("generated anchor future should spawn through compile-time contract");
        assert_eq!(
            handle.admission().poll_stack,
            AsyncPollStackContract::Explicit { bytes: 1536 }
        );
        handle.join().expect("anchor future should complete");
    }

    #[test]
    fn classed_future_storage_derives_one_poll_stack_heuristic_by_default() {
        let executor = Executor::new(ExecutorConfig::new());
        let payload = [0_u8; 384];
        let handle = executor
            .spawn(async move {
                let _ = payload[0];
                5_u8
            })
            .expect("task should spawn");
        assert_eq!(
            handle.admission().future_storage_class,
            AsyncStorageClass::Medium
        );
        assert_eq!(
            handle.admission().poll_stack,
            AsyncPollStackContract::DerivedHeuristic { bytes: 1024 }
        );
        assert_eq!(handle.join().expect("task should complete"), 5);
    }

    #[test]
    fn run_until_idle_drains_ready_current_thread_tasks() {
        let executor = Executor::new(ExecutorConfig::new());
        let handle = executor
            .spawn(async {
                async_yield_now().await;
                async_yield_now().await;
                11_u8
            })
            .expect("task should spawn");

        assert_eq!(executor.run_until_idle().expect("drain should succeed"), 3);
        assert!(handle.is_finished().expect("task state should read"));
        assert_eq!(handle.join().expect("task should complete"), 11);
    }

    #[test]
    fn classed_future_storage_accepts_medium_future_frames() {
        let executor = Executor::new(ExecutorConfig::new());
        let sample_payload = [0_u8; 384];
        let sample = async move { sample_payload.len() };
        assert!(size_of_val(&sample) > INLINE_ASYNC_FUTURE_BYTES);

        let payload = [0_u8; 384];
        let handle = executor
            .spawn(async move { payload.len() })
            .expect("medium-sized future should spill into classed storage");

        assert_eq!(handle.join().expect("task should complete"), 384);
    }

    #[test]
    fn oversized_futures_are_rejected_honestly() {
        let executor = Executor::new(ExecutorConfig::new());
        let oversized = [0_u8; 2048];

        assert!(matches!(
            executor.spawn(async move { oversized.len() }),
            Err(ExecutorError::Unsupported)
        ));
    }

    #[test]
    fn classed_result_storage_accepts_medium_outputs() {
        let executor = Executor::new(ExecutorConfig::new());
        assert!(size_of::<[u8; 384]>() > INLINE_ASYNC_RESULT_BYTES);

        let handle = executor
            .spawn(async move { [7_u8; 384] })
            .expect("medium-sized outputs should spill into classed result storage");

        let output = handle.join().expect("task should complete");
        assert_eq!(output.len(), 384);
        assert!(output.iter().all(|byte| *byte == 7));
    }

    #[test]
    fn oversized_results_are_rejected_honestly() {
        let executor = Executor::new(ExecutorConfig::new());

        assert!(matches!(
            executor.spawn(async move { [0_u8; 2048] }),
            Err(ExecutorError::Unsupported)
        ));
    }

    #[test]
    fn dropping_executor_shuts_down_live_pending_slots() {
        let executor = Executor::new(ExecutorConfig::new());
        let handle = executor
            .spawn(core::future::pending::<u8>())
            .expect("pending task should spawn");
        let slot_index = handle.inner.slot_index;
        let generation = handle.inner.generation;
        let core = handle
            .inner
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
                .with_ref(Option::is_none)
                .expect("slot core access should succeed")
        );
        assert!(slot.waker.core_ptr().is_null());
        assert!(matches!(handle.join(), Err(ExecutorError::Stopped)));
        assert_eq!(slot.generation(), generation);
    }

    #[cfg(feature = "std")]
    #[test]
    fn executor_binds_to_hosted_fiber_runtime() {
        let runtime = HostedFiberRuntime::fixed(2).expect("hosted fiber runtime should build");
        let executor = Executor::new(ExecutorConfig::green_pool())
            .on_hosted_fibers(&runtime)
            .expect("executor should bind to hosted fibers");
        assert_eq!(executor.mode(), ExecutorMode::GreenPool);
        drop(executor);
        drop(runtime);
    }

    #[cfg(feature = "std")]
    #[test]
    fn executor_runs_on_thread_pool_carriers() {
        let pool = ThreadPool::new(&ThreadPoolConfig {
            min_threads: 1,
            max_threads: 1,
            ..ThreadPoolConfig::new()
        })
        .expect("thread pool should build");
        let executor = Executor::new(ExecutorConfig::thread_pool())
            .on_pool(&pool)
            .expect("executor should bind to thread pool");

        let handle = executor
            .spawn(async {
                async_yield_now().await;
                21_u8
            })
            .expect("task should spawn");

        assert_eq!(handle.join().expect("task should complete"), 21);
    }

    #[test]
    fn current_async_runtime_drives_async_fn_to_completion() {
        async fn value() -> u8 {
            async_yield_now().await;
            34
        }

        let runtime = CurrentAsyncRuntime::new();
        let handle = runtime.spawn(value()).expect("task should spawn");
        assert_eq!(runtime.run_until_idle().expect("runtime should drain"), 2);
        assert_eq!(handle.join().expect("task should complete"), 34);
    }

    #[test]
    fn task_handle_is_awaitable_on_current_runtime() {
        let runtime = CurrentAsyncRuntime::new();
        let handle = runtime
            .spawn(async {
                async_yield_now().await;
                13_u8
            })
            .expect("task should spawn");
        let result = runtime
            .block_on(handle)
            .expect("runtime should drive task join");
        assert_eq!(result.expect("task should complete"), 13);
    }

    #[test]
    fn current_runtime_spawn_with_poll_stack_bytes_preserves_contract() {
        let runtime = CurrentAsyncRuntime::new();
        let handle = runtime
            .spawn_with_poll_stack_bytes(2048, async { 9_u8 })
            .expect("task should spawn");
        assert_eq!(
            handle.admission().poll_stack,
            AsyncPollStackContract::Explicit { bytes: 2048 }
        );
        assert_eq!(handle.join().expect("task should complete"), 9);
    }

    #[test]
    fn current_runtime_spawn_generated_preserves_contract() {
        let runtime = CurrentAsyncRuntime::new();
        let handle = runtime
            .spawn_generated(ExplicitGeneratedPollStackFuture)
            .expect("generated-contract task should spawn");
        assert_eq!(
            handle.admission().poll_stack,
            AsyncPollStackContract::Explicit { bytes: 1792 }
        );
        assert_eq!(handle.join().expect("task should complete"), 55);
    }

    #[test]
    fn current_runtime_spawn_local_accepts_non_send_future() {
        use std::rc::Rc;

        let runtime = CurrentAsyncRuntime::new();
        let local = Rc::new(5_u8);
        let handle = runtime
            .spawn_local({
                let local = Rc::clone(&local);
                async move {
                    async_yield_now().await;
                    *local + 2
                }
            })
            .expect("local task should spawn");
        let result = runtime
            .block_on(handle)
            .expect("runtime should drive local task join");
        assert_eq!(result.expect("local task should complete"), 7);
    }

    #[test]
    fn current_runtime_spawn_local_with_poll_stack_bytes_preserves_contract() {
        use std::rc::Rc;

        let runtime = CurrentAsyncRuntime::new();
        let local = Rc::new(3_u8);
        let handle = runtime
            .spawn_local_with_poll_stack_bytes(1024, {
                let local = Rc::clone(&local);
                async move { *local + 4 }
            })
            .expect("local task should spawn");
        assert_eq!(
            handle.admission().poll_stack,
            AsyncPollStackContract::Explicit { bytes: 1024 }
        );
        let result = runtime
            .block_on(handle)
            .expect("runtime should drive local task join");
        assert_eq!(result.expect("local task should complete"), 7);
    }

    #[test]
    fn current_runtime_spawn_local_generated_preserves_contract() {
        let runtime = CurrentAsyncRuntime::new();
        let handle = runtime
            .spawn_local_generated(ExplicitGeneratedPollStackFuture)
            .expect("generated-contract local task should spawn");
        assert_eq!(
            handle.admission().poll_stack,
            AsyncPollStackContract::Explicit { bytes: 1792 }
        );
        let result = runtime
            .block_on(handle)
            .expect("runtime should drive generated local task");
        assert_eq!(result.expect("local task should complete"), 55);
    }

    #[test]
    fn task_handle_abort_reports_cancelled() {
        let runtime = CurrentAsyncRuntime::new();
        let handle = runtime
            .spawn(async {
                async_yield_now().await;
                21_u8
            })
            .expect("task should spawn");
        handle.abort().expect("task should abort cleanly");
        let result = runtime
            .block_on(handle)
            .expect("runtime should drive cancelled task join");
        assert!(matches!(result, Err(ExecutorError::Cancelled)));
    }

    #[cfg(feature = "std")]
    #[test]
    fn current_runtime_waits_for_readiness() {
        let runtime = CurrentAsyncRuntime::new();
        let pipe = Arc::new(TestPipe::new());
        let handle = runtime
            .spawn({
                let pipe = Arc::clone(&pipe);
                async move {
                    let readiness = async_wait_for_readiness(
                        pipe.source(),
                        EventInterest::READABLE | EventInterest::ERROR | EventInterest::HANGUP,
                    )
                    .await
                    .expect("readiness wait should complete");
                    assert!(readiness.contains(EventReadiness::READABLE));
                    pipe.read_byte()
                }
            })
            .expect("task should spawn");

        assert!(
            runtime
                .drive_once()
                .expect("registration poll should succeed")
        );
        pipe.write_byte(37);
        assert_eq!(
            runtime
                .block_on(handle)
                .expect("runtime should drive readiness task")
                .expect("task should complete"),
            37
        );
    }

    #[cfg(feature = "std")]
    #[test]
    fn current_runtime_sleep_for_completes() {
        let runtime = CurrentAsyncRuntime::new();
        let handle = runtime
            .spawn(async {
                async_sleep_for(Duration::from_millis(1))
                    .await
                    .expect("sleep should complete");
                99_u8
            })
            .expect("task should spawn");

        assert_eq!(
            runtime
                .block_on(handle)
                .expect("runtime should drive timer task")
                .expect("task should complete"),
            99
        );
    }

    #[cfg(feature = "std")]
    #[test]
    fn current_runtime_sleep_until_instant_completes() {
        let runtime = CurrentAsyncRuntime::new();
        let clock = system_monotonic_time();
        let start = clock
            .now_instant()
            .expect("monotonic runtime instant should be readable");
        let deadline = clock
            .checked_add_duration(start, Duration::from_millis(1))
            .expect("deadline should fit");
        let handle = runtime
            .spawn(async move {
                async_sleep_until_instant(deadline)
                    .await
                    .expect("sleep-until should complete");
                41_u8
            })
            .expect("task should spawn");

        assert_eq!(
            runtime
                .block_on(handle)
                .expect("runtime should drive timer task")
                .expect("task should complete"),
            41
        );
    }

    #[cfg(feature = "std")]
    #[test]
    fn current_task_handle_join_drives_timer_only_waits() {
        let executor = Executor::new_fast_current();
        let handle = executor
            .spawn(async {
                async_sleep_for(Duration::from_millis(1))
                    .await
                    .expect("sleep should complete");
                73_u8
            })
            .expect("task should spawn");

        assert_eq!(handle.join().expect("timer-only join should complete"), 73);
    }

    #[cfg(feature = "std")]
    #[test]
    fn thread_async_runtime_runs_async_fn() {
        async fn value() -> u8 {
            async_yield_now().await;
            55
        }

        let runtime = ThreadAsyncRuntime::new(&ThreadPoolConfig {
            min_threads: 1,
            max_threads: 1,
            ..ThreadPoolConfig::new()
        })
        .expect("thread async runtime should build");
        let handle = runtime.spawn(value()).expect("task should spawn");
        assert_eq!(handle.join().expect("task should complete"), 55);
    }

    #[cfg(feature = "std")]
    #[test]
    fn thread_async_runtime_defaults_to_direct_hosted_workers() {
        let runtime = ThreadAsyncRuntime::new(&ThreadPoolConfig {
            min_threads: 1,
            max_threads: 1,
            ..ThreadPoolConfig::new()
        })
        .expect("thread async runtime should build");

        assert_eq!(
            runtime.bootstrap(),
            ThreadAsyncRuntimeBootstrap::DirectHostedWorkers
        );
        assert!(runtime.thread_pool().is_none());
    }

    #[cfg(feature = "std")]
    #[test]
    fn thread_async_runtime_spawn_generated_preserves_contract() {
        let runtime = ThreadAsyncRuntime::new(&ThreadPoolConfig {
            min_threads: 1,
            max_threads: 1,
            ..ThreadPoolConfig::new()
        })
        .expect("thread async runtime should build");
        let handle = runtime
            .spawn_generated(ExplicitGeneratedPollStackFuture)
            .expect("generated-contract task should spawn");
        assert_eq!(
            handle.admission().poll_stack,
            AsyncPollStackContract::Explicit { bytes: 1792 }
        );
        assert_eq!(handle.join().expect("task should complete"), 55);
    }

    #[cfg(feature = "std")]
    #[test]
    fn thread_runtime_block_on_awaits_task_handles() {
        let runtime = ThreadAsyncRuntime::new(&ThreadPoolConfig {
            min_threads: 1,
            max_threads: 1,
            ..ThreadPoolConfig::new()
        })
        .expect("thread async runtime should build");
        let first = runtime
            .spawn(async {
                async_yield_now().await;
                13_u8
            })
            .expect("first task should spawn");
        let second = runtime
            .spawn(async {
                async_yield_now().await;
                21_u8
            })
            .expect("second task should spawn");

        let sum = runtime
            .block_on(async move {
                let first = first.await?;
                let second = second.await?;
                Ok::<u8, ExecutorError>(first + second)
            })
            .expect("runtime should drive awaitable task handles")
            .expect("task handles should complete");

        assert_eq!(sum, 34);
    }

    #[cfg(feature = "std")]
    #[test]
    fn thread_async_runtime_falls_back_to_composed_thread_pool_for_non_inherit_placement() {
        let cpu = ThreadLogicalCpuId {
            group: ThreadProcessorGroupId(0),
            index: 0,
        };
        let runtime = ThreadAsyncRuntime::new(&ThreadPoolConfig {
            min_threads: 1,
            max_threads: 1,
            placement: PoolPlacement::Static(core::slice::from_ref(&cpu)),
            ..ThreadPoolConfig::new()
        })
        .expect("thread async runtime should build");

        assert_eq!(
            runtime.bootstrap(),
            ThreadAsyncRuntimeBootstrap::ComposedThreadPool
        );
        assert!(runtime.thread_pool().is_some());
    }

    #[cfg(feature = "std")]
    #[test]
    fn current_runtime_executor_capacity_can_be_shaped_explicitly() {
        let runtime =
            CurrentAsyncRuntime::with_executor_config(ExecutorConfig::new().with_capacity(1));
        let _first = runtime
            .spawn(async {
                core::future::pending::<()>().await;
            })
            .expect("first task should fit in one-slot runtime");

        assert_eq!(
            runtime
                .spawn(async { 1_u8 })
                .expect_err("second task should exhaust one-slot runtime"),
            executor_busy()
        );
    }

    #[test]
    fn current_runtime_from_explicit_backing_runs_task() {
        let config = ExecutorConfig::new().with_capacity(2);
        let plan = CurrentAsyncRuntime::backing_plan(config).expect("backing plan should build");
        assert!(plan.control.bytes >= size_of::<ExecutorCore>());
        let backing = current_async_runtime_virtual_backing(config)
            .expect("virtual backing should build for hosted tests");
        let runtime = CurrentAsyncRuntime::from_backing(config, backing)
            .expect("runtime should build from explicit backing");
        let handle = runtime
            .spawn(async {
                async_yield_now().await;
                29_u8
            })
            .expect("task should spawn");

        assert_eq!(
            runtime
                .block_on(handle)
                .expect("runtime should drive explicit-backed task")
                .expect("task should complete"),
            29
        );
    }

    #[test]
    fn global_nearest_round_up_executor_sizing_inflates_backing_requests() {
        let exact = CurrentAsyncRuntime::backing_plan(ExecutorConfig::new().with_capacity(2))
            .expect("exact backing plan should build");
        let rounded = CurrentAsyncRuntime::backing_plan(
            ExecutorConfig::new()
                .with_capacity(2)
                .with_sizing_strategy(RuntimeSizingStrategy::GlobalNearestRoundUp),
        )
        .expect("rounded backing plan should build");

        assert!(rounded.control.bytes >= exact.control.bytes);
        assert!(rounded.reactor.bytes >= exact.reactor.bytes);
        assert!(rounded.registry.bytes >= exact.registry.bytes);
        assert!(rounded.future_medium.bytes >= exact.future_medium.bytes);
        assert!(rounded.future_large.bytes >= exact.future_large.bytes);
        assert!(rounded.result_medium.bytes >= exact.result_medium.bytes);
        assert!(rounded.result_large.bytes >= exact.result_large.bytes);
        assert!(rounded.control.bytes.is_power_of_two());
        assert!(rounded.reactor.bytes.is_power_of_two());
        assert!(rounded.registry.bytes.is_power_of_two());
    }

    #[test]
    fn global_nearest_round_up_executor_internal_virtual_backing_uses_rounded_sizes() {
        let exact = current_async_runtime_virtual_backing(ExecutorConfig::new().with_capacity(2))
            .expect("exact virtual backing should build");
        let rounded = current_async_runtime_virtual_backing(
            ExecutorConfig::new()
                .with_capacity(2)
                .with_sizing_strategy(RuntimeSizingStrategy::GlobalNearestRoundUp),
        )
        .expect("rounded virtual backing should build");

        assert!(rounded.control.view().len() >= exact.control.view().len());
        assert!(rounded.reactor.view().len() >= exact.reactor.view().len());
        assert!(rounded.registry.view().len() >= exact.registry.view().len());
        assert!(
            rounded
                .future_medium
                .as_ref()
                .expect("medium future backing should exist")
                .view()
                .len()
                >= exact
                    .future_medium
                    .as_ref()
                    .expect("medium future backing should exist")
                    .view()
                    .len()
        );
    }

    #[test]
    fn current_runtime_from_bound_slab_runs_task() {
        let config = ExecutorConfig::new().with_capacity(2);
        let layout = CurrentAsyncRuntime::backing_plan(config)
            .expect("backing plan should build")
            .combined()
            .expect("combined layout should build");
        let slab = aligned_bound_resource(layout.slab.bytes, layout.slab.align);
        let runtime = CurrentAsyncRuntime::from_bound_slab(config, slab)
            .expect("runtime should build from one bound slab");
        let handle = runtime
            .spawn(async {
                async_yield_now().await;
                31_u8
            })
            .expect("task should spawn");

        assert_eq!(
            runtime
                .block_on(handle)
                .expect("runtime should drive bound-slab task")
                .expect("task should complete"),
            31
        );
    }

    #[test]
    fn current_runtime_from_exact_aligned_bound_slab_runs_task() {
        let config = ExecutorConfig::new().with_capacity(2);
        let conservative = CurrentAsyncRuntime::backing_plan(config)
            .expect("backing plan should build")
            .combined_eager()
            .expect("conservative layout should build");
        let exact = CurrentAsyncRuntime::backing_plan(config)
            .expect("backing plan should build")
            .combined_eager_for_base_alignment(conservative.slab.align)
            .expect("exact-aligned layout should build");
        let slab = aligned_bound_resource(exact.slab.bytes, exact.slab.align);
        let runtime = CurrentAsyncRuntime::from_bound_slab(config, slab)
            .expect("runtime should build from exact-aligned slab");
        let handle = runtime
            .spawn(async {
                async_yield_now().await;
                37_u8
            })
            .expect("task should spawn");

        assert_eq!(
            runtime
                .block_on(handle)
                .expect("runtime should drive exact-aligned bound-slab task")
                .expect("task should complete"),
            37
        );
    }

    #[cfg(feature = "std")]
    #[test]
    fn thread_async_runtime_executor_capacity_can_be_shaped_explicitly() {
        let runtime = ThreadAsyncRuntime::with_executor_config(
            &ThreadPoolConfig {
                min_threads: 1,
                max_threads: 1,
                ..ThreadPoolConfig::new()
            },
            ExecutorConfig::thread_pool().with_capacity(1),
        )
        .expect("thread async runtime should build");
        let _first = runtime
            .spawn(async {
                core::future::pending::<()>().await;
            })
            .expect("first task should fit in one-slot runtime");

        assert_eq!(
            runtime
                .spawn(async { 2_u8 })
                .expect_err("second task should exhaust one-slot runtime"),
            executor_busy()
        );
    }

    #[cfg(feature = "std")]
    #[test]
    fn thread_async_runtime_repeated_create_drop_stays_alive() {
        for _ in 0..64 {
            let runtime = ThreadAsyncRuntime::new(&ThreadPoolConfig {
                min_threads: 1,
                max_threads: 1,
                ..ThreadPoolConfig::new()
            })
            .expect("thread async runtime should build");
            let handle = runtime
                .spawn(async {
                    async_yield_now().await;
                    8_u8
                })
                .expect("task should spawn");
            assert_eq!(handle.join().expect("task should complete"), 8);
            drop(runtime);
        }
    }

    #[cfg(feature = "std")]
    #[test]
    fn thread_async_runtime_repeated_warm_yield_batches_stay_alive_multi_worker() {
        const TASKS: usize = 16;

        let runtime = ThreadAsyncRuntime::with_executor_config(
            &ThreadPoolConfig {
                min_threads: 2,
                max_threads: 2,
                ..ThreadPoolConfig::new()
            },
            ExecutorConfig::thread_pool().with_capacity(TASKS),
        )
        .expect("thread async runtime should build");

        for iteration in 0..64 {
            let mut handles = Vec::with_capacity(TASKS);
            for task_index in 0..TASKS {
                let handle = match runtime.spawn(async {
                    async_yield_now().await;
                }) {
                    Ok(handle) => handle,
                    Err(error) => {
                        let core = runtime
                            .executor()
                            .core()
                            .expect("runtime executor should stay bound");
                        let registry = core.registry().expect("registry should stay available");
                        let free_len = registry
                            .free
                            .with_ref(|free| free.len)
                            .expect("free stack access should succeed");
                        let run_states: Vec<u8> = registry
                            .slots
                            .iter()
                            .map(|slot| slot.run_state.load(Ordering::Acquire))
                            .collect();
                        let states: Vec<u8> =
                            registry.slots.iter().map(|slot| slot.state()).collect();
                        panic!(
                            "yield-once task should spawn at iteration={iteration} task={task_index}: {error:?}; free_len={free_len}; states={states:?}; run_states={run_states:?}"
                        );
                    }
                };
                handles.push(handle);
            }

            test_block_on(async move {
                while let Some(handle) = handles.pop() {
                    handle.await.expect("yield-once task should complete");
                }
            });
        }
    }

    #[cfg(feature = "std")]
    #[test]
    fn thread_async_runtime_waits_for_readiness() {
        let runtime = ThreadAsyncRuntime::new(&ThreadPoolConfig {
            min_threads: 1,
            max_threads: 1,
            ..ThreadPoolConfig::new()
        })
        .expect("thread async runtime should build");
        let pipe = Arc::new(TestPipe::new());
        let handle = runtime
            .spawn({
                let pipe = Arc::clone(&pipe);
                async move {
                    let readiness = async_wait_for_readiness(
                        pipe.source(),
                        EventInterest::READABLE | EventInterest::ERROR | EventInterest::HANGUP,
                    )
                    .await
                    .expect("readiness wait should complete");
                    assert!(readiness.contains(EventReadiness::READABLE));
                    pipe.read_byte()
                }
            })
            .expect("task should spawn");

        thread::sleep(Duration::from_millis(1));
        pipe.write_byte(12);
        assert_eq!(handle.join().expect("task should complete"), 12);
    }

    #[cfg(feature = "std")]
    #[test]
    fn thread_async_runtime_sleep_for_completes() {
        let runtime = ThreadAsyncRuntime::new(&ThreadPoolConfig {
            min_threads: 1,
            max_threads: 1,
            ..ThreadPoolConfig::new()
        })
        .expect("thread async runtime should build");
        let handle = runtime
            .spawn(async {
                async_sleep_for(Duration::from_millis(1))
                    .await
                    .expect("sleep should complete");
                13_u8
            })
            .expect("task should spawn");
        assert_eq!(handle.join().expect("task should complete"), 13);
    }

    #[cfg(feature = "std")]
    #[test]
    fn fiber_async_runtime_binds_owned_hosted_fibers() {
        let hosted = HostedFiberRuntime::fixed(2).expect("hosted fiber runtime should build");
        let runtime =
            FiberAsyncRuntime::from_hosted_fibers(hosted).expect("fiber async runtime should bind");
        assert_eq!(runtime.executor().mode(), ExecutorMode::GreenPool);
        drop(runtime);
    }

    #[cfg(feature = "std")]
    #[test]
    fn fiber_async_runtime_spawn_generated_preserves_contract() {
        let runtime = FiberAsyncRuntime::fixed(2).expect("fiber async runtime should build");
        let handle = runtime
            .spawn_generated(ExplicitGeneratedPollStackFuture)
            .expect("generated-contract task should spawn");
        assert_eq!(
            handle.admission().poll_stack,
            AsyncPollStackContract::Explicit { bytes: 1792 }
        );
        assert_eq!(handle.join().expect("task should complete"), 55);
    }

    #[cfg(feature = "std")]
    #[test]
    fn fiber_async_runtime_repeated_create_drop_stays_alive() {
        for _ in 0..32 {
            let runtime = FiberAsyncRuntime::fixed(2).expect("fiber async runtime should build");
            let handle = runtime
                .spawn(async {
                    async_yield_now().await;
                    6_u8
                })
                .expect("task should spawn");
            assert_eq!(handle.join().expect("task should complete"), 6);
            drop(runtime);
        }
    }

    #[cfg(feature = "std")]
    #[test]
    fn fiber_async_runtime_sleep_for_completes() {
        let runtime = FiberAsyncRuntime::fixed(2).expect("fiber async runtime should build");
        let handle = runtime
            .spawn(async { async_sleep_for(Duration::from_millis(1)).await })
            .expect("task should spawn");
        assert!(matches!(handle.join(), Ok(Err(ExecutorError::Unsupported))));
    }

    #[cfg(feature = "std")]
    #[test]
    fn fiber_async_runtime_waits_for_readiness() {
        let runtime = FiberAsyncRuntime::fixed(2).expect("fiber async runtime should build");
        let pipe = Arc::new(TestPipe::new());
        let handle = runtime
            .spawn({
                let pipe = Arc::clone(&pipe);
                async move {
                    async_wait_for_readiness(
                        pipe.source(),
                        EventInterest::READABLE | EventInterest::ERROR | EventInterest::HANGUP,
                    )
                    .await
                }
            })
            .expect("task should spawn");

        thread::sleep(Duration::from_millis(1));
        pipe.write_byte(19);
        assert!(matches!(handle.join(), Ok(Err(ExecutorError::Unsupported))));
    }
}
