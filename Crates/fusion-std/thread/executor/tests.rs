use core::num::NonZeroUsize;
use core::sync::atomic::{
    AtomicUsize,
    Ordering,
};

use std::sync::Arc;
#[cfg(feature = "std")]
use std::sync::atomic::AtomicBool;
#[cfg(feature = "std")]
use std::task::Wake;
#[cfg(feature = "std")]
use std::thread;
#[cfg(feature = "std")]
use std::time::Duration;

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
};
use fusion_sys::context::{
    ContextCaps,
    ContextKind,
};
use fusion_sys::courier::{
    CourierCaps,
    CourierPlan,
    CourierVisibility,
};
use fusion_sys::domain::{
    ContextDescriptor,
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
    ResourceResidencySupport,
    ResourceState,
    ResourceSupport,
    SharingPolicy,
    StateValue,
};
use fusion_sys::thread::{
    ThreadLogicalCpuId,
    ThreadProcessorGroupId,
};

use crate::thread::{
    PoolPlacement,
    ThreadPoolConfig,
};
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

#[test]
fn backing_plan_memory_footprint_matches_domain_requests() {
    let config = ExecutorConfig::new().with_capacity(2);
    let plan = CurrentAsyncRuntime::backing_plan(config).expect("backing plan should build");
    let footprint = plan.memory_footprint();

    assert_eq!(footprint.control_bytes, plan.control.bytes);
    assert_eq!(footprint.reactor_bytes, plan.reactor.bytes);
    assert_eq!(footprint.registry_bytes, plan.registry.bytes);
    assert_eq!(footprint.spill_bytes, plan.spill.bytes);
    assert_eq!(footprint.packing_padding_bytes, 0);
    assert_eq!(
        footprint.total_bytes(),
        plan.control.bytes + plan.reactor.bytes + plan.registry.bytes + plan.spill.bytes
    );
}

#[test]
fn combined_backing_plan_memory_footprint_captures_padding() {
    let config = ExecutorConfig::new().with_capacity(2);
    let combined = CurrentAsyncRuntime::backing_plan_with_layout_policy(
        config,
        AllocatorLayoutPolicy::exact_static(),
    )
    .expect("backing plan should build")
    .combined_eager()
    .expect("combined eager plan should build");
    let footprint = combined.memory_footprint();
    let domain_bytes = combined.control.len
        + combined.reactor.len
        + combined.registry.len
        + combined.spill.map_or(0, |range| range.len);

    assert_eq!(footprint.domain_bytes(), domain_bytes);
    assert_eq!(footprint.total_bytes(), combined.slab.bytes);
    assert_eq!(
        footprint.packing_padding_bytes,
        combined.slab.bytes.saturating_sub(domain_bytes)
    );
}

struct ExplicitGeneratedPollStackFuture;

impl Future for ExplicitGeneratedPollStackFuture {
    type Output = u8;

    fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(55)
    }
}

