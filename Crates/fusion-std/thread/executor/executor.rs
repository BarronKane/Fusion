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
//! let handle = match executor.spawn_with_poll_stack_bytes(1024, async { 5_u8 }) {
//!     Ok(handle) => handle,
//!     Err(_) => return,
//! };
//! assert_eq!(handle.join(), Ok(5));
//! # }
//! # demo();
//! ```

use ::core::any::{
    TypeId,
    type_name,
};
use ::core::array;
use ::core::cell::UnsafeCell;
use ::core::fmt;
use ::core::future::Future;
use ::core::hint::spin_loop;
use ::core::marker::PhantomData;
use ::core::mem::{
    align_of,
    size_of,
};
use ::core::num::NonZeroUsize;
use ::core::pin::{
    Pin,
    pin,
};
use ::core::ptr::NonNull;
use ::core::sync::atomic::{
    AtomicBool,
    AtomicU8,
    AtomicUsize,
    Ordering,
};
use ::core::task::{
    Context,
    Poll,
    RawWaker,
    RawWakerVTable,
    Waker,
};
use ::core::time::Duration;

#[cfg(feature = "std")]
use std::string::String;
#[cfg(feature = "std")]
use std::sync::Arc;
#[cfg(feature = "std")]
use std::thread::{
    Builder as StdThreadBuilder,
    JoinHandle,
};
#[cfg(feature = "std")]
use std::vec::Vec;

#[cfg(feature = "std")]
use fusion_pal::sys::fiber::{
    PlatformFiberWakeSignal,
    system_fiber_host,
};
use fusion_pal::sys::mem::MemBaseContract;
use fusion_sys::alloc::{
    AllocError,
    AllocErrorKind,
    Allocator,
    ArenaInitError,
    ArenaSlice,
    BoundedArena,
    ControlLease,
    ExtentLease,
    MemoryPoolExtentRequest,
};
use fusion_sys::channel::ChannelError;
#[cfg(feature = "debug-insights")]
use fusion_sys::channel::ChannelReceiveContract;
use fusion_sys::domain::context::ContextId;
use fusion_sys::courier::{
    CourierId,
    CourierLaneSummary,
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
use fusion_sys::fiber::{
    FiberError,
    FiberErrorKind,
};
use fusion_sys::channel::insight::{
    InsightCaptureMode,
    InsightChannelClass,
    InsightSupport,
    LocalInsightChannel,
};
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
use fusion_sys::thread::{
    CarrierSpawnLocalityPolicy,
    CanonicalInstant,
    MonotonicRawInstant,
    system_monotonic_time,
    system_thread,
};
#[cfg(feature = "debug-insights")]
use fusion_sys::transport::TransportAttachmentControlContract;
use fusion_sys::transport::{
    TransportAttachmentRequest,
    TransportError,
};

use crate::sync::{
    Mutex as SyncMutex,
    Semaphore,
    SyncError,
    SyncErrorKind,
};
#[cfg(feature = "std")]
use super::HostedFiberRuntime;
use super::{
    ExplicitFiberTask,
    FiberTaskAttributes,
    GreenPool,
    RuntimeSizingStrategy,
    ThreadPool,
    default_runtime_sizing_strategy,
    ensure_runtime_reserved_wake_vectors_best_effort,
    yield_now as green_yield_now,
};

mod engine;
use self::engine::*;

#[cfg(feature = "std")]
mod hosted;
#[cfg(feature = "std")]
pub use self::hosted::{
    FiberAsyncRuntime,
    ThreadAsyncRuntime,
    ThreadAsyncRuntimeBootstrap,
};
#[cfg(feature = "std")]
use self::hosted::executor_error_from_fiber_host;
#[cfg(all(feature = "std", test))]
use self::hosted::hosted_green_executor_stack_size;

const GREEN_EXECUTOR_DISPATCH_STACK_BYTES: NonZeroUsize =
    NonZeroUsize::new(16 * 1024).expect("green executor dispatch stack must be non-zero");

struct GreenExecutorDispatchTask {
    core: ControlLease<ExecutorCore>,
    slot_index: usize,
    generation: u64,
}

impl ExplicitFiberTask for GreenExecutorDispatchTask {
    type Output = ();

    const STACK_BYTES: NonZeroUsize = GREEN_EXECUTOR_DISPATCH_STACK_BYTES;

    fn run(self) -> Self::Output {
        run_scheduled_green_slot_lease(self.core, self.slot_index, self.generation);
    }
}

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
    /// Concrete output storage size in bytes.
    pub output_bytes: usize,
    /// Concrete output storage alignment in bytes.
    pub output_align: usize,
    /// Exact task-backing bytes required to host the future frame and eventual output over the
    /// task lifecycle without coarse storage classes.
    pub exact_backing_bytes: usize,
    /// Exact task-backing alignment required across the future/output lifecycle.
    pub exact_backing_align: usize,
    /// Distinct poll-stack contract carried alongside the future frame layout.
    pub poll_stack: AsyncPollStackContract,
}

