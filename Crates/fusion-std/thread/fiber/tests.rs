use std::sync::{
    Arc,
    Mutex as StdMutex,
    OnceLock as StdOnceLock,
};
use std::vec::Vec;

use fusion_pal::sys::mem::{
    Address,
    CachePolicy,
    MemAdviceCaps,
    Protect,
    Region,
};
use fusion_sys::claims::{
    ClaimAwareness,
    ClaimContextId,
    ClaimsDigest,
    ImageSealId,
    PrincipalId,
};
use fusion_sys::courier::{
    CourierCaps,
    CourierChildLaunchRequest,
    CourierLaunchDescriptor,
    CourierPlan,
    CourierVisibility,
};
use fusion_sys::domain::{
    CourierDescriptor,
    DomainCaps,
    DomainDescriptor,
    DomainId,
    DomainKind,
    DomainRegistry,
};
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
    ResourceRequest,
    ResourceResidencySupport,
    ResourceState,
    ResourceSupport,
    SharingPolicy,
    StateValue,
    VirtualMemoryResource,
};

use crate::sync::Mutex as FusionMutex;
use super::*;

fn aligned_bound_resource(len: usize, align: usize) -> MemoryResourceHandle {
    use std::alloc::{
        Layout,
        alloc_zeroed,
    };

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
            fusion_sys::mem::resource::AllocatorLayoutPolicy::exact_static(),
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

#[derive(Debug, Clone, Copy)]
struct TestRuntimeSinkState {
    ledger: CourierRuntimeLedger,
    fiber: Option<CourierFiberRecord>,
    responsiveness: CourierResponsiveness,
    metadata: Option<fusion_sys::courier::CourierMetadataEntry<'static>>,
    obligation: Option<fusion_sys::courier::CourierObligationRecord<'static>>,
}

unsafe fn test_runtime_sink_record_context(
    context: *mut (),
    _courier: CourierId,
    runtime_context: ContextId,
    tick: u64,
) -> Result<(), fusion_sys::courier::CourierRuntimeSinkError> {
    let state = unsafe { &*context.cast::<StdMutex<TestRuntimeSinkState>>() };
    let mut state = state.lock().expect("test runtime sink mutex should lock");
    state.ledger.record_context(runtime_context, tick);
    Ok(())
}

unsafe fn test_runtime_sink_register_fiber(
    context: *mut (),
    _courier: CourierId,
    snapshot: ManagedFiberSnapshot,
    generation: u64,
    class: fusion_sys::courier::CourierFiberClass,
    is_root: bool,
    metadata_attachment: Option<fusion_sys::courier::FiberMetadataAttachment>,
    tick: u64,
) -> Result<(), fusion_sys::courier::CourierRuntimeSinkError> {
    let state = unsafe { &*context.cast::<StdMutex<TestRuntimeSinkState>>() };
    let mut state = state.lock().expect("test runtime sink mutex should lock");
    state.fiber = Some(CourierFiberRecord::from_snapshot(
        snapshot,
        generation,
        class,
        is_root,
        metadata_attachment,
        tick,
    ));
    state.ledger.register_fiber(class);
    Ok(())
}

unsafe fn test_runtime_sink_update_fiber(
    context: *mut (),
    _courier: CourierId,
    snapshot: ManagedFiberSnapshot,
    tick: u64,
) -> Result<(), fusion_sys::courier::CourierRuntimeSinkError> {
    let state = unsafe { &*context.cast::<StdMutex<TestRuntimeSinkState>>() };
    let mut state = state.lock().expect("test runtime sink mutex should lock");
    let Some(record) = state.fiber.as_mut() else {
        return Err(fusion_sys::courier::CourierRuntimeSinkError::NotFound);
    };
    record.update_from_snapshot(snapshot, tick);
    Ok(())
}

unsafe fn test_runtime_sink_mark_fiber_terminal(
    context: *mut (),
    _courier: CourierId,
    fiber: FiberId,
    terminal: fusion_sys::courier::FiberTerminalStatus,
    tick: u64,
) -> Result<(), fusion_sys::courier::CourierRuntimeSinkError> {
    let state = unsafe { &*context.cast::<StdMutex<TestRuntimeSinkState>>() };
    let mut state = state.lock().expect("test runtime sink mutex should lock");
    let Some(mut record) = state.fiber else {
        return Err(fusion_sys::courier::CourierRuntimeSinkError::NotFound);
    };
    if record.fiber != fiber {
        return Err(fusion_sys::courier::CourierRuntimeSinkError::NotFound);
    }
    record.mark_terminal(terminal, tick);
    state.ledger.release_fiber(record.class);
    state.fiber = Some(record);
    Ok(())
}

unsafe fn test_runtime_sink_record_runtime_summary(
    context: *mut (),
    _courier: CourierId,
    summary: CourierRuntimeSummary,
    tick: u64,
) -> Result<(), fusion_sys::courier::CourierRuntimeSinkError> {
    let state = unsafe { &*context.cast::<StdMutex<TestRuntimeSinkState>>() };
    let mut state = state.lock().expect("test runtime sink mutex should lock");
    state.ledger.record_summary(summary, tick);
    Ok(())
}

unsafe fn test_runtime_sink_runtime_ledger(
    context: *mut (),
    _courier: CourierId,
) -> Result<CourierRuntimeLedger, fusion_sys::courier::CourierRuntimeSinkError> {
    let state = unsafe { &*context.cast::<StdMutex<TestRuntimeSinkState>>() };
    let state = state.lock().expect("test runtime sink mutex should lock");
    Ok(state.ledger)
}

unsafe fn test_runtime_sink_fiber_record(
    context: *mut (),
    _courier: CourierId,
    fiber: FiberId,
) -> Result<Option<CourierFiberRecord>, fusion_sys::courier::CourierRuntimeSinkError> {
    let state = unsafe { &*context.cast::<StdMutex<TestRuntimeSinkState>>() };
    let state = state.lock().expect("test runtime sink mutex should lock");
    Ok(state.fiber.filter(|record| record.fiber == fiber))
}

unsafe fn test_runtime_sink_evaluate_responsiveness(
    context: *mut (),
    _courier: CourierId,
    _tick: u64,
) -> Result<CourierResponsiveness, fusion_sys::courier::CourierRuntimeSinkError> {
    let state = unsafe { &*context.cast::<StdMutex<TestRuntimeSinkState>>() };
    let state = state.lock().expect("test runtime sink mutex should lock");
    Ok(state.responsiveness)
}

unsafe fn test_runtime_sink_upsert_metadata(
    context: *mut (),
    _courier: CourierId,
    subject: fusion_sys::courier::CourierMetadataSubject,
    key: &'static str,
    value: &'static str,
    tick: u64,
) -> Result<(), fusion_sys::courier::CourierRuntimeSinkError> {
    let state = unsafe { &*context.cast::<StdMutex<TestRuntimeSinkState>>() };
    let mut state = state.lock().expect("test runtime sink mutex should lock");
    state.metadata = Some(fusion_sys::courier::CourierMetadataEntry::new(
        subject, key, value, tick,
    ));
    Ok(())
}

unsafe fn test_runtime_sink_remove_metadata(
    context: *mut (),
    _courier: CourierId,
    subject: fusion_sys::courier::CourierMetadataSubject,
    key: &str,
) -> Result<(), fusion_sys::courier::CourierRuntimeSinkError> {
    let state = unsafe { &*context.cast::<StdMutex<TestRuntimeSinkState>>() };
    let mut state = state.lock().expect("test runtime sink mutex should lock");
    if state
        .metadata
        .is_some_and(|entry| entry.subject == subject && entry.key == key)
    {
        state.metadata = None;
        return Ok(());
    }
    Err(fusion_sys::courier::CourierRuntimeSinkError::NotFound)
}

unsafe fn test_runtime_sink_register_obligation(
    context: *mut (),
    _courier: CourierId,
    spec: fusion_sys::courier::CourierObligationSpec<'static>,
    tick: u64,
) -> Result<fusion_sys::courier::CourierObligationId, fusion_sys::courier::CourierRuntimeSinkError>
{
    let state = unsafe { &*context.cast::<StdMutex<TestRuntimeSinkState>>() };
    let mut state = state.lock().expect("test runtime sink mutex should lock");
    let record = fusion_sys::courier::CourierObligationRecord::new(
        fusion_sys::courier::CourierObligationId::new(1),
        spec,
        tick,
    );
    state.obligation = Some(record);
    Ok(record.id)
}

unsafe fn test_runtime_sink_record_obligation_progress(
    context: *mut (),
    _courier: CourierId,
    obligation: fusion_sys::courier::CourierObligationId,
    tick: u64,
) -> Result<(), fusion_sys::courier::CourierRuntimeSinkError> {
    let state = unsafe { &*context.cast::<StdMutex<TestRuntimeSinkState>>() };
    let mut state = state.lock().expect("test runtime sink mutex should lock");
    let Some(record) = state.obligation.as_mut() else {
        return Err(fusion_sys::courier::CourierRuntimeSinkError::NotFound);
    };
    if record.id != obligation {
        return Err(fusion_sys::courier::CourierRuntimeSinkError::NotFound);
    }
    record.last_progress_tick = tick;
    record.responsiveness = CourierResponsiveness::Responsive;
    Ok(())
}

unsafe fn test_runtime_sink_remove_obligation(
    context: *mut (),
    _courier: CourierId,
    obligation: fusion_sys::courier::CourierObligationId,
) -> Result<(), fusion_sys::courier::CourierRuntimeSinkError> {
    let state = unsafe { &*context.cast::<StdMutex<TestRuntimeSinkState>>() };
    let mut state = state.lock().expect("test runtime sink mutex should lock");
    if state
        .obligation
        .is_some_and(|record| record.id == obligation)
    {
        state.obligation = None;
        return Ok(());
    }
    Err(fusion_sys::courier::CourierRuntimeSinkError::NotFound)
}

const TEST_RUNTIME_SINK_VTABLE: fusion_sys::courier::CourierRuntimeSinkVTable =
    fusion_sys::courier::CourierRuntimeSinkVTable {
        record_context: test_runtime_sink_record_context,
        register_fiber: test_runtime_sink_register_fiber,
        update_fiber: test_runtime_sink_update_fiber,
        mark_fiber_terminal: test_runtime_sink_mark_fiber_terminal,
        record_runtime_summary: test_runtime_sink_record_runtime_summary,
        runtime_ledger: test_runtime_sink_runtime_ledger,
        fiber_record: test_runtime_sink_fiber_record,
        evaluate_responsiveness: test_runtime_sink_evaluate_responsiveness,
        upsert_metadata: test_runtime_sink_upsert_metadata,
        remove_metadata: test_runtime_sink_remove_metadata,
        register_obligation: test_runtime_sink_register_obligation,
        record_obligation_progress: test_runtime_sink_record_obligation_progress,
        remove_obligation: test_runtime_sink_remove_obligation,
    };

fn test_runtime_sink(state: &StdMutex<TestRuntimeSinkState>) -> CourierRuntimeSink {
    CourierRuntimeSink::new(
        (state as *const StdMutex<TestRuntimeSinkState>) as *mut (),
        &TEST_RUNTIME_SINK_VTABLE,
    )
}

struct OversizedExplicitTask;

impl ExplicitFiberTask for OversizedExplicitTask {
    type Output = ();

    const STACK_BYTES: NonZeroUsize =
        NonZeroUsize::new(32 * 1024).expect("non-zero oversized stack");

    fn run(self) -> Self::Output {}
}

struct SupportedExplicitTask;

impl ExplicitFiberTask for SupportedExplicitTask {
    type Output = ();

    const STACK_BYTES: NonZeroUsize =
        NonZeroUsize::new(8 * 1024).expect("non-zero supported stack");

    fn run(self) -> Self::Output {}
}

struct SupportedInlineNoYieldTask;

impl ExplicitFiberTask for SupportedInlineNoYieldTask {
    type Output = u32;

    const STACK_BYTES: NonZeroUsize =
        NonZeroUsize::new(32 * 1024).expect("non-zero oversized inline stack");
    const EXECUTION: FiberTaskExecution = FiberTaskExecution::InlineNoYield;

    fn run(self) -> Self::Output {
        17
    }
}

struct SupportedGeneratedContractTask;

impl GeneratedExplicitFiberTask for SupportedGeneratedContractTask {
    type Output = ();

    fn run(self) -> Self::Output {}

    fn task_attributes() -> Result<FiberTaskAttributes, FiberError>
    where
        Self: Sized,
    {
        Ok(generated_explicit_task_contract_attributes::<Self>())
    }
}

declare_generated_fiber_task_contract!(
    SupportedGeneratedContractTask,
    NonZeroUsize::new(8 * 1024).expect("non-zero supported generated stack"),
);

const SUPPORTED_GENERATED_CONTRACT_STACK_BYTES: NonZeroUsize =
    NonZeroUsize::new(8 * 1024).expect("non-zero supported generated stack");
const SUPPORTED_GENERATED_CONTRACT_ADMITTED_STACK_BYTES: NonZeroUsize =
    match admit_generated_fiber_task_stack_bytes(SUPPORTED_GENERATED_CONTRACT_STACK_BYTES) {
        Ok(stack_bytes) => stack_bytes,
        Err(_) => panic!("generated stack bytes should admit"),
    };
const SUPPORTED_GENERATED_CONTRACT_CLASS: FiberStackClass =
    match FiberStackClass::from_stack_bytes(SUPPORTED_GENERATED_CONTRACT_ADMITTED_STACK_BYTES) {
        Ok(class) => class,
        Err(_) => panic!("generated stack class should be valid"),
    };

const COMPILE_TIME_EXPLICIT_CLASSES: [FiberStackClassConfig; 1] = [
    match FiberStackClassConfig::new(SUPPORTED_GENERATED_CONTRACT_CLASS, 2) {
        Ok(class) => class,
        Err(_) => panic!("valid compile-time class config"),
    },
];

const COMPILE_TIME_EXPLICIT_CONFIG: FiberPoolConfig<'static> =
    match FiberPoolConfig::classed(&COMPILE_TIME_EXPLICIT_CLASSES) {
        Ok(config) => config,
        Err(_) => panic!("compile-time explicit config should be valid"),
    };
const COMPILE_TIME_EXPLICIT_ATTRIBUTES: FiberTaskAttributes =
    match FiberTaskAttributes::from_stack_bytes(
        NonZeroUsize::new(8 * 1024).expect("non-zero supported stack"),
        FiberTaskPriority::DEFAULT,
    ) {
        Ok(attributes) => attributes,
        Err(_) => panic!("compile-time explicit attributes should be valid"),
    };

const _: () =
    COMPILE_TIME_EXPLICIT_CONFIG.assert_explicit_task_supported::<SupportedExplicitTask>();
const _: () =
    COMPILE_TIME_EXPLICIT_CONFIG.assert_task_attributes_supported(COMPILE_TIME_EXPLICIT_ATTRIBUTES);
const _: () = COMPILE_TIME_EXPLICIT_CONFIG
    .assert_generated_task_supported::<SupportedGeneratedContractTask>();

static CAPACITY_EVENT_CALLS: AtomicU32 = AtomicU32::new(0);
static LAST_CAPACITY_FIBER_ID: AtomicUsize = AtomicUsize::new(0);
static LAST_CAPACITY_CARRIER_ID: AtomicUsize = AtomicUsize::new(usize::MAX);
static LAST_CAPACITY_COMMITTED: AtomicU32 = AtomicU32::new(0);
static LAST_CAPACITY_RESERVATION: AtomicU32 = AtomicU32::new(0);
static YIELD_BUDGET_EVENT_CALLS: AtomicU32 = AtomicU32::new(0);
static LAST_YIELD_BUDGET_FIBER_ID: AtomicUsize = AtomicUsize::new(0);
static LAST_YIELD_BUDGET_CARRIER_ID: AtomicUsize = AtomicUsize::new(usize::MAX);
static LAST_YIELD_BUDGET_NANOS: AtomicU64 = AtomicU64::new(0);
static LAST_YIELD_OBSERVED_NANOS: AtomicU64 = AtomicU64::new(0);
static ELASTIC_TEST_LOCK: StdOnceLock<StdMutex<()>> = StdOnceLock::new();

fn lock_elastic_tests() -> std::sync::MutexGuard<'static, ()> {
    ELASTIC_TEST_LOCK
        .get_or_init(|| StdMutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn record_capacity_event(event: FiberCapacityEvent) {
    CAPACITY_EVENT_CALLS.fetch_add(1, Ordering::AcqRel);
    LAST_CAPACITY_FIBER_ID.store(
        usize::try_from(event.fiber_id).unwrap_or(usize::MAX),
        Ordering::Release,
    );
    LAST_CAPACITY_CARRIER_ID.store(event.carrier_id, Ordering::Release);
    LAST_CAPACITY_COMMITTED.store(event.committed_pages, Ordering::Release);
    LAST_CAPACITY_RESERVATION.store(event.reservation_pages, Ordering::Release);
}

fn record_yield_budget_event(event: FiberYieldBudgetEvent) {
    YIELD_BUDGET_EVENT_CALLS.fetch_add(1, Ordering::AcqRel);
    LAST_YIELD_BUDGET_FIBER_ID.store(
        usize::try_from(event.fiber_id).unwrap_or(usize::MAX),
        Ordering::Release,
    );
    LAST_YIELD_BUDGET_CARRIER_ID.store(event.carrier_id, Ordering::Release);
    LAST_YIELD_BUDGET_NANOS.store(
        saturating_duration_to_nanos_u64(event.budget),
        Ordering::Release,
    );
    LAST_YIELD_OBSERVED_NANOS.store(
        saturating_duration_to_nanos_u64(event.observed),
        Ordering::Release,
    );
}

#[test]
fn stack_class_rounding_matches_current_slab_envelope() {
    let _guard = crate::thread::runtime_test_guard();
    let support = GreenPool::support();
    let config = FiberPoolConfig::fixed(
        NonZeroUsize::new(12 * 1024).expect("non-zero fixed stack"),
        2,
    );
    let slab = FiberStackSlab::new(
        &config,
        support.context.min_stack_alignment.max(16),
        support.context.stack_direction,
    )
    .expect("fixed stack slab should build");

    let default = slab
        .default_task_class()
        .expect("slab should map its envelope to a task class");
    assert_eq!(default.size_bytes().get(), 8 * 1024);
    assert!(slab.supports_task_class(default));
    assert!(slab.supports_task_class(FiberStackClass::MIN));
    assert!(
        !slab.supports_task_class(
            FiberStackClass::new(NonZeroUsize::new(32 * 1024).expect("non-zero larger class"))
                .expect("larger class should be valid"),
        )
    );
}

#[test]
fn class_store_selects_smallest_matching_pool() {
    let _guard = crate::thread::runtime_test_guard();
    let support = GreenPool::support();
    let classes = [
        FiberStackClassConfig::new(
            FiberStackClass::new(NonZeroUsize::new(4 * 1024).expect("non-zero class"))
                .expect("valid class"),
            1,
        )
        .expect("valid class config"),
        FiberStackClassConfig::new(
            FiberStackClass::new(NonZeroUsize::new(8 * 1024).expect("non-zero class"))
                .expect("valid class"),
            1,
        )
        .expect("valid class config"),
        FiberStackClassConfig::new(
            FiberStackClass::new(NonZeroUsize::new(16 * 1024).expect("non-zero class"))
                .expect("valid class"),
            1,
        )
        .expect("valid class config"),
    ];
    let config = FiberPoolConfig::classed(&classes).expect("classed config should build");
    let store = FiberStackStore::new(
        &config,
        support.context.min_stack_alignment.max(16),
        support.context.stack_direction,
    )
    .expect("class store should build");

    assert_eq!(store.total_capacity(), 3);
    assert_eq!(
        store
            .default_task_class()
            .expect("largest configured class should be discoverable")
            .size_bytes()
            .get(),
        16 * 1024
    );

    let task = FiberTaskAttributes::new(
        FiberStackClass::from_stack_bytes(
            NonZeroUsize::new(6 * 1024).expect("non-zero requested size"),
        )
        .expect("task class should round"),
    );
    let lease = store.acquire(task).expect("matching class should allocate");
    assert_eq!(lease.class.size_bytes().get(), 8 * 1024);
    store
        .release(lease.pool_index, lease.slot_index)
        .expect("allocated class slot should release");

    let oversize = FiberStackClass::new(NonZeroUsize::new(32 * 1024).expect("non-zero class"))
        .expect("valid class");
    assert!(!store.supports_task_class(oversize));
}

#[test]
fn classed_config_derives_capacity_and_largest_backing() {
    let _guard = crate::thread::runtime_test_guard();
    let classes = [
        FiberStackClassConfig::new(FiberStackClass::MIN, 3).expect("valid class config"),
        FiberStackClassConfig::new(
            FiberStackClass::new(NonZeroUsize::new(8 * 1024).expect("non-zero class"))
                .expect("valid class"),
            5,
        )
        .expect("valid class config"),
    ];
    let config = FiberPoolConfig::classed(&classes).expect("classed config should build");

    assert_eq!(config.task_capacity_per_carrier().expect("capacity"), 8);
    assert_eq!(config.growth_chunk, 8);
    assert_eq!(config.max_fibers_per_carrier, 8);
    assert_eq!(config.growth, GreenGrowth::Fixed);
    assert_eq!(config.scheduling, GreenScheduling::Fifo);
    assert_eq!(
        config.stack_backing,
        FiberStackBacking::Fixed {
            stack_size: classes[1].class.size_bytes(),
        }
    );
}

#[test]
fn class_store_uses_per_class_growth_chunks() {
    let _guard = crate::thread::runtime_test_guard();
    let support = GreenPool::support();
    let classes = [
        FiberStackClassConfig::new(FiberStackClass::MIN, 4)
            .expect("valid class config")
            .with_growth_chunk(2)
            .expect("valid class growth chunk"),
        FiberStackClassConfig::new(
            FiberStackClass::new(NonZeroUsize::new(8 * 1024).expect("non-zero class"))
                .expect("valid class"),
            6,
        )
        .expect("valid class config")
        .with_growth_chunk(3)
        .expect("valid class growth chunk"),
    ];
    let config = FiberPoolConfig::classed(&classes)
        .expect("classed config should build")
        .with_growth(GreenGrowth::OnDemand);
    let store = FiberStackStore::new(
        &config,
        support.context.min_stack_alignment.max(16),
        support.context.stack_direction,
    )
    .expect("class store should build");

    let FiberStackStore::Classes(pools) = store else {
        panic!("expected class-backed store");
    };
    assert_eq!(pools.entry(0).expect("first pool").slab.chunk_size, 2);
    assert_eq!(pools.entry(0).expect("first pool").slab.initial_slots, 2);
    assert_eq!(pools.entry(1).expect("second pool").slab.chunk_size, 3);
    assert_eq!(pools.entry(1).expect("second pool").slab.initial_slots, 3);
}

#[test]
fn classed_config_validates_explicit_task_contracts() {
    let _guard = crate::thread::runtime_test_guard();
    let classes = [
        FiberStackClassConfig::new(SUPPORTED_GENERATED_CONTRACT_CLASS, 2)
            .expect("valid class config"),
    ];
    let config = FiberPoolConfig::classed(&classes).expect("classed config should build");

    assert!(
        config
            .validate_generated_task::<SupportedGeneratedContractTask>()
            .is_ok()
    );

    let error = config
        .validate_explicit_task::<OversizedExplicitTask>()
        .expect_err("oversized explicit task should be rejected");
    assert_eq!(error.kind(), FiberError::unsupported().kind());
}

#[test]
fn explicit_task_contracts_work_in_const_context() {
    let _guard = crate::thread::runtime_test_guard();
    const VALIDATION: Result<(), FiberError> =
        COMPILE_TIME_EXPLICIT_CONFIG.validate_explicit_task::<SupportedExplicitTask>();

    assert_eq!(
        SupportedExplicitTask::ATTRIBUTES
            .stack_class
            .size_bytes()
            .get(),
        8 * 1024
    );
    VALIDATION.expect("supported explicit task should validate in const context");
}

#[test]
fn raw_task_attributes_work_in_const_context() {
    let _guard = crate::thread::runtime_test_guard();
    const VALIDATION: Result<(), FiberError> =
        COMPILE_TIME_EXPLICIT_CONFIG.validate_task_attributes(COMPILE_TIME_EXPLICIT_ATTRIBUTES);

    assert_eq!(
        COMPILE_TIME_EXPLICIT_ATTRIBUTES
            .stack_class
            .size_bytes()
            .get(),
        8 * 1024
    );
    VALIDATION.expect("raw task attributes should validate in const context");
}

#[test]
fn generated_task_contracts_work_in_const_context() {
    let _guard = crate::thread::runtime_test_guard();
    const VALIDATION: Result<(), FiberError> = COMPILE_TIME_EXPLICIT_CONFIG
        .validate_generated_task_contract::<SupportedGeneratedContractTask>(
    );

    assert_eq!(
        <SupportedGeneratedContractTask as GeneratedExplicitFiberTaskContract>::ATTRIBUTES
            .stack_class
            .size_bytes()
            .get(),
        SUPPORTED_GENERATED_CONTRACT_CLASS.size_bytes().get()
    );
    VALIDATION.expect("generated task should validate in const context");
}

#[test]
fn live_pool_validates_generated_task_contracts_before_spawn() {
    let _guard = crate::thread::runtime_test_guard();
    let carrier = ThreadPool::new(&ThreadPoolConfig::new()).expect("carrier pool should build");
    let classes =
        [FiberStackClassConfig::new(FiberStackClass::MIN, 2).expect("valid class config")];
    let green = GreenPool::new(
        &FiberPoolConfig::classed(&classes).expect("classed config should build"),
        &carrier,
    )
    .expect("green pool should build");

    let error = green
        .validate_generated_task::<SupportedGeneratedContractTask>()
        .expect_err("generated task should be rejected when class is missing");
    assert_eq!(error.kind(), FiberError::unsupported().kind());

    let error = green
        .spawn_generated(SupportedGeneratedContractTask)
        .expect_err("spawn should reject unsupported generated class");
    assert_eq!(error.kind(), FiberError::unsupported().kind());

    green
        .shutdown()
        .expect("green pool should shut down cleanly");
    carrier
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[test]
fn elastic_stack_slab_grows_and_shrinks_by_chunk() {
    let _guard = crate::thread::runtime_test_guard();
    let _guard = lock_elastic_tests();
    let support = GreenPool::support();
    let config = FiberPoolConfig {
        classes: &[],
        stack_backing: FiberStackBacking::Elastic {
            initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
            max_size: NonZeroUsize::new(16 * 1024).expect("non-zero max stack"),
        },
        sizing: default_runtime_sizing_strategy(),
        guard_pages: 1,
        growth_chunk: 2,
        max_fibers_per_carrier: 5,
        scheduling: GreenScheduling::Fifo,
        spawn_locality_policy: CarrierSpawnLocalityPolicy::SameCore,
        priority_age_cap: None,
        growth: GreenGrowth::OnDemand,
        telemetry: FiberTelemetry::Full,
        capacity_policy: CapacityPolicy::Abort,
        yield_budget_policy: FiberYieldBudgetPolicy::Abort,
        reactor_policy: GreenReactorPolicy::Automatic,
        huge_pages: HugePagePolicy::Disabled,
        courier_id: None,
        context_id: None,
        runtime_sink: None,
        launch_control: None,
        launch_request: None,
    };
    let slab = FiberStackSlab::new(
        &config,
        support.context.min_stack_alignment.max(16),
        support.context.stack_direction,
    )
    .expect("elastic stack slab should build");

    {
        let state = slab
            .state
            .lock()
            .map_err(fiber_error_from_sync)
            .expect("slab state should be observable");
        assert_eq!(state.committed_slots, 2);
    }

    let mut leases = Vec::new();
    for _ in 0..5 {
        leases.push(slab.acquire().expect("chunked slab should grow on demand"));
    }
    {
        let state = slab
            .state
            .lock()
            .map_err(fiber_error_from_sync)
            .expect("slab state should be observable");
        assert_eq!(state.committed_slots, 5);
    }

    for lease in &leases {
        if lease.slot_index >= 2 {
            slab.release(lease.slot_index)
                .expect("tail slots should release cleanly");
        }
    }
    {
        let state = slab
            .state
            .lock()
            .map_err(fiber_error_from_sync)
            .expect("slab state should be observable");
        assert_eq!(state.committed_slots, 4);
    }

    for lease in &leases {
        if lease.slot_index < 2 {
            slab.release(lease.slot_index)
                .expect("initial slots should release cleanly");
        }
    }
    {
        let state = slab
            .state
            .lock()
            .map_err(fiber_error_from_sync)
            .expect("slab state should be observable");
        assert_eq!(state.committed_slots, 2);
    }
}

#[test]
fn elastic_stack_fault_promotion_makes_detector_page_writable() {
    let _guard = crate::thread::runtime_test_guard();
    let _guard = lock_elastic_tests();
    let support = GreenPool::support();
    let config = FiberPoolConfig {
        classes: &[],
        stack_backing: FiberStackBacking::Elastic {
            initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
            max_size: NonZeroUsize::new(16 * 1024).expect("non-zero max stack"),
        },
        sizing: default_runtime_sizing_strategy(),
        guard_pages: 1,
        growth_chunk: 1,
        max_fibers_per_carrier: 1,
        scheduling: GreenScheduling::Fifo,
        spawn_locality_policy: CarrierSpawnLocalityPolicy::SameCore,
        priority_age_cap: None,
        growth: GreenGrowth::OnDemand,
        telemetry: FiberTelemetry::Full,
        capacity_policy: CapacityPolicy::Abort,
        yield_budget_policy: FiberYieldBudgetPolicy::Abort,
        reactor_policy: GreenReactorPolicy::Automatic,
        huge_pages: HugePagePolicy::Disabled,
        courier_id: None,
        context_id: None,
        runtime_sink: None,
        launch_control: None,
        launch_request: None,
    };
    let slab = FiberStackSlab::new(
        &config,
        support.context.min_stack_alignment.max(16),
        support.context.stack_direction,
    )
    .expect("elastic stack slab should build");

    let metadata = match &slab.backing {
        FiberStackBackingState::Elastic { metadata, .. } => metadata,
        FiberStackBackingState::Fixed(_) => panic!("expected elastic backing"),
    };
    let meta = &metadata[0];
    let detector = meta.detector_page.load(Ordering::Acquire);
    let old_guard = meta.guard_page.load(Ordering::Acquire);

    assert!(try_promote_elastic_stack_fault(detector));
    assert_eq!(meta.detector_page.load(Ordering::Acquire), old_guard);
    assert_ne!(meta.guard_page.load(Ordering::Acquire), old_guard);

    unsafe {
        (detector as *mut u8).write_volatile(0x5A);
        assert_eq!((detector as *const u8).read_volatile(), 0x5A);
    }
}

#[test]
fn elastic_stack_stats_track_growth_and_capacity() {
    let _guard = crate::thread::runtime_test_guard();
    let _guard = lock_elastic_tests();
    let support = GreenPool::support();
    let config = FiberPoolConfig {
        classes: &[],
        stack_backing: FiberStackBacking::Elastic {
            initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
            max_size: NonZeroUsize::new(8 * 1024).expect("non-zero max stack"),
        },
        sizing: default_runtime_sizing_strategy(),
        guard_pages: 1,
        growth_chunk: 1,
        max_fibers_per_carrier: 1,
        scheduling: GreenScheduling::Fifo,
        spawn_locality_policy: CarrierSpawnLocalityPolicy::SameCore,
        priority_age_cap: None,
        growth: GreenGrowth::OnDemand,
        telemetry: FiberTelemetry::Full,
        capacity_policy: CapacityPolicy::Abort,
        yield_budget_policy: FiberYieldBudgetPolicy::Abort,
        reactor_policy: GreenReactorPolicy::Automatic,
        huge_pages: HugePagePolicy::Disabled,
        courier_id: None,
        context_id: None,
        runtime_sink: None,
        launch_control: None,
        launch_request: None,
    };
    let slab = FiberStackSlab::new(
        &config,
        support.context.min_stack_alignment.max(16),
        support.context.stack_direction,
    )
    .expect("elastic stack slab should build");

    let lease = slab
        .acquire()
        .expect("elastic slab should allocate one slot");
    let metadata = match &slab.backing {
        FiberStackBackingState::Elastic { metadata, .. } => metadata,
        FiberStackBackingState::Fixed(_) => panic!("expected elastic backing"),
    };
    let meta = &metadata[lease.slot_index];
    let detector = meta.detector_page.load(Ordering::Acquire);

    assert!(try_promote_elastic_stack_fault(detector));

    let stats = slab.stack_stats().expect("telemetry should be enabled");
    assert_eq!(stats.total_growth_events, 1);
    assert_eq!(stats.peak_committed_pages, 2);
    assert_eq!(stats.committed_distribution.as_slice(), &[(2, 1)]);
    assert_eq!(stats.at_capacity_count, 1);

    slab.release(lease.slot_index)
        .expect("elastic slab should release the slot cleanly");
    let stats = slab.stack_stats().expect("telemetry should remain enabled");
    assert_eq!(stats.total_growth_events, 0);
    assert_eq!(stats.peak_committed_pages, 0);
    assert!(stats.committed_distribution.is_empty());
    assert_eq!(stats.at_capacity_count, 0);
}

#[test]
fn elastic_capacity_events_dispatch_with_fiber_identity() {
    let _guard = crate::thread::runtime_test_guard();
    let _guard = lock_elastic_tests();
    CAPACITY_EVENT_CALLS.store(0, Ordering::Release);
    LAST_CAPACITY_FIBER_ID.store(0, Ordering::Release);
    LAST_CAPACITY_CARRIER_ID.store(usize::MAX, Ordering::Release);
    LAST_CAPACITY_COMMITTED.store(0, Ordering::Release);
    LAST_CAPACITY_RESERVATION.store(0, Ordering::Release);

    let support = GreenPool::support();
    let config = FiberPoolConfig {
        classes: &[],
        stack_backing: FiberStackBacking::Elastic {
            initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
            max_size: NonZeroUsize::new(8 * 1024).expect("non-zero max stack"),
        },
        sizing: default_runtime_sizing_strategy(),
        guard_pages: 1,
        growth_chunk: 1,
        max_fibers_per_carrier: 1,
        scheduling: GreenScheduling::Fifo,
        spawn_locality_policy: CarrierSpawnLocalityPolicy::SameCore,
        priority_age_cap: None,
        growth: GreenGrowth::OnDemand,
        telemetry: FiberTelemetry::Full,
        capacity_policy: CapacityPolicy::Notify(record_capacity_event),
        yield_budget_policy: FiberYieldBudgetPolicy::Abort,
        reactor_policy: GreenReactorPolicy::Automatic,
        huge_pages: HugePagePolicy::Disabled,
        courier_id: None,
        context_id: None,
        runtime_sink: None,
        launch_control: None,
        launch_request: None,
    };
    let slab = FiberStackSlab::new(
        &config,
        support.context.min_stack_alignment.max(16),
        support.context.stack_direction,
    )
    .expect("elastic stack slab should build");

    let lease = slab
        .acquire()
        .expect("elastic slab should allocate one slot");
    slab.attach_slot_identity(lease.slot_index, 41, 3, PlatformWakeToken::invalid())
        .expect("slot identity should attach");

    let metadata = match &slab.backing {
        FiberStackBackingState::Elastic { metadata, .. } => metadata,
        FiberStackBackingState::Fixed(_) => panic!("expected elastic backing"),
    };
    let meta = &metadata[lease.slot_index];
    let detector = meta.detector_page.load(Ordering::Acquire);
    assert!(try_promote_elastic_stack_fault(detector));

    slab.dispatch_capacity_event(lease.slot_index, config.capacity_policy)
        .expect("capacity event should dispatch");
    assert_eq!(CAPACITY_EVENT_CALLS.load(Ordering::Acquire), 1);
    assert_eq!(LAST_CAPACITY_FIBER_ID.load(Ordering::Acquire), 41);
    assert_eq!(LAST_CAPACITY_CARRIER_ID.load(Ordering::Acquire), 3);
    assert_eq!(LAST_CAPACITY_COMMITTED.load(Ordering::Acquire), 2);
    assert_eq!(LAST_CAPACITY_RESERVATION.load(Ordering::Acquire), 2);

    slab.dispatch_capacity_event(lease.slot_index, config.capacity_policy)
        .expect("capacity event should not redispatch");
    assert_eq!(CAPACITY_EVENT_CALLS.load(Ordering::Acquire), 1);
}

#[test]
fn elastic_stack_registry_tracks_live_slots_and_clears_on_drop() {
    let _guard = crate::thread::runtime_test_guard();
    let _guard = lock_elastic_tests();
    let support = GreenPool::support();
    let config = FiberPoolConfig {
        classes: &[],
        stack_backing: FiberStackBacking::Elastic {
            initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
            max_size: NonZeroUsize::new(16 * 1024).expect("non-zero max stack"),
        },
        sizing: default_runtime_sizing_strategy(),
        guard_pages: 1,
        growth_chunk: 1,
        max_fibers_per_carrier: 1,
        scheduling: GreenScheduling::Fifo,
        spawn_locality_policy: CarrierSpawnLocalityPolicy::SameCore,
        priority_age_cap: None,
        growth: GreenGrowth::OnDemand,
        telemetry: FiberTelemetry::Disabled,
        capacity_policy: CapacityPolicy::Abort,
        yield_budget_policy: FiberYieldBudgetPolicy::Abort,
        reactor_policy: GreenReactorPolicy::Automatic,
        huge_pages: HugePagePolicy::Disabled,
        courier_id: None,
        context_id: None,
        runtime_sink: None,
        launch_control: None,
        launch_request: None,
    };
    let slab = FiberStackSlab::new(
        &config,
        support.context.min_stack_alignment.max(16),
        support.context.stack_direction,
    )
    .expect("elastic stack slab should build");

    let lease = slab
        .acquire()
        .expect("elastic slab should allocate one slot");
    let metadata = match &slab.backing {
        FiberStackBackingState::Elastic { metadata, .. } => metadata,
        FiberStackBackingState::Fixed(_) => panic!("expected elastic backing"),
    };
    let detector = metadata[lease.slot_index]
        .detector_page
        .load(Ordering::Acquire);

    let snapshot_ptr =
        ELASTIC_STACK_SNAPSHOT.load(Ordering::Acquire) as *const ElasticRegistrySnapshotHeader;
    assert!(!snapshot_ptr.is_null());
    let snapshot = unsafe { &*snapshot_ptr };
    assert!(find_snapshot_elastic_entry(snapshot, detector).is_some());

    drop(slab);
    let snapshot_ptr =
        ELASTIC_STACK_SNAPSHOT.load(Ordering::Acquire) as *const ElasticRegistrySnapshotHeader;
    if !snapshot_ptr.is_null() {
        let snapshot = unsafe { &*snapshot_ptr };
        assert!(find_snapshot_elastic_entry(snapshot, detector).is_none());
    }
}

#[test]
fn elastic_huge_page_policy_leaves_a_small_page_growth_window() {
    let _guard = crate::thread::runtime_test_guard();
    let _guard = lock_elastic_tests();
    if !system_mem()
        .support()
        .advice
        .contains(MemAdviceCaps::HUGE_PAGE)
    {
        return;
    }
    let support = GreenPool::support();
    let page = system_mem().page_info().alloc_granule.get();
    let config = FiberPoolConfig {
        classes: &[],
        stack_backing: FiberStackBacking::Elastic {
            initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
            max_size: NonZeroUsize::new(4 * 1024 * 1024).expect("non-zero max stack"),
        },
        sizing: default_runtime_sizing_strategy(),
        guard_pages: 1,
        growth_chunk: 1,
        max_fibers_per_carrier: 1,
        scheduling: GreenScheduling::Fifo,
        spawn_locality_policy: CarrierSpawnLocalityPolicy::SameCore,
        priority_age_cap: None,
        growth: GreenGrowth::OnDemand,
        telemetry: FiberTelemetry::Disabled,
        capacity_policy: CapacityPolicy::Abort,
        yield_budget_policy: FiberYieldBudgetPolicy::Abort,
        reactor_policy: GreenReactorPolicy::Automatic,
        huge_pages: HugePagePolicy::Enabled {
            size: HugePageSize::TwoMiB,
        },
        courier_id: None,
        context_id: None,
        runtime_sink: None,
        launch_control: None,
        launch_request: None,
    };
    let slab = FiberStackSlab::new(
        &config,
        support.context.min_stack_alignment.max(16),
        support.context.stack_direction,
    )
    .expect("elastic stack slab should build with huge-page advice");

    let (huge_region, no_huge_region) = slab
        .huge_page_regions(0, HugePageSize::TwoMiB)
        .expect("huge-page planning should succeed");
    let huge_region = huge_region.expect("large elastic slots should expose an upper huge region");
    let no_huge_region =
        no_huge_region.expect("elastic huge-page planning should keep a lower small-page window");
    assert!(huge_region.len >= HugePageSize::TwoMiB.bytes());
    assert!(no_huge_region.len >= 3 * page);
    assert!(huge_region.base.addr().get() > no_huge_region.base.addr().get());
}

#[test]
fn priority_queue_dequeues_higher_priorities_first() {
    let _guard = crate::thread::runtime_test_guard();
    let mut buckets = [PriorityBucket::empty(); FIBER_PRIORITY_LEVELS];
    let mut next = [EMPTY_QUEUE_SLOT; 8];
    let mut priorities = [FiberTaskPriority::DEFAULT.get(); 8];
    let mut enqueue_epochs = [0u64; 8];
    let bucket_slice = MetadataSlice {
        ptr: core::ptr::NonNull::new(buckets.as_mut_ptr()).expect("bucket slice pointer"),
        len: buckets.len(),
    };
    let next_slice = MetadataSlice {
        ptr: core::ptr::NonNull::new(next.as_mut_ptr()).expect("next slice pointer"),
        len: next.len(),
    };
    let priority_slice = MetadataSlice {
        ptr: core::ptr::NonNull::new(priorities.as_mut_ptr()).expect("priority slice pointer"),
        len: priorities.len(),
    };
    let enqueue_epoch_slice = MetadataSlice {
        ptr: core::ptr::NonNull::new(enqueue_epochs.as_mut_ptr())
            .expect("enqueue epoch slice pointer"),
        len: enqueue_epochs.len(),
    };
    let mut queue = MetadataPriorityQueue::new(
        bucket_slice,
        next_slice,
        priority_slice,
        enqueue_epoch_slice,
        None,
    )
    .expect("priority queue builds");

    queue
        .enqueue(1, FiberTaskPriority::new(-5))
        .expect("low-priority item should enqueue");
    queue
        .enqueue(2, FiberTaskPriority::DEFAULT)
        .expect("default-priority item should enqueue");
    queue
        .enqueue(3, FiberTaskPriority::new(10))
        .expect("high-priority item should enqueue");

    assert_eq!(queue.dequeue(), Some(3));
    assert_eq!(queue.dequeue(), Some(2));
    assert_eq!(queue.dequeue(), Some(1));
    assert_eq!(queue.dequeue(), None);
}

#[test]
fn priority_queue_aging_eventually_promotes_waiting_work() {
    let _guard = crate::thread::runtime_test_guard();
    let mut buckets = [PriorityBucket::empty(); FIBER_PRIORITY_LEVELS];
    let mut next = [EMPTY_QUEUE_SLOT; 8];
    let mut priorities = [FiberTaskPriority::DEFAULT.get(); 8];
    let mut enqueue_epochs = [0u64; 8];
    let bucket_slice = MetadataSlice {
        ptr: core::ptr::NonNull::new(buckets.as_mut_ptr()).expect("bucket slice pointer"),
        len: buckets.len(),
    };
    let next_slice = MetadataSlice {
        ptr: core::ptr::NonNull::new(next.as_mut_ptr()).expect("next slice pointer"),
        len: next.len(),
    };
    let priority_slice = MetadataSlice {
        ptr: core::ptr::NonNull::new(priorities.as_mut_ptr()).expect("priority slice pointer"),
        len: priorities.len(),
    };
    let enqueue_epoch_slice = MetadataSlice {
        ptr: core::ptr::NonNull::new(enqueue_epochs.as_mut_ptr())
            .expect("enqueue epoch slice pointer"),
        len: enqueue_epochs.len(),
    };
    let mut queue = MetadataPriorityQueue::new(
        bucket_slice,
        next_slice,
        priority_slice,
        enqueue_epoch_slice,
        None,
    )
    .expect("priority queue builds");

    queue
        .enqueue(1, FiberTaskPriority::DEFAULT)
        .expect("default-priority task should enqueue");
    queue
        .enqueue(2, FiberTaskPriority::new(1))
        .expect("slightly higher-priority task should enqueue");

    assert_eq!(queue.dequeue(), Some(2));
    assert_eq!(queue.waiting_age(1), FiberTaskAge(1));

    queue
        .enqueue(3, FiberTaskPriority::new(1))
        .expect("another slightly higher-priority task should enqueue");
    assert_eq!(queue.dequeue(), Some(3));
    assert_eq!(queue.waiting_age(1), FiberTaskAge(2));

    queue
        .enqueue(4, FiberTaskPriority::new(1))
        .expect("third slightly higher-priority task should enqueue");
    assert_eq!(queue.dequeue(), Some(1));
    assert_eq!(queue.waiting_age(1), FiberTaskAge::ZERO);
}

#[test]
fn priority_queue_prefers_higher_base_priority_when_effective_priorities_tie() {
    let _guard = crate::thread::runtime_test_guard();
    let mut buckets = [PriorityBucket::empty(); FIBER_PRIORITY_LEVELS];
    let mut next = [EMPTY_QUEUE_SLOT; 8];
    let mut priorities = [FiberTaskPriority::DEFAULT.get(); 8];
    let mut enqueue_epochs = [0u64; 8];
    let bucket_slice = MetadataSlice {
        ptr: core::ptr::NonNull::new(buckets.as_mut_ptr()).expect("bucket slice pointer"),
        len: buckets.len(),
    };
    let next_slice = MetadataSlice {
        ptr: core::ptr::NonNull::new(next.as_mut_ptr()).expect("next slice pointer"),
        len: next.len(),
    };
    let priority_slice = MetadataSlice {
        ptr: core::ptr::NonNull::new(priorities.as_mut_ptr()).expect("priority slice pointer"),
        len: priorities.len(),
    };
    let enqueue_epoch_slice = MetadataSlice {
        ptr: core::ptr::NonNull::new(enqueue_epochs.as_mut_ptr())
            .expect("enqueue epoch slice pointer"),
        len: enqueue_epochs.len(),
    };
    let mut queue = MetadataPriorityQueue::new(
        bucket_slice,
        next_slice,
        priority_slice,
        enqueue_epoch_slice,
        None,
    )
    .expect("priority queue builds");

    queue
        .enqueue(1, FiberTaskPriority::DEFAULT)
        .expect("default-priority task should enqueue");
    queue
        .enqueue(7, FiberTaskPriority::new(10))
        .expect("high-priority task should enqueue");
    assert_eq!(queue.dequeue(), Some(7));

    queue
        .enqueue(2, FiberTaskPriority::new(1))
        .expect("slightly higher-priority task should enqueue");

    assert_eq!(queue.waiting_age(1), FiberTaskAge(1));
    assert_eq!(queue.waiting_age(2), FiberTaskAge::ZERO);
    assert_eq!(queue.effective_priority(1), FiberTaskPriority::new(1));
    assert_eq!(queue.effective_priority(2), FiberTaskPriority::new(1));
    assert_eq!(queue.dequeue(), Some(2));
    assert_eq!(queue.dequeue(), Some(1));
}

#[test]
fn priority_queue_preserves_fifo_order_within_one_priority_bucket() {
    let _guard = crate::thread::runtime_test_guard();
    let mut buckets = [PriorityBucket::empty(); FIBER_PRIORITY_LEVELS];
    let mut next = [EMPTY_QUEUE_SLOT; 8];
    let mut priorities = [FiberTaskPriority::DEFAULT.get(); 8];
    let mut enqueue_epochs = [0u64; 8];
    let bucket_slice = MetadataSlice {
        ptr: core::ptr::NonNull::new(buckets.as_mut_ptr()).expect("bucket slice pointer"),
        len: buckets.len(),
    };
    let next_slice = MetadataSlice {
        ptr: core::ptr::NonNull::new(next.as_mut_ptr()).expect("next slice pointer"),
        len: next.len(),
    };
    let priority_slice = MetadataSlice {
        ptr: core::ptr::NonNull::new(priorities.as_mut_ptr()).expect("priority slice pointer"),
        len: priorities.len(),
    };
    let enqueue_epoch_slice = MetadataSlice {
        ptr: core::ptr::NonNull::new(enqueue_epochs.as_mut_ptr())
            .expect("enqueue epoch slice pointer"),
        len: enqueue_epochs.len(),
    };
    let mut queue = MetadataPriorityQueue::new(
        bucket_slice,
        next_slice,
        priority_slice,
        enqueue_epoch_slice,
        None,
    )
    .expect("priority queue builds");

    queue
        .enqueue(1, FiberTaskPriority::new(3))
        .expect("first task should enqueue");
    queue
        .enqueue(2, FiberTaskPriority::new(3))
        .expect("second task should enqueue");
    queue
        .enqueue(3, FiberTaskPriority::new(3))
        .expect("third task should enqueue");

    assert_eq!(queue.dequeue(), Some(1));
    assert_eq!(queue.dequeue(), Some(2));
    assert_eq!(queue.dequeue(), Some(3));
}

#[test]
fn priority_queue_age_cap_limits_virtual_promotion() {
    let _guard = crate::thread::runtime_test_guard();
    let mut buckets = [PriorityBucket::empty(); FIBER_PRIORITY_LEVELS];
    let mut next = [EMPTY_QUEUE_SLOT; 8];
    let mut priorities = [FiberTaskPriority::DEFAULT.get(); 8];
    let mut enqueue_epochs = [0u64; 8];
    let bucket_slice = MetadataSlice {
        ptr: core::ptr::NonNull::new(buckets.as_mut_ptr()).expect("bucket slice pointer"),
        len: buckets.len(),
    };
    let next_slice = MetadataSlice {
        ptr: core::ptr::NonNull::new(next.as_mut_ptr()).expect("next slice pointer"),
        len: next.len(),
    };
    let priority_slice = MetadataSlice {
        ptr: core::ptr::NonNull::new(priorities.as_mut_ptr()).expect("priority slice pointer"),
        len: priorities.len(),
    };
    let enqueue_epoch_slice = MetadataSlice {
        ptr: core::ptr::NonNull::new(enqueue_epochs.as_mut_ptr())
            .expect("enqueue epoch slice pointer"),
        len: enqueue_epochs.len(),
    };
    let mut queue = MetadataPriorityQueue::new(
        bucket_slice,
        next_slice,
        priority_slice,
        enqueue_epoch_slice,
        Some(FiberTaskAgeCap::new(1)),
    )
    .expect("priority queue builds");

    queue
        .enqueue(1, FiberTaskPriority::new(-4))
        .expect("low-priority task should enqueue");
    queue
        .enqueue(2, FiberTaskPriority::new(1))
        .expect("higher-priority task should enqueue");

    assert_eq!(queue.dequeue(), Some(2));
    assert_eq!(queue.waiting_age(1), FiberTaskAge(1));
    assert_eq!(queue.effective_priority(1), FiberTaskPriority::new(-3));

    queue
        .enqueue(3, FiberTaskPriority::new(1))
        .expect("second higher-priority task should enqueue");
    assert_eq!(queue.dequeue(), Some(3));
    assert_eq!(queue.waiting_age(1), FiberTaskAge(1));
    assert_eq!(queue.dequeue(), Some(1));
}

#[test]
fn green_exclusion_span_guard_tracks_active_spans_and_blocks_yield() {
    let _guard = crate::thread::runtime_test_guard();
    const REQUIRED_LEAF: [u32; 2] = [1_u32 << 6, 0];
    const REQUIRED_ROOT: [u32; 1] = [1_u32 << 0];
    const REQUIRED_LEVELS: [&[u32]; 1] = [&REQUIRED_ROOT];
    const REQUIRED_TREE: CooperativeExclusionSummaryTree =
        CooperativeExclusionSummaryTree::new(&REQUIRED_LEAF, &REQUIRED_LEVELS);

    let carrier = ThreadPool::new(&ThreadPoolConfig {
        min_threads: 1,
        max_threads: 1,
        placement: PoolPlacement::Inherit,
        ..ThreadPoolConfig::new()
    })
    .expect("single-carrier pool should build");
    let fibers =
        GreenPool::new(&FiberPoolConfig::new(), &carrier).expect("green pool should build");

    let task = fibers
        .spawn_with_stack::<4096, _, _>(move || -> Result<(), FiberError> {
            let span = CooperativeExclusionSpan::new(7).map_err(fiber_error_from_sync)?;
            let _guard = enter_current_green_exclusion_span(span).map_err(fiber_error_from_sync)?;
            let mut active = [span; 4];
            let copied = current_green_exclusion_spans(&mut active);
            assert_eq!(copied, 1);
            assert_eq!(active[0], span);
            assert!(current_green_exclusion_allows(&[]));
            assert!(current_green_exclusion_allows(&[
                CooperativeExclusionSpan::new(9).map_err(fiber_error_from_sync)?
            ]));
            assert!(!current_green_exclusion_allows(&[span]));
            assert!(!current_green_exclusion_allows_tree(&REQUIRED_TREE));
            let error = yield_now().expect_err("yield should reject while exclusion span held");
            assert_eq!(error.kind(), FiberError::state_conflict().kind());
            Ok(())
        })
        .expect("task should spawn");

    task.join()
        .expect("task should complete without runtime failure")
        .expect("task should observe the expected span behavior");
    fibers
        .shutdown()
        .expect("green pool should shut down cleanly");
    carrier
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[test]
fn cooperative_exclusion_summary_tree_matches_named_spans() {
    let _guard = crate::thread::runtime_test_guard();
    const LEAF: [u32; 33] = {
        let mut words = [0_u32; 33];
        words[0] = 1_u32 << 2;
        words[32] = 1_u32 << 0;
        words
    };
    const LEVEL_ONE: [u32; 2] = [1_u32 << 0, 1_u32 << 0];
    const ROOT: [u32; 1] = [(1_u32 << 0) | (1_u32 << 1)];
    const LEVELS: [&[u32]; 2] = [&LEVEL_ONE, &ROOT];
    const TREE: CooperativeExclusionSummaryTree =
        CooperativeExclusionSummaryTree::new(&LEAF, &LEVELS);

    assert_eq!(
        TREE.span_capacity(),
        33 * COOPERATIVE_EXCLUSION_TREE_WORD_BITS
    );
    assert!(
        TREE.contains(
            CooperativeExclusionSpan::new(3).expect("span identifiers should be non-zero")
        )
    );
    assert!(TREE.contains(
        CooperativeExclusionSpan::new(1025).expect("span identifiers should be non-zero")
    ));
    assert!(
        !TREE.contains(
            CooperativeExclusionSpan::new(4).expect("span identifiers should be non-zero")
        )
    );
    assert!(!TREE.contains(
        CooperativeExclusionSpan::new(2048).expect("span identifiers should be non-zero")
    ));
}

#[test]
fn green_exclusion_summary_tree_falls_back_honestly_for_spans_beyond_fast_cache() {
    let _guard = crate::thread::runtime_test_guard();
    const LEAF: [u32; 33] = {
        let mut words = [0_u32; 33];
        words[32] = 1_u32 << 0;
        words
    };
    const LEVEL_ONE: [u32; 2] = [0, 1_u32 << 0];
    const ROOT: [u32; 1] = [1_u32 << 1];
    const LEVELS: [&[u32]; 2] = [&LEVEL_ONE, &ROOT];
    const TREE: CooperativeExclusionSummaryTree =
        CooperativeExclusionSummaryTree::new(&LEAF, &LEVELS);

    let carrier = ThreadPool::new(&ThreadPoolConfig {
        min_threads: 1,
        max_threads: 1,
        placement: PoolPlacement::Inherit,
        ..ThreadPoolConfig::new()
    })
    .expect("single-carrier pool should build");
    let fibers =
        GreenPool::new(&FiberPoolConfig::new(), &carrier).expect("green pool should build");

    let task = fibers
        .spawn_with_stack::<4096, _, _>(move || -> Result<(), FiberError> {
            let span = CooperativeExclusionSpan::new(1025).map_err(fiber_error_from_sync)?;
            let _guard = enter_current_green_exclusion_span(span).map_err(fiber_error_from_sync)?;
            assert!(!current_green_exclusion_allows_tree(&TREE));
            Ok(())
        })
        .expect("task should spawn");

    task.join()
        .expect("task should complete without runtime failure")
        .expect("task should observe the overflow fallback");
    fibers
        .shutdown()
        .expect("green pool should shut down cleanly");
    carrier
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[test]
fn green_yield_rejects_when_cooperative_mutex_is_held() {
    let _guard = crate::thread::runtime_test_guard();
    let carrier = ThreadPool::new(&ThreadPoolConfig {
        min_threads: 1,
        max_threads: 1,
        placement: PoolPlacement::Inherit,
        ..ThreadPoolConfig::new()
    })
    .expect("single-carrier pool should build");
    let fibers =
        GreenPool::new(&FiberPoolConfig::new(), &carrier).expect("green pool should build");
    let lock = Arc::new(FusionMutex::new(()));

    let task = fibers
        .spawn_with_stack::<4096, _, _>({
            let lock = Arc::clone(&lock);
            move || -> Result<(), FiberError> {
                let _guard = lock.lock().map_err(fiber_error_from_sync)?;
                let error =
                    yield_now().expect_err("yield should reject while cooperative lock held");
                assert_eq!(error.kind(), FiberError::state_conflict().kind());
                Ok(())
            }
        })
        .expect("task should spawn");

    task.join()
        .expect("task should complete without runtime failure")
        .expect("task should observe the expected yield rejection");
    fibers
        .shutdown()
        .expect("green pool should shut down cleanly");
    carrier
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[test]
fn ranked_green_mutexes_reject_descending_acquisition_order() {
    let _guard = crate::thread::runtime_test_guard();
    let carrier = ThreadPool::new(&ThreadPoolConfig {
        min_threads: 1,
        max_threads: 1,
        placement: PoolPlacement::Inherit,
        ..ThreadPoolConfig::new()
    })
    .expect("single-carrier pool should build");
    let fibers =
        GreenPool::new(&FiberPoolConfig::new(), &carrier).expect("green pool should build");
    let low = Arc::new(FusionMutex::ranked(
        (),
        crate::sync::CooperativeLockRank::new(1).expect("rank one should be valid"),
    ));
    let high = Arc::new(FusionMutex::ranked(
        (),
        crate::sync::CooperativeLockRank::new(2).expect("rank two should be valid"),
    ));

    let task = fibers
        .spawn_with_stack::<4096, _, _>({
            let low = Arc::clone(&low);
            let high = Arc::clone(&high);
            move || -> Result<(), FiberError> {
                let _high_guard = high.lock().map_err(fiber_error_from_sync)?;
                let Err(error) = low.lock() else {
                    panic!("descending ranked acquisition should be rejected");
                };
                assert_eq!(error.kind, SyncErrorKind::Invalid);
                Ok(())
            }
        })
        .expect("task should spawn");

    task.join()
        .expect("task should complete without runtime failure")
        .expect("task should observe the expected ranked-lock rejection");
    fibers
        .shutdown()
        .expect("green pool should shut down cleanly");
    carrier
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[test]
fn ranked_green_mutexes_allow_ascending_acquisition_order() {
    let _guard = crate::thread::runtime_test_guard();
    let carrier = ThreadPool::new(&ThreadPoolConfig {
        min_threads: 1,
        max_threads: 1,
        placement: PoolPlacement::Inherit,
        ..ThreadPoolConfig::new()
    })
    .expect("single-carrier pool should build");
    let fibers =
        GreenPool::new(&FiberPoolConfig::new(), &carrier).expect("green pool should build");
    let low = Arc::new(FusionMutex::ranked(
        (),
        crate::sync::CooperativeLockRank::new(1).expect("rank one should be valid"),
    ));
    let high = Arc::new(FusionMutex::ranked(
        (),
        crate::sync::CooperativeLockRank::new(2).expect("rank two should be valid"),
    ));

    let task = fibers
        .spawn_with_stack::<4096, _, _>({
            let low = Arc::clone(&low);
            let high = Arc::clone(&high);
            move || -> Result<(), FiberError> {
                let _low_guard = low.lock().map_err(fiber_error_from_sync)?;
                let _high_guard = high.lock().map_err(fiber_error_from_sync)?;
                assert_eq!(current_green_cooperative_lock_depth(), 2);
                Ok(())
            }
        })
        .expect("task should spawn");

    task.join()
        .expect("task should complete without runtime failure")
        .expect("ascending ranked acquisition should succeed");
    fibers
        .shutdown()
        .expect("green pool should shut down cleanly");
    carrier
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[test]
fn green_join_rejects_when_cooperative_mutex_is_held() {
    let _guard = crate::thread::runtime_test_guard();
    let carrier = ThreadPool::new(&ThreadPoolConfig {
        min_threads: 1,
        max_threads: 1,
        placement: PoolPlacement::Inherit,
        ..ThreadPoolConfig::new()
    })
    .expect("single-carrier pool should build");
    let fibers = GreenPool::new(
        &FiberPoolConfig::new().with_scheduling(GreenScheduling::Priority),
        &carrier,
    )
    .expect("priority green pool should build");
    let lock = Arc::new(FusionMutex::new(()));
    let child_ran = Arc::new(AtomicBool::new(false));
    let pool_for_parent = fibers
        .try_clone()
        .expect("green pool handle should clone for in-fiber spawn");
    let parent = fibers
        .spawn_with_attrs(
            FiberTaskAttributes::new(FiberStackClass::MIN)
                .with_priority(FiberTaskPriority::new(10)),
            {
                let lock = Arc::clone(&lock);
                let child_ran = Arc::clone(&child_ran);
                move || -> Result<(), FiberError> {
                    let child = pool_for_parent.spawn_with_attrs(
                        FiberTaskAttributes::new(FiberStackClass::MIN)
                            .with_priority(FiberTaskPriority::new(-10)),
                        move || {
                            child_ran.store(true, Ordering::Release);
                        },
                    )?;
                    let _guard = lock.lock().map_err(fiber_error_from_sync)?;
                    let error = child
                        .join()
                        .expect_err("join should reject while cooperative lock held");
                    assert_eq!(error.kind(), FiberError::state_conflict().kind());
                    Ok(())
                }
            },
        )
        .expect("parent should spawn");

    parent
        .join()
        .expect("parent should complete without runtime failure")
        .expect("parent should observe the expected join rejection");
    for _ in 0..1_000 {
        if child_ran.load(Ordering::Acquire) {
            break;
        }
        std::thread::yield_now();
    }
    assert!(
        child_ran.load(Ordering::Acquire),
        "child should still run after the parent join attempt is rejected"
    );

    fibers
        .shutdown()
        .expect("green pool should shut down cleanly");
    carrier
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[test]
fn green_task_yield_budget_faults_after_overrun_and_yield() {
    let _guard = crate::thread::runtime_test_guard();
    YIELD_BUDGET_EVENT_CALLS.store(0, Ordering::Release);
    LAST_YIELD_BUDGET_FIBER_ID.store(0, Ordering::Release);
    LAST_YIELD_BUDGET_CARRIER_ID.store(usize::MAX, Ordering::Release);
    LAST_YIELD_BUDGET_NANOS.store(0, Ordering::Release);
    LAST_YIELD_OBSERVED_NANOS.store(0, Ordering::Release);

    let carrier = ThreadPool::new(&ThreadPoolConfig {
        min_threads: 1,
        max_threads: 1,
        placement: PoolPlacement::Inherit,
        ..ThreadPoolConfig::new()
    })
    .expect("single-carrier pool should build");
    let fibers = GreenPool::new(
        &FiberPoolConfig::new()
            .with_yield_budget_policy(FiberYieldBudgetPolicy::Notify(record_yield_budget_event)),
        &carrier,
    )
    .expect("green pool should build");

    let task = fibers
        .spawn_with_attrs(
            FiberTaskAttributes::new(FiberStackClass::MIN)
                .with_yield_budget(Duration::from_millis(5)),
            || {
                std::thread::sleep(Duration::from_millis(15));
                yield_now().expect("task should still be able to yield after long segment");
            },
        )
        .expect("budgeted task should spawn");
    let task_id = task.id();

    let error = task
        .join()
        .expect_err("run-between-yield overrun should fault the task");
    assert_eq!(error.kind(), FiberError::deadline_exceeded().kind());
    assert_eq!(YIELD_BUDGET_EVENT_CALLS.load(Ordering::Acquire), 1);
    assert_eq!(
        LAST_YIELD_BUDGET_FIBER_ID.load(Ordering::Acquire),
        usize::try_from(task_id).unwrap_or(usize::MAX)
    );
    assert_eq!(LAST_YIELD_BUDGET_CARRIER_ID.load(Ordering::Acquire), 0);
    assert!(
        LAST_YIELD_OBSERVED_NANOS.load(Ordering::Acquire)
            >= LAST_YIELD_BUDGET_NANOS.load(Ordering::Acquire)
    );

    fibers
        .shutdown()
        .expect("green pool should shut down cleanly");
    carrier
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[test]
fn watchdog_faults_non_yielding_green_budget_overrun() {
    let _guard = crate::thread::runtime_test_guard();
    YIELD_BUDGET_EVENT_CALLS.store(0, Ordering::Release);
    LAST_YIELD_BUDGET_FIBER_ID.store(0, Ordering::Release);
    LAST_YIELD_BUDGET_CARRIER_ID.store(usize::MAX, Ordering::Release);
    LAST_YIELD_BUDGET_NANOS.store(0, Ordering::Release);
    LAST_YIELD_OBSERVED_NANOS.store(0, Ordering::Release);

    let carrier = ThreadPool::new(&ThreadPoolConfig {
        min_threads: 1,
        max_threads: 1,
        placement: PoolPlacement::Inherit,
        ..ThreadPoolConfig::new()
    })
    .expect("single-carrier pool should build");
    let fibers = GreenPool::new(
        &FiberPoolConfig::new()
            .with_yield_budget_policy(FiberYieldBudgetPolicy::Notify(record_yield_budget_event)),
        &carrier,
    )
    .expect("green pool should build");

    let task = fibers
        .spawn_with_attrs(
            FiberTaskAttributes::new(FiberStackClass::MIN)
                .with_yield_budget(Duration::from_millis(5)),
            || {
                std::thread::sleep(Duration::from_millis(25));
            },
        )
        .expect("budgeted task should spawn");
    let task_id = task.id();

    let error = task
        .join()
        .expect_err("watchdog should fault one non-yielding overrun");
    assert_eq!(error.kind(), FiberError::deadline_exceeded().kind());
    assert_eq!(YIELD_BUDGET_EVENT_CALLS.load(Ordering::Acquire), 1);
    assert_eq!(
        LAST_YIELD_BUDGET_FIBER_ID.load(Ordering::Acquire),
        usize::try_from(task_id).unwrap_or(usize::MAX)
    );
    assert_eq!(LAST_YIELD_BUDGET_CARRIER_ID.load(Ordering::Acquire), 0);
    assert!(
        LAST_YIELD_OBSERVED_NANOS.load(Ordering::Acquire)
            >= LAST_YIELD_BUDGET_NANOS.load(Ordering::Acquire)
    );

    fibers
        .shutdown()
        .expect("green pool should shut down cleanly");
    carrier
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[test]
fn work_stealing_runs_ready_work_on_an_idle_carrier() {
    if GreenPool::support().context.migration != ContextMigrationSupport::CrossCarrier {
        return;
    }

    let _guard = crate::thread::runtime_test_guard();
    let carrier = ThreadPool::new(&ThreadPoolConfig {
        min_threads: 2,
        max_threads: 2,
        placement: PoolPlacement::Inherit,
        ..ThreadPoolConfig::new()
    })
    .expect("two-carrier pool should build");
    let fibers = GreenPool::new(
        &FiberPoolConfig {
            scheduling: GreenScheduling::WorkStealing,
            spawn_locality_policy: CarrierSpawnLocalityPolicy::SameCore,
            growth_chunk: 4,
            max_fibers_per_carrier: 4,
            ..FiberPoolConfig::new()
        },
        &carrier,
    )
    .expect("work-stealing fiber pool should build");

    let first_thread = Arc::new(StdMutex::new(None));
    let second_thread = Arc::new(StdMutex::new(None));
    let started = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));

    fibers.inner.next_carrier.store(0, Ordering::Release);
    let blocker = fibers
        .spawn_with_stack::<4096, _, _>({
            let first_thread = Arc::clone(&first_thread);
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            move || {
                *first_thread
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) =
                    Some(std::thread::current().id());
                started.store(true, Ordering::Release);
                while !release.load(Ordering::Acquire) {
                    std::thread::yield_now();
                }
            }
        })
        .expect("blocking fiber should spawn");
    while !started.load(Ordering::Acquire) {
        std::thread::yield_now();
    }

    fibers.inner.next_carrier.store(0, Ordering::Release);
    let stolen = fibers
        .spawn_with_stack::<4096, _, _>({
            let second_thread = Arc::clone(&second_thread);
            move || {
                *second_thread
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) =
                    Some(std::thread::current().id());
            }
        })
        .expect("second fiber should spawn onto the busy source carrier");
    stolen
        .join()
        .expect("idle carrier should steal and complete ready work");

    release.store(true, Ordering::Release);
    blocker
        .join()
        .expect("blocking fiber should finish after release");

    let first = first_thread
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .expect("first fiber should record a carrier thread");
    let second = second_thread
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .expect("stolen fiber should record a carrier thread");
    assert_ne!(first, second);

    fibers
        .shutdown()
        .expect("work-stealing fiber pool should shut down cleanly");
    carrier
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[test]
fn hosted_spawn_from_running_fiber_prefers_origin_carrier_locality() {
    let _guard = crate::thread::runtime_test_guard();
    let carrier = ThreadPool::new(&ThreadPoolConfig {
        min_threads: 2,
        max_threads: 2,
        placement: PoolPlacement::Inherit,
        ..ThreadPoolConfig::new()
    })
    .expect("two-carrier pool should build");
    let fibers = GreenPool::new(
        &FiberPoolConfig::new().with_scheduling(GreenScheduling::Fifo),
        &carrier,
    )
    .expect("fifo fiber pool should build");

    let parent_thread = Arc::new(StdMutex::new(None));
    let child_thread = Arc::new(StdMutex::new(None));
    let child_fibers = fibers.try_clone().expect("pool handle should clone");

    fibers.inner.next_carrier.store(0, Ordering::Release);
    let parent = fibers
        .spawn_with_stack::<4096, _, _>({
            let parent_thread = Arc::clone(&parent_thread);
            let child_thread = Arc::clone(&child_thread);
            move || {
                *parent_thread
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) =
                    Some(std::thread::current().id());
                child_fibers
                    .spawn_with_stack::<4096, _, _>(move || {
                        *child_thread
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner) =
                            Some(std::thread::current().id());
                    })
                    .expect("child fiber should spawn from running parent")
            }
        })
        .expect("parent fiber should spawn");

    let child = parent
        .join()
        .expect("parent fiber should complete without runtime failure");
    child
        .join()
        .expect("child fiber should complete without runtime failure");

    let parent = parent_thread
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .expect("parent fiber should record a carrier thread");
    let child = child_thread
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .expect("child fiber should record a carrier thread");
    assert_eq!(parent, child);

    fibers
        .shutdown()
        .expect("fifo fiber pool should shut down cleanly");
    carrier
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[test]
fn automatic_huge_page_policy_tracks_backend_support_and_reservation_size() {
    let _guard = crate::thread::runtime_test_guard();
    let small = automatic_huge_page_policy(FiberStackBacking::Elastic {
        initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
        max_size: NonZeroUsize::new(64 * 1024).expect("non-zero max stack"),
    });
    assert_eq!(small, HugePagePolicy::Disabled);

    let large = automatic_huge_page_policy(FiberStackBacking::Elastic {
        initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
        max_size: NonZeroUsize::new(4 * 1024 * 1024).expect("non-zero max stack"),
    });
    let expected = if system_mem()
        .support()
        .advice
        .contains(MemAdviceCaps::HUGE_PAGE)
    {
        HugePagePolicy::Enabled {
            size: HugePageSize::TwoMiB,
        }
    } else {
        HugePagePolicy::Disabled
    };
    assert_eq!(large, expected);
}

#[path = "tests_runtime/tests_runtime.rs"]
mod tests_runtime;