crate::declare_generated_async_poll_stack_contract!(ExplicitGeneratedPollStackFuture, 1792);
const TEST_ASYNC_POLL_STACK_BYTES: usize = 2048;

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
        let rc = create_nonblocking_cloexec_pipe(&mut fds);
        assert_eq!(rc, 0, "test pipe should create");
        Self {
            read_fd: fds[0],
            write_fd: fds[1],
        }
    }

    fn source(&self) -> EventSourceHandle {
        EventSourceHandle(usize::try_from(self.read_fd).expect("pipe fd should be non-negative"))
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
            let errno = last_errno();
            if errno == libc::EINTR {
                continue;
            }
            panic!("pipe read should complete after readiness, errno={errno}");
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn create_nonblocking_cloexec_pipe(fds: &mut [i32; 2]) -> i32 {
    unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) }
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn create_nonblocking_cloexec_pipe(fds: &mut [i32; 2]) -> i32 {
    let rc = unsafe { libc::pipe(fds.as_mut_ptr()) };
    if rc != 0 {
        return rc;
    }

    for &fd in fds.iter() {
        let current = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if current < 0 {
            return -1;
        }
        if unsafe { libc::fcntl(fd, libc::F_SETFL, current | libc::O_NONBLOCK) } < 0 {
            return -1;
        }

        let current_fd = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        if current_fd < 0 {
            return -1;
        }
        if unsafe { libc::fcntl(fd, libc::F_SETFD, current_fd | libc::FD_CLOEXEC) } < 0 {
            return -1;
        }
    }
    0
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn last_errno() -> i32 {
    unsafe { *libc::__errno_location() }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn last_errno() -> i32 {
    unsafe { *libc::__error() }
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

#[cfg(feature = "std")]
const fn is_unsupported_executor_error(error: ExecutorError) -> bool {
    matches!(error, ExecutorError::Unsupported)
}

#[cfg(feature = "std")]
const fn is_unsupported_fiber_error(error: fusion_sys::fiber::FiberError) -> bool {
    matches!(error.kind(), fusion_sys::fiber::FiberErrorKind::Unsupported)
}

#[test]
fn registry_reuses_slots_with_new_generations() {
    let executor = Executor::new(ExecutorConfig::new());

    let first = executor
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async { 7_u8 })
        .expect("first task should spawn");
    let first_slot = first.inner.slot_index;
    let first_generation = first.inner.generation;
    assert_eq!(first.join().expect("first task should finish"), 7);

    let second = executor
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async { 9_u8 })
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
        .spawn_with_poll_stack_bytes(&executor, TEST_ASYNC_POLL_STACK_BYTES, async { 3_u8 })
        .expect("first join-set task should spawn");
    join_set
        .spawn_with_poll_stack_bytes(&executor, TEST_ASYNC_POLL_STACK_BYTES, async { 5_u8 })
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
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async move {
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
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async { [1_u16, 2, 3, 4] })
        .expect("task should spawn");
    let admission = handle.admission();
    assert_eq!(admission.carrier, ExecutorMode::CurrentThread);
    assert_eq!(admission.future_bytes, size_of_val(&sample));
    assert_eq!(admission.future_align, core::mem::align_of_val(&sample));
    assert_eq!(admission.output_bytes, size_of::<[u16; 4]>());
    assert_eq!(admission.output_align, align_of::<[u16; 4]>());
    assert_eq!(
        admission.poll_stack,
        AsyncPollStackContract::Explicit {
            bytes: TEST_ASYNC_POLL_STACK_BYTES
        }
    );
    assert_eq!(
        handle.join().expect("task should complete"),
        [1_u16, 2, 3, 4]
    );
}

#[test]
fn task_handle_reports_exact_backing_and_poll_stack_contract() {
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
    assert_eq!(admission.future_bytes, size_of_val(&sample));
    assert_eq!(admission.future_align, align_of_val(&sample));
    assert_eq!(admission.output_bytes, size_of::<[u8; 384]>());
    assert_eq!(admission.output_align, align_of::<[u8; 384]>());
    assert_eq!(admission.exact_backing_bytes, size_of_val(&sample));
    assert_eq!(
        admission.exact_backing_align,
        align_of_val(&sample).max(align_of::<[u8; 384]>())
    );
    assert_eq!(
        admission.poll_stack,
        AsyncPollStackContract::Explicit { bytes: 1536 }
    );
    assert_eq!(handle.join().expect("task should complete"), [7_u8; 384]);
}

#[test]
fn exact_backing_tracks_larger_output_shape() {
    let executor = Executor::new(ExecutorConfig::new());
    let sample = async { [9_u8; 384] };
    let handle = executor
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async { [9_u8; 384] })
        .expect("task should spawn");
    let admission = handle.admission();

    assert_eq!(admission.future_bytes, size_of_val(&sample));
    assert_eq!(admission.output_bytes, size_of::<[u8; 384]>());
    assert_eq!(admission.exact_backing_bytes, size_of::<[u8; 384]>());
    assert_eq!(
        admission.exact_backing_align,
        align_of_val(&sample).max(align_of::<[u8; 384]>())
    );
    assert_eq!(handle.join().expect("task should complete"), [9_u8; 384]);
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
fn missing_generated_async_poll_stack_contract_is_rejected_by_default() {
    let executor = Executor::new(ExecutorConfig::new());
    let payload = [0_u8; 384];
    assert!(matches!(
        executor.spawn(async move {
            let _ = payload[0];
            5_u8
        }),
        Err(ExecutorError::Unsupported)
    ));
}

#[test]
fn run_until_idle_drains_ready_current_thread_tasks() {
    let executor = Executor::new(ExecutorConfig::new());
    let handle = executor
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
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
fn executor_runtime_summary_reports_active_async_lane_state() {
    let executor = Executor::new(ExecutorConfig::new());
    let idle = executor
        .runtime_summary()
        .expect("summary should observe empty executor");
    assert_eq!(idle.total_active_units(), 0);
    assert_eq!(idle.run_state, CourierRunState::Idle);

    let handle = executor
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
            async_yield_now().await;
            13_u8
        })
        .expect("task should spawn");
    let active = executor
        .runtime_summary()
        .expect("summary should observe spawned task");
    assert!(active.async_lane.is_some());
    assert!(active.total_active_units() >= 1);
    assert!(
        matches!(
            active.run_state,
            CourierRunState::Runnable | CourierRunState::Running
        ),
        "spawned task should make the async lane runnable"
    );

    let _ = executor.run_until_idle().expect("executor should drain");
    assert_eq!(handle.join().expect("task should complete"), 13);
    let drained = executor
        .runtime_summary()
        .expect("summary should observe drained executor");
    assert_eq!(drained.total_active_units(), 0);
    assert_eq!(drained.run_state, CourierRunState::Idle);
}

#[test]
fn exact_future_spill_accepts_medium_future_frames() {
    let executor = Executor::new(ExecutorConfig::new());
    let sample_payload = [0_u8; 384];
    let sample = async move { sample_payload.len() };
    assert!(size_of_val(&sample) > INLINE_ASYNC_FUTURE_BYTES);

    let payload = [0_u8; 384];
    let handle = executor
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async move { payload.len() })
        .expect("medium-sized future should spill into exact leased backing");

    assert_eq!(handle.join().expect("task should complete"), 384);
}

