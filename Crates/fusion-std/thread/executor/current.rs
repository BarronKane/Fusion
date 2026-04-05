pub(super) enum AsyncWaitOutcome {
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
    courier_id: Option<CourierId>,
    context_id: Option<ContextId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AsyncTaskSchedulerTag {
    Current = 1,
    ThreadWorkers = 2,
    GreenPool = 3,
    Unsupported = 4,
}

const ASYNC_TASK_LIFECYCLE_INSIGHT_CAPACITY: usize = 128;

/// One async task lifecycle record emitted by the executor insight lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AsyncTaskLifecycleRecord {
    Spawned {
        task: TaskId,
        slot_index: usize,
        generation: u64,
        scheduler: AsyncTaskSchedulerTag,
        admission: AsyncTaskAdmission,
    },
    PolledPending {
        task: TaskId,
        slot_index: usize,
        generation: u64,
        scheduler: AsyncTaskSchedulerTag,
    },
    PolledReady {
        task: TaskId,
        slot_index: usize,
        generation: u64,
        scheduler: AsyncTaskSchedulerTag,
    },
    Completed {
        task: TaskId,
        slot_index: usize,
        generation: u64,
        scheduler: AsyncTaskSchedulerTag,
    },
    Failed {
        task: TaskId,
        slot_index: usize,
        generation: u64,
        scheduler: AsyncTaskSchedulerTag,
        error: ExecutorError,
    },
    Cancelled {
        task: TaskId,
        slot_index: usize,
        generation: u64,
        scheduler: AsyncTaskSchedulerTag,
    },
}

/// ProtocolContract for async task lifecycle insight records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AsyncTaskLifecycleProtocol;

impl fusion_sys::transport::protocol::ProtocolContract for AsyncTaskLifecycleProtocol {
    type Message = AsyncTaskLifecycleRecord;

    const DESCRIPTOR: fusion_sys::transport::protocol::ProtocolDescriptor =
        fusion_sys::transport::protocol::ProtocolDescriptor {
            id: fusion_sys::transport::protocol::ProtocolId(0x4655_5349_4f4e_4153_594e_435f_544c_0001),
            version: fusion_sys::transport::protocol::ProtocolVersion::new(1, 0, 0),
            caps: fusion_sys::transport::protocol::ProtocolCaps::DEBUG_VIEW,
            bootstrap: fusion_sys::transport::protocol::ProtocolBootstrapKind::Immediate,
            debug_view: fusion_sys::transport::protocol::ProtocolDebugView::Structured,
            transport: fusion_sys::transport::protocol::ProtocolTransportRequirements::message_local(),
            implementation: fusion_sys::transport::protocol::ProtocolImplementationKind::Native,
        };
}

#[cfg(feature = "debug-insights")]
struct AsyncTaskLifecycleInsightState {
    channel: LocalInsightChannel<AsyncTaskLifecycleProtocol, ASYNC_TASK_LIFECYCLE_INSIGHT_CAPACITY>,
    producer: usize,
}

#[cfg(feature = "debug-insights")]
impl AsyncTaskLifecycleInsightState {
    fn new() -> Result<Self, ExecutorError> {
        let channel = LocalInsightChannel::<
            AsyncTaskLifecycleProtocol,
            ASYNC_TASK_LIFECYCLE_INSIGHT_CAPACITY,
        >::new(InsightChannelClass::Timeline, InsightCaptureMode::Lossy)
        .map_err(|_| ExecutorError::Unsupported)?;
        let producer = channel
            .attach_producer(TransportAttachmentRequest::same_courier())
            .map_err(|_| ExecutorError::Unsupported)?;
        Ok(Self { channel, producer })
    }

    fn emit_if_observed(&self, record: AsyncTaskLifecycleRecord) {
        let _ = self.channel.try_send_if_observed(self.producer, || record);
    }
}

/// Consumer-facing async task lifecycle insight view for one executor.
pub struct AsyncTaskLifecycleInsight<'a> {
    #[cfg_attr(not(feature = "debug-insights"), allow(dead_code))]
    core: Option<&'a ExecutorCore>,
}