impl AsyncTaskAdmission {
    fn for_future<F>(carrier: ExecutorMode) -> Self
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        let exact_backing_bytes = if size_of::<F>() >= size_of::<F::Output>() {
            size_of::<F>()
        } else {
            size_of::<F::Output>()
        };
        let exact_backing_align = if align_of::<F>() >= align_of::<F::Output>() {
            align_of::<F>()
        } else {
            align_of::<F::Output>()
        };
        let poll_stack =
            generated_async_poll_stack_contract::<F>().unwrap_or(AsyncPollStackContract::Unknown);
        Self {
            carrier,
            future_bytes: size_of::<F>(),
            future_align: align_of::<F>(),
            output_bytes: size_of::<F::Output>(),
            output_align: align_of::<F::Output>(),
            exact_backing_bytes,
            exact_backing_align,
            poll_stack,
        }
    }

    const fn with_poll_stack_bytes(mut self, bytes: usize) -> Self {
        self.poll_stack = AsyncPollStackContract::from_bytes(bytes);
        self
    }
}

/// Separate poll-stack contract for one async task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AsyncPollStackContract {
    /// No honest poll-stack bound has been attached.
    Unknown,
    /// One build-generated poll-stack budget was emitted for this exact future type.
    Generated { bytes: usize },
    /// One explicit poll-stack byte budget was attached to the task admission.
    Explicit { bytes: usize },
}

impl AsyncPollStackContract {
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
    let mut future = pin!(GeneratedAsyncPollStackMetadataAnchorFuture);
    matches!(
        generated_async_poll_stack_root(future.as_mut(), &mut context),
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
    // TODO: unnamed async blocks and other anonymous future types remain a metadata blind spot
    // across crate boundaries under the current sidecar bridge. Rejecting `Unknown` is the honest
    // path for now; the real fix is toolchain-owned future metadata instead of heuristic cosplay.
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

    fn extent(&self, request: MemoryPoolExtentRequest) -> Result<ExtentLease, ExecutorError> {
        self.allocator
            .extent(self.domain, request)
            .map_err(executor_error_from_alloc)
    }
}

#[derive(Debug)]
struct ExecutorBackingAllocators {
    control: ExecutorDomainAllocator,
    reactor: ExecutorDomainAllocator,
    registry: ExecutorDomainAllocator,
    spill: Option<ExecutorDomainAllocator>,
}

impl ExecutorBackingAllocators {
    fn acquire_current(config: ExecutorConfig) -> Result<Self, ExecutorError> {
        Self::from_current_backing(current_async_runtime_virtual_backing(config)?)
    }