#[test]
fn larger_futures_can_exceed_default_per_task_spill_budget_when_domain_has_room() {
    let executor = Executor::new(ExecutorConfig::new());
    let oversized = [0_u8; 64 * 1024];

    let handle = executor
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async move { oversized.len() })
        .expect("larger future frames should use the shared spill domain when it has room");

    assert_eq!(
        handle.admission().exact_backing_bytes,
        handle
            .admission()
            .future_bytes
            .max(handle.admission().output_bytes)
    );
    assert!(handle.admission().exact_backing_bytes > DEFAULT_ASYNC_SPILL_BUDGET_PER_TASK_BYTES);
    assert_eq!(
        handle.join().expect("task should complete"),
        oversized.len()
    );
}

#[test]
fn futures_without_one_spill_domain_are_rejected_honestly() {
    let spill_store = AsyncTaskSpillStore::new(true, None);
    let oversized = [0_u8; 2048];
    let mut future = InlineAsyncFutureStorage::empty();

    assert!(matches!(
        future.store_future(&spill_store, async move { oversized.len() }),
        Err(ExecutorError::Unsupported)
    ));
}

#[test]
fn exact_result_spill_accepts_medium_outputs() {
    let executor = Executor::new(ExecutorConfig::new());

    let handle = executor
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async move { [7_u8; 384] })
        .expect("medium-sized outputs should spill into exact leased backing");

    let output = handle.join().expect("task should complete");
    assert_eq!(output.len(), 384);
    assert!(output.iter().all(|byte| *byte == 7));
}

#[test]
fn future_and_result_share_one_exact_spill_envelope() {
    let request = ExecutorBackingRequest::from_extent_request(MemoryPoolExtentRequest {
        len: DEFAULT_ASYNC_SPILL_BUDGET_PER_TASK_BYTES * 2,
        align: default_async_spill_align(),
    })
    .expect("spill domain request should size honestly");
    let spill_store = AsyncTaskSpillStore::new(
        true,
        Some(
            ExecutorDomainAllocator::acquire_virtual(request, "fusion-executor-test-shared-spill")
                .expect("spill domain should build"),
        ),
    );
    let spill_stats = || {
        let allocator = spill_store
            .allocator
            .as_ref()
            .expect("spill allocator should exist");
        allocator
            .allocator
            .domain_pool_stats(allocator.domain)
            .expect("spill pool stats should read")
            .expect("spill pool should exist")
    };

    let mut future = InlineAsyncFutureStorage::empty();
    let result = ExecutorCell::new(true, InlineAsyncResultStorage::empty());

    future
        .store_future(&spill_store, async { [9_u8; 384] })
        .expect("future should reserve one spill envelope for its spilled output");
    let reserved_ptr = future
        .allocation
        .as_ref()
        .expect("reserved spill envelope should exist")
        .as_non_null();
    assert_eq!(spill_stats().leased_extent_count, 1);

    let waker = unsafe { Waker::from_raw(noop_async_task_raw_waker()) };
    let mut context = Context::from_waker(&waker);
    assert_eq!(
        future
            .poll_in_place(&result, &spill_store, &mut context)
            .expect("poll should succeed"),
        Poll::Ready(())
    );

    let result_ptr = result
        .with_ref(|slot| {
            slot.allocation
                .as_ref()
                .map(|allocation| allocation.as_non_null())
        })
        .expect("result storage should synchronize")
        .expect("result should retain the shared spill envelope");
    assert_eq!(result_ptr, reserved_ptr);
    assert_eq!(spill_stats().leased_extent_count, 1);

    let output = result
        .with(|slot| slot.take::<[u8; 384]>(&spill_store))
        .expect("result storage should synchronize")
        .expect("result should take cleanly");
    assert!(output.iter().all(|byte| *byte == 9));
    assert_eq!(spill_stats().leased_extent_count, 0);
}

#[test]
fn larger_results_can_exceed_default_per_task_spill_budget_when_domain_has_room() {
    let executor = Executor::new(ExecutorConfig::new());

    let handle = executor
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async move { [7_u8; 2048] })
        .expect("larger outputs should use the shared spill domain when it has room");

    let output = handle.join().expect("task should complete");
    assert_eq!(output.len(), 2048);
    assert!(output.iter().all(|byte| *byte == 7));
}

#[test]
fn results_without_one_spill_domain_are_rejected_honestly() {
    let spill_store = AsyncTaskSpillStore::new(true, None);
    let result = ExecutorCell::new(true, InlineAsyncResultStorage::empty());

    assert!(matches!(
        result
            .with(|slot| slot.store_with_allocation(&spill_store, None, [0_u8; 2048]))
            .expect("result storage should synchronize"),
        Err(ExecutorError::Unsupported)
    ));
}