impl<'a> AsyncTaskLifecycleInsight<'a> {
    /// Returns the configured support surface for async task lifecycle insight.
    #[must_use]
    pub const fn support(&self) -> InsightSupport {
        LocalInsightChannel::<
            AsyncTaskLifecycleProtocol,
            ASYNC_TASK_LIFECYCLE_INSIGHT_CAPACITY,
        >::configured_support(InsightChannelClass::Timeline, InsightCaptureMode::Lossy)
    }

    /// Returns `true` when one consumer is currently attached.
    #[must_use]
    pub fn is_observed(&self) -> bool {
        #[cfg(feature = "debug-insights")]
        {
            let Some(core) = self.core else {
                return false;
            };
            core.task_lifecycle
                .with_ref(|state| {
                    state
                        .as_ref()
                        .is_some_and(|state| state.channel.is_observed())
                })
                .unwrap_or(false)
        }
        #[cfg(not(feature = "debug-insights"))]
        {
            false
        }
    }

    /// Attaches one consumer to the async task lifecycle insight lane.
    pub fn attach_consumer(
        &self,
        request: TransportAttachmentRequest,
    ) -> Result<usize, TransportError> {
        #[cfg(feature = "debug-insights")]
        {
            let Some(core) = self.core else {
                return Err(TransportError::unsupported());
            };
            core.ensure_task_lifecycle_insight()
                .map_err(|_| TransportError::unsupported())?;
            core.task_lifecycle
                .with_ref(|state| {
                    state
                        .as_ref()
                        .ok_or_else(TransportError::unsupported)?
                        .channel
                        .attach_consumer(request)
                })
                .map_err(|_| TransportError::unsupported())?
        }
        #[cfg(not(feature = "debug-insights"))]
        {
            let _ = request;
            Err(TransportError::unsupported())
        }
    }

    /// Detaches one consumer from the async task lifecycle insight lane.
    pub fn detach_consumer(&self, consumer: usize) -> Result<(), TransportError> {
        #[cfg(feature = "debug-insights")]
        {
            let Some(core) = self.core else {
                return Err(TransportError::unsupported());
            };
            core.task_lifecycle
                .with_ref(|state| {
                    state
                        .as_ref()
                        .ok_or_else(TransportError::unsupported)?
                        .channel
                        .detach_consumer(consumer)
                })
                .map_err(|_| TransportError::unsupported())?
        }
        #[cfg(not(feature = "debug-insights"))]
        {
            let _ = consumer;
            Err(TransportError::unsupported())
        }
    }