    fn from_current_backing(backing: CurrentAsyncRuntimeBacking) -> Result<Self, ExecutorError> {
        let CurrentAsyncRuntimeBacking {
            control,
            reactor,
            registry,
            spill,
            slab_owner: _,
        } = backing;
        Ok(Self {
            control: ExecutorDomainAllocator::from_resource(control)?,
            reactor: ExecutorDomainAllocator::from_resource(reactor)?,
            registry: ExecutorDomainAllocator::from_resource(registry)?,
            spill: spill
                .map(ExecutorDomainAllocator::from_resource)
                .transpose()?,
        })
    }
}

const fn default_async_spill_align() -> usize {
    1usize << DEFAULT_ASYNC_SPILL_BUDGET_PER_TASK_BYTES.trailing_zeros()
}

const fn executor_exact_backing_len(bytes: usize) -> usize {
    if bytes == 0 { 1 } else { bytes }
}

fn executor_async_spill_capacity_bytes(capacity: usize) -> Result<usize, ExecutorError> {
    capacity
        .checked_mul(DEFAULT_ASYNC_SPILL_BUDGET_PER_TASK_BYTES)
        .ok_or_else(executor_overflow)
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

include!("current.rs");

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
    /// Locality preference when admitting new async work onto external carriers.
    pub spawn_locality_policy: CarrierSpawnLocalityPolicy,
    /// Sizing strategy applied to executor-owned backing plans.
    pub sizing: RuntimeSizingStrategy,
    /// Optional owning courier identity carried by this runtime for self-query surfaces.
    pub courier_id: Option<CourierId>,
    /// Optional owning context identity carried by this runtime for self-query surfaces.
    pub context_id: Option<ContextId>,
    /// Optional courier-runtime sink used to publish authoritative runtime truth.
    pub runtime_sink: Option<CourierRuntimeSink>,
}

impl ExecutorConfig {
    /// Returns a current-thread executor configuration.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            mode: ExecutorMode::CurrentThread,
            reactor: ReactorConfig::new(),
            capacity: TASK_REGISTRY_CAPACITY,
            spawn_locality_policy: CarrierSpawnLocalityPolicy::SameCore,
            sizing: default_runtime_sizing_strategy(),
            courier_id: None,
            context_id: None,
            runtime_sink: None,
        }
    }

    /// Returns one thread-pool executor configuration.
    #[must_use]
    pub const fn thread_pool() -> Self {
        Self {
            mode: ExecutorMode::ThreadPool,
            reactor: ReactorConfig::new(),
            capacity: TASK_REGISTRY_CAPACITY,
            spawn_locality_policy: CarrierSpawnLocalityPolicy::SameCore,
            sizing: default_runtime_sizing_strategy(),
            courier_id: None,
            context_id: None,
            runtime_sink: None,
        }
    }

    /// Returns one fiber-carrier executor configuration.
    #[must_use]
    pub const fn green_pool() -> Self {
        Self {
            mode: ExecutorMode::GreenPool,
            reactor: ReactorConfig::new(),
            capacity: TASK_REGISTRY_CAPACITY,
            spawn_locality_policy: CarrierSpawnLocalityPolicy::SameCore,
            sizing: default_runtime_sizing_strategy(),
            courier_id: None,
            context_id: None,
            runtime_sink: None,
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

    /// Returns one copy of this configuration with an explicit async spawn-locality policy.
    #[must_use]
    pub const fn with_spawn_locality_policy(
        mut self,
        spawn_locality_policy: CarrierSpawnLocalityPolicy,
    ) -> Self {
        self.spawn_locality_policy = spawn_locality_policy;
        self
    }

    /// Returns one copy of this configuration with an explicit sizing strategy.
    #[must_use]
    pub const fn with_sizing_strategy(mut self, sizing: RuntimeSizingStrategy) -> Self {
        self.sizing = sizing;
        self
    }

    /// Returns one copy of this configuration with an explicit owning courier identity.
    #[must_use]
    pub const fn with_courier_id(mut self, courier_id: CourierId) -> Self {
        self.courier_id = Some(courier_id);
        self
    }

    /// Returns one copy of this configuration with an explicit owning context identity.
    #[must_use]
    pub const fn with_context_id(mut self, context_id: ContextId) -> Self {
        self.context_id = Some(context_id);
        self
    }

    /// Returns one copy of this configuration with one explicit runtime-to-courier sink.
    #[must_use]
    pub const fn with_runtime_sink(mut self, runtime_sink: CourierRuntimeSink) -> Self {
        self.runtime_sink = Some(runtime_sink);
        self
    }
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self::new()
    }
}

include!("planning.rs");

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
        spill: Some(current_async_runtime_virtual_resource(
            plan.spill,
            "fusion-executor-current-spill",
        )?),
        slab_owner: None,
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
    /// Optional exact async spill-domain resource shared across future/result lifecycle envelopes.
    pub spill: Option<MemoryResourceHandle>,
    /// Optional owned slab retaining the backing lifetime for partitioned explicit resources.
    pub slab_owner: Option<ExtentLease>,
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

#[cfg(feature = "std")]
const CURRENT_QUEUE_CAPACITY: usize = 256;
const TASK_REGISTRY_CAPACITY: usize = 256;
const JOIN_SET_CAPACITY: usize = 64;
#[cfg(test)]
const INLINE_ASYNC_FUTURE_BYTES: usize = 256;
const DEFAULT_ASYNC_SPILL_BUDGET_PER_TASK_BYTES: usize = 1024;
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