#[test]
fn dropping_executor_shuts_down_live_pending_slots() {
    let executor = Executor::new(ExecutorConfig::new());
    let handle = executor
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, core::future::pending::<u8>())
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
    let _guard = crate::thread::hosted_test_guard();
    let runtime = match HostedFiberRuntime::fixed_with_stack(
        hosted_green_executor_stack_size().expect("green executor stack size should resolve"),
        2,
    ) {
        Ok(runtime) => runtime,
        Err(error) if is_unsupported_fiber_error(error) => return,
        Err(error) => panic!("hosted fiber runtime should build: {error:?}"),
    };
    let executor = match Executor::new(ExecutorConfig::green_pool()).on_hosted_fibers(&runtime) {
        Ok(executor) => executor,
        Err(error) if is_unsupported_executor_error(error) => return,
        Err(error) => panic!("executor should bind to hosted fibers: {error:?}"),
    };
    assert_eq!(executor.mode(), ExecutorMode::GreenPool);
    drop(executor);
    drop(runtime);
}

#[cfg(feature = "std")]
#[test]
fn executor_runs_on_thread_pool_carriers() {
    let _guard = crate::thread::hosted_test_guard();
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
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
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
    let handle = runtime
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, value())
        .expect("task should spawn");
    assert_eq!(runtime.run_until_idle().expect("runtime should drain"), 2);
    assert_eq!(handle.join().expect("task should complete"), 34);
}

#[test]
fn current_async_runtime_binds_current_courier_identity() {
    let runtime = CurrentAsyncRuntime::with_executor_config(
        ExecutorConfig::new().with_courier_id(CourierId::new(91)),
    );
    assert_eq!(runtime.courier_id(), Some(CourierId::new(91)));
    let handle = runtime
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
            current_async_courier_id()
                .expect("current courier id should be visible")
                .get()
        })
        .expect("task should spawn");
    assert_eq!(runtime.run_until_idle().expect("runtime should drain"), 1);
    assert_eq!(handle.join().expect("task should complete"), 91);
}

#[test]
fn current_async_runtime_queries_courier_truth() {
    const COURIER: CourierId = CourierId::new(91);
    const CONTEXT: ContextId = ContextId::new(0x440);

    let mut registry: DomainRegistry<'static, 4, 4, 4, 2, 4> =
        DomainRegistry::new(DomainDescriptor {
            id: DomainId::new(0x5056_4153),
            name: "pvas",
            kind: DomainKind::NativeSubstrate,
            caps: DomainCaps::COURIER_REGISTRY | DomainCaps::COURIER_VISIBILITY,
        });
    registry
        .register_courier(CourierDescriptor {
            id: COURIER,
            name: "httpd",
            caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS | CourierCaps::SPAWN_SUB_FIBERS,
            visibility: CourierVisibility::Scoped,
            claim_awareness: ClaimAwareness::Black,
            claim_context: Some(ClaimContextId::new(0xBBB0)),
            plan: CourierPlan::new(0, 2).with_async_capacity(2),
        })
        .expect("courier should register");

    let runtime = CurrentAsyncRuntime::with_executor_config(
        ExecutorConfig::new()
            .with_courier_id(COURIER)
            .with_context_id(CONTEXT)
            .with_runtime_sink(registry.runtime_sink()),
    );
    let handle = runtime
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
            let ledger = current_async_courier_runtime_ledger()
                .expect("courier runtime ledger should be visible");
            let responsiveness = current_async_courier_responsiveness()
                .expect("courier responsiveness should be visible");
            (ledger.current_context.unwrap().context, responsiveness)
        })
        .expect("task should spawn");

    assert_eq!(runtime.run_until_idle().expect("runtime should drain"), 1);
    assert_eq!(
        handle.join().expect("task should complete"),
        (CONTEXT, CourierResponsiveness::Responsive)
    );
}