    /// Receives one pending async task lifecycle record, if present.
    pub fn try_receive(
        &self,
        consumer: usize,
    ) -> Result<Option<AsyncTaskLifecycleRecord>, ChannelError> {
        #[cfg(feature = "debug-insights")]
        {
            let Some(core) = self.core else {
                return Err(ChannelError::unsupported());
            };
            core.task_lifecycle
                .with_ref(|state| {
                    state
                        .as_ref()
                        .ok_or_else(ChannelError::unsupported)?
                        .channel
                        .try_receive(consumer)
                })
                .map_err(|_| ChannelError::unsupported())?
        }
        #[cfg(not(feature = "debug-insights"))]
        {
            let _ = consumer;
            Err(ChannelError::unsupported())
        }
    }
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
#[cfg(feature = "std")]
#[thread_local]
static mut CURRENT_ASYNC_TASK_COURIER_STD: u64 = u64::MAX;
#[cfg(feature = "std")]
#[thread_local]
static mut CURRENT_ASYNC_TASK_CONTEXT_STD: u64 = u64::MAX;
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
#[cfg(not(feature = "std"))]
static CURRENT_ASYNC_TASK_COURIER: AtomicUsize = AtomicUsize::new(usize::MAX);
#[cfg(not(feature = "std"))]
static CURRENT_ASYNC_TASK_CONTEXT: AtomicUsize = AtomicUsize::new(usize::MAX);

#[cfg(feature = "std")]
const ASYNC_TASK_ID_SENTINEL_STD: u64 = u64::MAX;
#[cfg(not(feature = "std"))]
const ASYNC_TASK_ID_SENTINEL: usize = usize::MAX;

#[cfg(feature = "std")]
const fn encode_async_task_courier_id(id: Option<CourierId>) -> u64 {
    match id {
        Some(id) => id.get(),
        None => ASYNC_TASK_ID_SENTINEL_STD,
    }
}

#[cfg(feature = "std")]
const fn decode_async_task_courier_id(raw: u64) -> Option<CourierId> {
    if raw == ASYNC_TASK_ID_SENTINEL_STD {
        None
    } else {
        Some(CourierId::new(raw))
    }
}

#[cfg(feature = "std")]
const fn encode_async_task_context_id(id: Option<ContextId>) -> u64 {
    match id {
        Some(id) => id.get(),
        None => ASYNC_TASK_ID_SENTINEL_STD,
    }
}

#[cfg(feature = "std")]
const fn decode_async_task_context_id(raw: u64) -> Option<ContextId> {
    if raw == ASYNC_TASK_ID_SENTINEL_STD {
        None
    } else {
        Some(ContextId::new(raw))
    }
}

#[cfg(not(feature = "std"))]
fn encode_async_task_courier_id(id: Option<CourierId>) -> usize {
    match id {
        Some(id) => match usize::try_from(id.get()) {
            Ok(raw) => {
                debug_assert!(raw != ASYNC_TASK_ID_SENTINEL);
                raw
            }
            Err(_) => {
                debug_assert!(false, "courier id does not fit in async TLS usize slot");
                ASYNC_TASK_ID_SENTINEL - 1
            }
        },
        None => ASYNC_TASK_ID_SENTINEL,
    }
}

#[cfg(not(feature = "std"))]
fn decode_async_task_courier_id(raw: usize) -> Option<CourierId> {
    if raw == ASYNC_TASK_ID_SENTINEL {
        None
    } else {
        Some(CourierId::new(raw as u64))
    }
}

#[cfg(not(feature = "std"))]
fn encode_async_task_context_id(id: Option<ContextId>) -> usize {
    match id {
        Some(id) => match usize::try_from(id.get()) {
            Ok(raw) => {
                debug_assert!(raw != ASYNC_TASK_ID_SENTINEL);
                raw
            }
            Err(_) => {
                debug_assert!(false, "context id does not fit in async TLS usize slot");
                ASYNC_TASK_ID_SENTINEL - 1
            }
        },
        None => ASYNC_TASK_ID_SENTINEL,
    }
}

#[cfg(not(feature = "std"))]
fn decode_async_task_context_id(raw: usize) -> Option<ContextId> {
    if raw == ASYNC_TASK_ID_SENTINEL {
        None
    } else {
        Some(ContextId::new(raw as u64))
    }
}

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
            courier_id: decode_async_task_courier_id(unsafe { CURRENT_ASYNC_TASK_COURIER_STD }),
            context_id: decode_async_task_context_id(unsafe { CURRENT_ASYNC_TASK_CONTEXT_STD }),
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
            courier_id: decode_async_task_courier_id(
                CURRENT_ASYNC_TASK_COURIER.load(Ordering::Acquire),
            ),
            context_id: decode_async_task_context_id(
                CURRENT_ASYNC_TASK_CONTEXT.load(Ordering::Acquire),
            ),
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
                CURRENT_ASYNC_TASK_COURIER_STD = encode_async_task_courier_id(context.courier_id);
                CURRENT_ASYNC_TASK_CONTEXT_STD = encode_async_task_context_id(context.context_id);
            } else {
                CURRENT_ASYNC_TASK_CORE_STD = 0;
                CURRENT_ASYNC_TASK_SLOT_STD = usize::MAX;
                CURRENT_ASYNC_TASK_GENERATION_STD = 0;
                CURRENT_ASYNC_TASK_COURIER_STD = ASYNC_TASK_ID_SENTINEL_STD;
                CURRENT_ASYNC_TASK_CONTEXT_STD = ASYNC_TASK_ID_SENTINEL_STD;
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
            CURRENT_ASYNC_TASK_COURIER.store(
                encode_async_task_courier_id(context.courier_id),
                Ordering::Release,
            );
            CURRENT_ASYNC_TASK_CONTEXT.store(
                encode_async_task_context_id(context.context_id),
                Ordering::Release,
            );
        } else {
            CURRENT_ASYNC_TASK_CORE.store(0, Ordering::Release);
            CURRENT_ASYNC_TASK_SLOT.store(usize::MAX, Ordering::Release);
            CURRENT_ASYNC_TASK_GENERATION.store(0, Ordering::Release);
            CURRENT_ASYNC_TASK_COURIER.store(ASYNC_TASK_ID_SENTINEL, Ordering::Release);
            CURRENT_ASYNC_TASK_CONTEXT.store(ASYNC_TASK_ID_SENTINEL, Ordering::Release);
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

/// Returns the owning courier identity for the current async task when available.
///
/// This prefers the active async-task context and falls back to the lower `fusion-sys`
/// managed-fiber slot when the caller is running on one raw managed fiber outside the executor.
///
/// # Errors
///
/// Returns an error when no current courier identity is available honestly.
pub fn current_async_courier_id() -> Result<CourierId, ExecutorError> {
    if let Some(context) = current_async_task_context()
        && let Some(courier_id) = context.courier_id
    {
        return Ok(courier_id);
    }
    system_current_courier_id().map_err(|_| ExecutorError::Unsupported)
}

/// Returns the owning context identity for the current async task when available.
///
/// This prefers the active async-task context and falls back to the lower `fusion-sys`
/// managed-fiber slot when the caller is running on one raw managed fiber outside the executor.
///
/// # Errors
///
/// Returns an error when no current context identity is available honestly.
pub fn current_async_context_id() -> Result<ContextId, ExecutorError> {
    if let Some(context) = current_async_task_context()
        && let Some(context_id) = context.context_id
    {
        return Ok(context_id);
    }
    system_current_context_id().map_err(|_| ExecutorError::Unsupported)
}

/// Returns the courier-owned runtime ledger for the current async task when available.
///
/// # Errors
///
/// Returns an error when the current async runtime cannot honestly resolve one courier-owned
/// ledger.
pub fn current_async_courier_runtime_ledger() -> Result<CourierRuntimeLedger, ExecutorError> {
    let context = current_async_task_context().ok_or(ExecutorError::Unsupported)?;
    let courier_id = context.courier_id.ok_or(ExecutorError::Unsupported)?;
    let core = executor_core_from_context(context)?;
    let runtime_sink = core.runtime_sink.ok_or(ExecutorError::Unsupported)?;
    runtime_sink
        .runtime_ledger(courier_id)
        .map_err(executor_error_from_runtime_sink)
}

/// Returns the courier-owned responsiveness classification for the current async task when
/// available.
///
/// # Errors
///
/// Returns an error when the current async runtime cannot honestly resolve one courier-owned
/// responsiveness state.
pub fn current_async_courier_responsiveness() -> Result<CourierResponsiveness, ExecutorError> {
    let context = current_async_task_context().ok_or(ExecutorError::Unsupported)?;
    let courier_id = context.courier_id.ok_or(ExecutorError::Unsupported)?;
    let core = executor_core_from_context(context)?;
    let runtime_sink = core.runtime_sink.ok_or(ExecutorError::Unsupported)?;
    runtime_sink
        .evaluate_responsiveness(courier_id, core.runtime_tick())
        .map_err(executor_error_from_runtime_sink)
}

fn current_async_runtime_subjects()
-> Result<(CourierRuntimeSink, CourierId, ContextId, u64), ExecutorError> {
    let context = current_async_task_context().ok_or(ExecutorError::Unsupported)?;
    let courier_id = context.courier_id.ok_or(ExecutorError::Unsupported)?;
    let core = executor_core_from_context(context)?;
    let runtime_sink = core.runtime_sink.ok_or(ExecutorError::Unsupported)?;
    Ok((
        runtime_sink,
        courier_id,
        current_async_context_id()?,
        core.runtime_tick(),
    ))
}

/// Updates one courier-owned metadata entry for the current async task's owning courier.
///
/// # Errors
///
/// Returns an error when the current async runtime cannot honestly resolve or update the courier.
pub fn update_current_async_courier_metadata(
    key: &'static str,
    value: &'static str,
) -> Result<(), ExecutorError> {
    let (runtime_sink, courier_id, _, tick) = current_async_runtime_subjects()?;
    runtime_sink
        .upsert_metadata(
            courier_id,
            CourierMetadataSubject::AsyncLane,
            key,
            value,
            tick,
        )
        .map_err(executor_error_from_runtime_sink)
}

/// Updates one courier-owned metadata entry for the current async task's context.
///
/// # Errors
///
/// Returns an error when the current async runtime cannot honestly resolve or update the current
/// context.
pub fn update_current_async_context_metadata(
    key: &'static str,
    value: &'static str,
) -> Result<(), ExecutorError> {
    let (runtime_sink, courier_id, context_id, tick) = current_async_runtime_subjects()?;
    runtime_sink
        .upsert_metadata(
            courier_id,
            CourierMetadataSubject::Context(context_id),
            key,
            value,
            tick,
        )
        .map_err(executor_error_from_runtime_sink)
}

/// Registers one courier-level externally visible obligation under the current async task.
///
/// # Errors
///
/// Returns an error when the current async runtime cannot honestly resolve or register the
/// obligation.
pub fn register_current_async_courier_obligation(
    spec: CourierObligationSpec<'static>,
) -> Result<CourierObligationId, ExecutorError> {
    let (runtime_sink, courier_id, _, tick) = current_async_runtime_subjects()?;
    runtime_sink
        .register_obligation(courier_id, spec, tick)
        .map_err(executor_error_from_runtime_sink)
}

/// Records progress on one previously registered courier obligation from the current async task.
///
/// # Errors
///
/// Returns an error when the current async runtime cannot honestly resolve or update the
/// obligation.
pub fn record_current_async_courier_obligation_progress(
    obligation: CourierObligationId,
) -> Result<(), ExecutorError> {
    let (runtime_sink, courier_id, _, tick) = current_async_runtime_subjects()?;
    runtime_sink
        .record_obligation_progress(courier_id, obligation, tick)
        .map_err(executor_error_from_runtime_sink)
}

/// Removes one previously registered courier obligation from the current async task's courier.
///
/// # Errors
///
/// Returns an error when the current async runtime cannot honestly resolve or remove the
/// obligation.
pub fn remove_current_async_courier_obligation(
    obligation: CourierObligationId,
) -> Result<(), ExecutorError> {
    let (runtime_sink, courier_id, _, _) = current_async_runtime_subjects()?;
    runtime_sink
        .remove_obligation(courier_id, obligation)
        .map_err(executor_error_from_runtime_sink)
}

fn executor_core_from_context(
    context: CurrentAsyncTaskContext,
) -> Result<&'static ExecutorCore, ExecutorError> {
    let core = context.core as *const ExecutorCore;
    if core.is_null() {
        return Err(ExecutorError::Unsupported);
    }
    // SAFETY: the async task TLS context only carries this pointer while the task is actively
    // executing on the owning executor core.
    Ok(unsafe { &*core })
}

#[derive(Debug)]
struct AsyncTaskContextGuard;

impl AsyncTaskContextGuard {
    fn install(core: &ExecutorCore, slot_index: usize, generation: u64) -> Self {
        set_current_async_task_context(Some(CurrentAsyncTaskContext {
            core: ::core::ptr::from_ref(core) as usize,
            slot_index,
            generation,
            courier_id: core.courier_id,
            context_id: core.context_id,
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