include!("core.rs");

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

        let result = slot.take_result::<T>(&registry.spill_store, self.generation);
        self.active = false;
        let _ = self.core.detach_handle(self.slot_index, self.generation);
        result
    }

    fn abort(&self) -> Result<(), ExecutorError> {
        let slot = self.core.registry()?.slot(self.slot_index)?;
        let _ = self.core.clear_wait(self.slot_index, self.generation);
        let registry = self.core.registry()?;
        slot.cancel(&registry.spill_store, self.generation)?;
        #[cfg(feature = "debug-insights")]
        if let Some(task) = slot.task_id() {
            self.core
                .emit_task_lifecycle(AsyncTaskLifecycleRecord::Cancelled {
                    task,
                    slot_index: self.slot_index,
                    generation: self.generation,
                    scheduler: self.core.scheduler_tag(),
                });
        }
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
                let result = slot.take_result::<T>(&registry.spill_store, self.generation);
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

    /// Spawns a task through the supplied executor with one explicit poll-stack contract and
    /// tracks it in this join set.
    ///
    /// # Errors
    ///
    /// Returns any honest spawn failure, or `Overflow` if the join set is full.
    pub fn spawn_with_poll_stack_bytes<F>(
        &self,
        executor: &Executor,
        poll_stack_bytes: usize,
        future: F,
    ) -> Result<TaskId, ExecutorError>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let mut state = self.entries.lock().map_err(executor_error_from_sync)?;
        let entry_index = state.first_free_index().ok_or_else(executor_overflow)?;
        let handle = executor.spawn_with_poll_stack_bytes(poll_stack_bytes, future)?;
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

/// Current-thread async runtime using ordinary Rust futures as one manual/bootstrap front door.
///
/// This is not the final autonomous courier runtime model. It is the current-thread runner for
/// bootstrap, audit, and explicitly cooperative local execution.
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
    pub(crate) fn install_runtime_dispatch_cookie(
        &self,
        cookie: fusion_pal::sys::runtime_dispatch::RuntimeDispatchCookie,
    ) -> Result<(), ExecutorError> {
        self.executor
            .core()?
            .install_runtime_dispatch_cookie(cookie);
        Ok(())
    }

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
        ensure_runtime_reserved_wake_vectors_best_effort();
        Self {
            executor: Executor::new_fast_current(),
            _not_send_sync: PhantomData,
        }
    }

    /// Creates one current-thread async runtime with one explicit executor configuration.
    #[must_use]
    pub fn with_executor_config(config: ExecutorConfig) -> Self {
        ensure_runtime_reserved_wake_vectors_best_effort();
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
        ensure_runtime_reserved_wake_vectors_best_effort();
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
            spill: layout
                .spill
                .map(|range| partition_executor_bound_resource(&slab, range))
                .transpose()?,
            slab_owner: None,
        };
        Self::from_backing(config, backing)
    }

    pub(crate) fn from_owned_extent(
        config: ExecutorConfig,
        owned_backing: ExtentLease,
    ) -> Result<Self, ExecutorError> {
        let slab = MemoryResourceHandle::from(
            BoundMemoryResource::static_allocatable_region(owned_backing.region())
                .map_err(executor_error_from_resource)?,
        );
        let layout = Self::backing_plan_with_layout_policy(config, slab.info().layout)?
            .combined_eager_for_base_alignment(executor_resource_base_alignment(&slab))?;
        if slab.view().len() < layout.slab.bytes {
            return Err(ExecutorError::Sync(SyncErrorKind::Overflow));
        }
        let backing = CurrentAsyncRuntimeBacking {
            control: partition_executor_bound_resource(&slab, layout.control)?,
            reactor: partition_executor_bound_resource(&slab, layout.reactor)?,
            registry: partition_executor_bound_resource(&slab, layout.registry)?,
            spill: layout
                .spill
                .map(|range| partition_executor_bound_resource(&slab, range))
                .transpose()?,
            slab_owner: Some(owned_backing),
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

    /// Returns the consumer-facing async task lifecycle insight lane for this runtime.
    #[must_use]
    pub fn task_lifecycle_insight(&self) -> AsyncTaskLifecycleInsight<'_> {
        self.executor.task_lifecycle_insight()
    }

    /// Returns the executor configuration backing this current-thread runtime.
    #[must_use]
    pub const fn config(&self) -> ExecutorConfig {
        self.executor.config
    }

    /// Returns the owning courier identity for this runtime, when configured.
    #[must_use]
    pub const fn courier_id(&self) -> Option<CourierId> {
        self.executor.config.courier_id
    }

    /// Returns the owning context identity for this runtime, when configured.
    #[must_use]
    pub const fn context_id(&self) -> Option<ContextId> {
        self.executor.config.context_id
    }

    /// Returns the number of immediately available task slots in this runtime.
    ///
    /// # Errors
    ///
    /// Returns an error if the executor registry cannot be observed honestly.
    pub fn available_task_slots(&self) -> Result<usize, ExecutorError> {
        self.executor.available_task_slots()
    }

    /// Returns the number of unfinished tasks still owned by this runtime.
    ///
    /// Finished-but-unjoined tasks do not count here; this is the quiescence-facing view used by
    /// higher layers to decide whether a runtime can be replaced honestly.
    ///
    /// # Errors
    ///
    /// Returns an error if the executor registry cannot be observed honestly.
    pub fn unfinished_task_count(&self) -> Result<usize, ExecutorError> {
        self.executor.unfinished_task_count()
    }

    /// Returns a courier-facing run summary for this current-thread async lane.
    ///
    /// # Errors
    ///
    /// Returns an error if the executor registry cannot be observed honestly.
    pub fn runtime_summary(&self) -> Result<CourierRuntimeSummary, ExecutorError> {
        self.runtime_summary_with_responsiveness(CourierResponsiveness::Responsive)
    }

    /// Returns a courier-facing run summary for this current-thread async lane using one
    /// caller-supplied responsiveness classification.
    ///
    /// # Errors
    ///
    /// Returns an error if the executor registry cannot be observed honestly.
    pub fn runtime_summary_with_responsiveness(
        &self,
        responsiveness: CourierResponsiveness,
    ) -> Result<CourierRuntimeSummary, ExecutorError> {
        self.executor
            .runtime_summary_with_responsiveness(responsiveness)
    }

    /// Returns the exact configured backing plan for this runtime.
    ///
    /// This reports the honest current runtime shape under the selected planning surface. It is a
    /// configured footprint view, not a guess at which future/result slabs have become active at
    /// this exact instant.
    ///
    /// # Errors
    ///
    /// Returns any honest sizing or metadata-overflow failure while shaping the runtime domains.
    pub fn configured_backing_plan(&self) -> Result<CurrentAsyncRuntimeBackingPlan, ExecutorError> {
        Self::backing_plan(self.executor.config)
    }

    /// Returns the exact configured memory footprint for this runtime.
    ///
    /// # Errors
    ///
    /// Returns any honest sizing or metadata-overflow failure while shaping the runtime domains.
    pub fn configured_memory_footprint(
        &self,
    ) -> Result<AsyncRuntimeMemoryFootprint, ExecutorError> {
        Ok(self.configured_backing_plan()?.memory_footprint())
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

    /// Pumps one ready async task.
    ///
    /// # Errors
    ///
    /// Returns any honest executor failure.
    pub(crate) fn pump_once(&self) -> Result<bool, ExecutorError> {
        self.executor.pump_current_thread_once()
    }

    /// Drains the current-thread async queue until idle.
    ///
    /// # Errors
    ///
    /// Returns any honest executor failure.
    #[cfg(test)]
    pub(crate) fn drain_until_idle(&self) -> Result<usize, ExecutorError> {
        self.executor.drain_current_thread_until_idle()
    }

    pub(crate) fn drive_reactor_once(&self, wait: bool) -> Result<bool, ExecutorError> {
        let core = self.executor.core()?;
        let SchedulerBinding::Current = &core.scheduler else {
            return Err(ExecutorError::Unsupported);
        };
        core.drive_reactor_once(wait)
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

    /// Drives one future to completion on the current thread with one explicit poll-stack
    /// contract.
    ///
    /// # Errors
    ///
    /// Returns any honest executor failure.
    pub fn block_on_with_poll_stack_bytes<F>(
        &self,
        poll_stack_bytes: usize,
        future: F,
    ) -> Result<F::Output, ExecutorError>
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        self.executor
            .block_on_with_poll_stack_bytes(poll_stack_bytes, future)
    }
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
mod tests;