#[test]
fn current_async_runtime_updates_courier_owned_metadata_and_obligations() {
    const COURIER: CourierId = CourierId::new(92);
    const CONTEXT: ContextId = ContextId::new(0x441);

    let mut registry: DomainRegistry<'static, 4, 4, 4, 2, 8> =
        DomainRegistry::new(DomainDescriptor {
            id: DomainId::new(0x5056_4153),
            name: "pvas",
            kind: DomainKind::NativeSubstrate,
            caps: DomainCaps::COURIER_REGISTRY
                | DomainCaps::COURIER_VISIBILITY
                | DomainCaps::CONTEXT_REGISTRY,
        });
    registry
        .register_courier(CourierDescriptor {
            id: COURIER,
            name: "httpd",
            caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS | CourierCaps::SPAWN_SUB_FIBERS,
            visibility: CourierVisibility::Scoped,
            claim_awareness: ClaimAwareness::Black,
            claim_context: Some(ClaimContextId::new(0xBBB1)),
            plan: CourierPlan::new(0, 2)
                .with_async_capacity(2)
                .with_app_metadata_capacity(4)
                .with_obligation_capacity(4),
        })
        .expect("courier should register");
    registry
        .register_context(
            COURIER,
            ContextDescriptor {
                id: CONTEXT,
                name: "httpd.main",
                kind: ContextKind::FiberMetadata,
                caps: ContextCaps::PROJECTABLE | ContextCaps::CONTROL_ENDPOINT,
                claim_context: Some(ClaimContextId::new(0xBBB1)),
            },
        )
        .expect("context should register");

    let runtime = CurrentAsyncRuntime::with_executor_config(
        ExecutorConfig::new()
            .with_courier_id(COURIER)
            .with_context_id(CONTEXT)
            .with_runtime_sink(registry.runtime_sink()),
    );
    let handle = runtime
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
            update_current_async_courier_metadata("executor", "hot")
                .expect("async-lane metadata update should succeed");
            update_current_async_context_metadata("phase", "warm")
                .expect("context metadata update should succeed");
            let obligation = register_current_async_courier_obligation(
                fusion_sys::courier::CourierObligationSpec::new(
                    fusion_sys::courier::CourierMetadataSubject::AsyncLane,
                    fusion_sys::courier::CourierObligationBinding::Input(
                        "hw.keyboard@kernel-local[pvas.me]",
                    ),
                    10_000_000_000,
                    20_000_000_000,
                ),
            )
            .expect("obligation registration should succeed");
            record_current_async_courier_obligation_progress(obligation)
                .expect("obligation progress should succeed");
            obligation.get()
        })
        .expect("task should spawn");

    assert_eq!(runtime.run_until_idle().expect("runtime should drain"), 1);
    assert_eq!(handle.join().expect("task should complete"), 1);

    let courier = registry.courier(COURIER).expect("courier should exist");
    let async_metadata = courier
        .async_metadata_entry("executor")
        .expect("async metadata should exist");
    assert_eq!(async_metadata.value, "hot");
    let context_metadata = courier
        .context_metadata_entry(CONTEXT, "phase")
        .expect("context metadata should exist");
    assert_eq!(context_metadata.value, "warm");
    let obligation = courier
        .obligations()
        .next()
        .expect("courier obligation should exist");
    assert_eq!(
        obligation.binding,
        fusion_sys::courier::CourierObligationBinding::Input("hw.keyboard@kernel-local[pvas.me]")
    );
    assert_eq!(obligation.responsiveness, CourierResponsiveness::Responsive);
}

#[test]
fn current_async_runtime_preserves_zero_identity_in_tls_context() {
    let runtime = CurrentAsyncRuntime::with_executor_config(
        ExecutorConfig::new()
            .with_courier_id(CourierId::new(0))
            .with_context_id(ContextId::new(0)),
    );
    let handle = runtime
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
            (
                current_async_courier_id()
                    .expect("current courier id should be visible")
                    .get(),
                current_async_context_id()
                    .expect("current context id should be visible")
                    .get(),
            )
        })
        .expect("task should spawn");
    assert_eq!(runtime.run_until_idle().expect("runtime should drain"), 1);
    assert_eq!(handle.join().expect("task should complete"), (0, 0));
}

#[test]
fn task_handle_is_awaitable_on_current_runtime() {
    let runtime = CurrentAsyncRuntime::new();
    let handle = runtime
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
            async_yield_now().await;
            13_u8
        })
        .expect("task should spawn");
    let result = runtime
        .block_on_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, handle)
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
        .spawn_local_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, {
            let local = Rc::clone(&local);
            async move {
                async_yield_now().await;
                *local + 2
            }
        })
        .expect("local task should spawn");
    let result = runtime
        .block_on_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, handle)
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
        .block_on_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, handle)
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
        .block_on_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, handle)
        .expect("runtime should drive generated local task");
    assert_eq!(result.expect("local task should complete"), 55);
}

#[cfg(feature = "debug-insights")]
#[test]
fn current_runtime_task_lifecycle_insight_reports_spawn_poll_and_complete() {
    use fusion_sys::transport::TransportAttachmentRequest;

    let runtime = CurrentAsyncRuntime::new();
    let insight = runtime.task_lifecycle_insight();
    let consumer = insight
        .attach_consumer(TransportAttachmentRequest::same_courier())
        .expect("task lifecycle consumer should attach");

    let handle = runtime
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
            async_yield_now().await;
            7_u8
        })
        .expect("task should spawn");
    let task = handle.id();

    assert_eq!(
        runtime
            .block_on_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, handle)
            .expect("runtime should drive task")
            .expect("task should complete"),
        7
    );

    let mut records = Vec::new();
    while let Some(record) = insight
        .try_receive(consumer)
        .expect("task lifecycle receive should succeed")
    {
        records.push(record);
    }

    assert!(matches!(
        records.first(),
        Some(AsyncTaskLifecycleRecord::Spawned {
            task: first_task,
            scheduler: AsyncTaskSchedulerTag::Current,
            ..
        }) if *first_task == task
    ));
    assert!(records.iter().any(|record| {
        matches!(
            record,
            AsyncTaskLifecycleRecord::PolledPending {
                task: event_task,
                scheduler: AsyncTaskSchedulerTag::Current,
                ..
            } if *event_task == task
        )
    }));
    assert!(records.iter().any(|record| {
        matches!(
            record,
            AsyncTaskLifecycleRecord::PolledReady {
                task: event_task,
                scheduler: AsyncTaskSchedulerTag::Current,
                ..
            } if *event_task == task
        )
    }));
    assert!(records.iter().any(|record| {
        matches!(
            record,
            AsyncTaskLifecycleRecord::Completed {
                task: event_task,
                scheduler: AsyncTaskSchedulerTag::Current,
                ..
            } if *event_task == task
        )
    }));
}

#[test]
fn task_handle_abort_reports_cancelled() {
    let runtime = CurrentAsyncRuntime::new();
    let handle = runtime
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
            async_yield_now().await;
            21_u8
        })
        .expect("task should spawn");
    handle.abort().expect("task should abort cleanly");
    let result = runtime
        .block_on_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, handle)
        .expect("runtime should drive cancelled task join");
    assert!(matches!(result, Err(ExecutorError::Cancelled)));
}

#[cfg(feature = "std")]
#[test]
fn current_runtime_waits_for_readiness() {
    let runtime = CurrentAsyncRuntime::new();
    let pipe = Arc::new(TestPipe::new());
    let handle = runtime
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, {
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
            .block_on_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, handle)
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
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
            async_sleep_for(Duration::from_millis(1))
                .await
                .expect("sleep should complete");
            99_u8
        })
        .expect("task should spawn");

    assert_eq!(
        runtime
            .block_on_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, handle)
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
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async move {
            async_sleep_until_instant(deadline)
                .await
                .expect("sleep-until should complete");
            41_u8
        })
        .expect("task should spawn");

    assert_eq!(
        runtime
            .block_on_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, handle)
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
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
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
    let _guard = crate::thread::hosted_test_guard();
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
    let handle = runtime
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, value())
        .expect("task should spawn");
    assert_eq!(handle.join().expect("task should complete"), 55);
}

#[cfg(feature = "std")]
#[test]
fn thread_async_runtime_defaults_to_direct_hosted_workers() {
    let _guard = crate::thread::hosted_test_guard();
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
    let _guard = crate::thread::hosted_test_guard();
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

#[cfg(all(feature = "std", feature = "debug-insights"))]
#[test]
fn thread_async_runtime_task_lifecycle_insight_reports_thread_workers_scheduler() {
    let _guard = crate::thread::hosted_test_guard();
    use fusion_sys::transport::TransportAttachmentRequest;

    let runtime = ThreadAsyncRuntime::new(&ThreadPoolConfig {
        min_threads: 1,
        max_threads: 1,
        ..ThreadPoolConfig::new()
    })
    .expect("thread async runtime should build");
    let insight = runtime.task_lifecycle_insight();
    let consumer = insight
        .attach_consumer(TransportAttachmentRequest::same_courier())
        .expect("task lifecycle consumer should attach");

    let handle = runtime
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
            async_yield_now().await;
            29_u8
        })
        .expect("task should spawn");
    let task = handle.id();
    assert_eq!(handle.join().expect("task should complete"), 29);

    let mut records = Vec::new();
    while let Some(record) = insight
        .try_receive(consumer)
        .expect("task lifecycle receive should succeed")
    {
        records.push(record);
    }

    assert!(matches!(
        records.first(),
        Some(AsyncTaskLifecycleRecord::Spawned {
            task: first_task,
            scheduler: AsyncTaskSchedulerTag::ThreadWorkers,
            ..
        }) if *first_task == task
    ));
    assert!(records.iter().any(|record| {
        matches!(
            record,
            AsyncTaskLifecycleRecord::Completed {
                task: event_task,
                scheduler: AsyncTaskSchedulerTag::ThreadWorkers,
                ..
            } if *event_task == task
        )
    }));
}

#[cfg(feature = "std")]
#[test]
fn thread_runtime_block_on_awaits_task_handles() {
    let _guard = crate::thread::hosted_test_guard();
    let runtime = ThreadAsyncRuntime::new(&ThreadPoolConfig {
        min_threads: 1,
        max_threads: 1,
        ..ThreadPoolConfig::new()
    })
    .expect("thread async runtime should build");
    let first = runtime
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
            async_yield_now().await;
            13_u8
        })
        .expect("first task should spawn");
    let second = runtime
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
            async_yield_now().await;
            21_u8
        })
        .expect("second task should spawn");

    let sum = runtime
        .block_on_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async move {
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
    let _guard = crate::thread::hosted_test_guard();
    let cpu = ThreadLogicalCpuId {
        group: ThreadProcessorGroupId(0),
        index: 0,
    };
    let runtime = match ThreadAsyncRuntime::new(&ThreadPoolConfig {
        min_threads: 1,
        max_threads: 1,
        placement: PoolPlacement::Static(core::slice::from_ref(&cpu)),
        ..ThreadPoolConfig::new()
    }) {
        Ok(runtime) => runtime,
        Err(error) if is_unsupported_executor_error(error) => return,
        Err(error) => panic!("thread async runtime should build: {error:?}"),
    };

    assert_eq!(
        runtime.bootstrap(),
        ThreadAsyncRuntimeBootstrap::ComposedThreadPool
    );
    assert!(runtime.thread_pool().is_some());
}

#[cfg(feature = "std")]
#[test]
fn current_runtime_executor_capacity_can_be_shaped_explicitly() {
    let runtime = CurrentAsyncRuntime::with_executor_config(ExecutorConfig::new().with_capacity(1));
    let _first = runtime
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
            core::future::pending::<()>().await;
        })
        .expect("first task should fit in one-slot runtime");

    assert_eq!(
        runtime
            .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async { 1_u8 })
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
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
            async_yield_now().await;
            29_u8
        })
        .expect("task should spawn");

    assert_eq!(
        runtime
            .block_on_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, handle)
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
    assert!(rounded.spill.bytes >= exact.spill.bytes);
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
            .spill
            .as_ref()
            .expect("async spill backing should exist")
            .view()
            .len()
            >= exact
                .spill
                .as_ref()
                .expect("async spill backing should exist")
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
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
            async_yield_now().await;
            31_u8
        })
        .expect("task should spawn");

    assert_eq!(
        runtime
            .block_on_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, handle)
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
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
            async_yield_now().await;
            37_u8
        })
        .expect("task should spawn");

    assert_eq!(
        runtime
            .block_on_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, handle)
            .expect("runtime should drive exact-aligned bound-slab task")
            .expect("task should complete"),
        37
    );
}

#[cfg(feature = "std")]
#[test]
fn thread_async_runtime_executor_capacity_can_be_shaped_explicitly() {
    let _guard = crate::thread::hosted_test_guard();
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
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
            core::future::pending::<()>().await;
        })
        .expect("first task should fit in one-slot runtime");

    assert_eq!(
        runtime
            .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async { 2_u8 })
            .expect_err("second task should exhaust one-slot runtime"),
        executor_busy()
    );
}

#[cfg(feature = "std")]
#[test]
fn thread_async_runtime_repeated_create_drop_stays_alive() {
    let _guard = crate::thread::hosted_test_guard();
    for _ in 0..64 {
        let runtime = ThreadAsyncRuntime::new(&ThreadPoolConfig {
            min_threads: 1,
            max_threads: 1,
            ..ThreadPoolConfig::new()
        })
        .expect("thread async runtime should build");
        let handle = runtime
            .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
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
    let _guard = crate::thread::hosted_test_guard();
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
            let handle = match runtime.spawn_with_poll_stack_bytes(
                TEST_ASYNC_POLL_STACK_BYTES,
                async {
                    async_yield_now().await;
                },
            ) {
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
                    let states: Vec<u8> = registry.slots.iter().map(|slot| slot.state()).collect();
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
    let _guard = crate::thread::hosted_test_guard();
    let runtime = match ThreadAsyncRuntime::new(&ThreadPoolConfig {
        min_threads: 1,
        max_threads: 1,
        ..ThreadPoolConfig::new()
    }) {
        Ok(runtime) => runtime,
        Err(error) if is_unsupported_executor_error(error) => return,
        Err(error) => panic!("thread async runtime should build: {error:?}"),
    };
    let pipe = Arc::new(TestPipe::new());
    let handle = runtime
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, {
            let pipe = Arc::clone(&pipe);
            async move {
                let readiness = async_wait_for_readiness(
                    pipe.source(),
                    EventInterest::READABLE | EventInterest::ERROR | EventInterest::HANGUP,
                )
                .await?;
                assert!(readiness.contains(EventReadiness::READABLE));
                Ok::<u8, ExecutorError>(pipe.read_byte())
            }
        })
        .expect("task should spawn");

    thread::sleep(Duration::from_millis(1));
    pipe.write_byte(12);
    match handle.join() {
        Ok(Ok(value)) => assert_eq!(value, 12),
        Ok(Err(error)) if is_unsupported_executor_error(error) => {}
        Err(error) if is_unsupported_executor_error(error) => {}
        other => panic!("task should complete or report unsupported: {other:?}"),
    }
}

#[cfg(feature = "std")]
#[test]
fn thread_async_runtime_sleep_for_completes() {
    let _guard = crate::thread::hosted_test_guard();
    let runtime = match ThreadAsyncRuntime::new(&ThreadPoolConfig {
        min_threads: 1,
        max_threads: 1,
        ..ThreadPoolConfig::new()
    }) {
        Ok(runtime) => runtime,
        Err(error) if is_unsupported_executor_error(error) => return,
        Err(error) => panic!("thread async runtime should build: {error:?}"),
    };
    let handle = runtime
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
            async_sleep_for(Duration::from_millis(1)).await?;
            Ok::<u8, ExecutorError>(13_u8)
        })
        .expect("task should spawn");
    match handle.join() {
        Ok(Ok(value)) => assert_eq!(value, 13),
        Ok(Err(error)) if is_unsupported_executor_error(error) => {}
        Err(error) if is_unsupported_executor_error(error) => {}
        other => panic!("task should complete or report unsupported: {other:?}"),
    }
}

#[cfg(feature = "std")]
#[test]
fn fiber_async_runtime_binds_owned_hosted_fibers() {
    let _guard = crate::thread::hosted_test_guard();
    let hosted = match HostedFiberRuntime::fixed_with_stack(
        hosted_green_executor_stack_size().expect("green executor stack size should resolve"),
        2,
    ) {
        Ok(hosted) => hosted,
        Err(error) if is_unsupported_fiber_error(error) => return,
        Err(error) => panic!("hosted fiber runtime should build: {error:?}"),
    };
    let runtime = match FiberAsyncRuntime::from_hosted_fibers(hosted) {
        Ok(runtime) => runtime,
        Err(error) if is_unsupported_executor_error(error) => return,
        Err(error) => panic!("fiber async runtime should bind: {error:?}"),
    };
    assert_eq!(runtime.executor().mode(), ExecutorMode::GreenPool);
    drop(runtime);
}

#[cfg(feature = "std")]
#[test]
fn fiber_async_runtime_rejects_undersized_hosted_fibers() {
    let _guard = crate::thread::hosted_test_guard();
    let hosted = match HostedFiberRuntime::fixed(2) {
        Ok(hosted) => hosted,
        Err(error) if is_unsupported_fiber_error(error) => return,
        Err(error) => panic!("hosted fiber runtime should build: {error:?}"),
    };
    assert!(matches!(
        FiberAsyncRuntime::from_hosted_fibers(hosted),
        Err(ExecutorError::Unsupported)
    ));
}

#[cfg(feature = "std")]
#[test]
fn fiber_async_runtime_spawn_generated_preserves_contract() {
    let _guard = crate::thread::hosted_test_guard();
    let runtime = match FiberAsyncRuntime::fixed(2) {
        Ok(runtime) => runtime,
        Err(error) if is_unsupported_executor_error(error) => return,
        Err(error) => panic!("fiber async runtime should build: {error:?}"),
    };
    let handle = runtime
        .spawn_generated(ExplicitGeneratedPollStackFuture)
        .expect("generated-contract task should spawn");
    assert_eq!(
        handle.admission().poll_stack,
        AsyncPollStackContract::Explicit { bytes: 1792 }
    );
    assert_eq!(handle.join().expect("task should complete"), 55);
}

#[cfg(all(feature = "std", feature = "debug-insights"))]
#[test]
fn fiber_async_runtime_task_lifecycle_insight_reports_green_pool_scheduler() {
    let _guard = crate::thread::hosted_test_guard();
    use fusion_sys::transport::TransportAttachmentRequest;

    let runtime = FiberAsyncRuntime::fixed(2).expect("fiber async runtime should build");
    let insight = runtime.task_lifecycle_insight();
    let consumer = insight
        .attach_consumer(TransportAttachmentRequest::same_courier())
        .expect("task lifecycle consumer should attach");

    let handle = runtime
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
            async_yield_now().await;
            31_u8
        })
        .expect("task should spawn");
    let task = handle.id();
    assert_eq!(handle.join().expect("task should complete"), 31);

    let mut records = Vec::new();
    while let Some(record) = insight
        .try_receive(consumer)
        .expect("task lifecycle receive should succeed")
    {
        records.push(record);
    }

    assert!(matches!(
        records.first(),
        Some(AsyncTaskLifecycleRecord::Spawned {
            task: first_task,
            scheduler: AsyncTaskSchedulerTag::GreenPool,
            ..
        }) if *first_task == task
    ));
    assert!(records.iter().any(|record| {
        matches!(
            record,
            AsyncTaskLifecycleRecord::Completed {
                task: event_task,
                scheduler: AsyncTaskSchedulerTag::GreenPool,
                ..
            } if *event_task == task
        )
    }));
}

#[cfg(feature = "std")]
#[test]
fn fiber_async_runtime_repeated_create_drop_stays_alive() {
    let _guard = crate::thread::hosted_test_guard();
    for _ in 0..32 {
        let runtime = match FiberAsyncRuntime::fixed(2) {
            Ok(runtime) => runtime,
            Err(error) if is_unsupported_executor_error(error) => return,
            Err(error) => panic!("fiber async runtime should build: {error:?}"),
        };
        let handle = runtime
            .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
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
    let _guard = crate::thread::hosted_test_guard();
    let runtime = match FiberAsyncRuntime::fixed(2) {
        Ok(runtime) => runtime,
        Err(error) if is_unsupported_executor_error(error) => return,
        Err(error) => panic!("fiber async runtime should build: {error:?}"),
    };
    let handle = runtime
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, async {
            async_sleep_for(Duration::from_millis(1)).await
        })
        .expect("task should spawn");
    assert!(matches!(handle.join(), Ok(Err(ExecutorError::Unsupported))));
}

#[cfg(feature = "std")]
#[test]
fn fiber_async_runtime_waits_for_readiness() {
    let _guard = crate::thread::hosted_test_guard();
    let runtime = match FiberAsyncRuntime::fixed(2) {
        Ok(runtime) => runtime,
        Err(error) if is_unsupported_executor_error(error) => return,
        Err(error) => panic!("fiber async runtime should build: {error:?}"),
    };
    let pipe = Arc::new(TestPipe::new());
    let handle = runtime
        .spawn_with_poll_stack_bytes(TEST_ASYNC_POLL_STACK_BYTES, {
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
