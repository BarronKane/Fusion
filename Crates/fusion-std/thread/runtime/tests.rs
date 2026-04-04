use std::alloc::{
    Layout,
    alloc_zeroed,
};

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
    BoundResourceSpec,
    MemoryDomain,
    MemoryGeometry,
    OvercommitPolicy,
    ResourceAttrs,
    ResourceContract,
    ResourceOpSet,
    ResourceResidencySupport,
    ResourceState,
    ResourceSupport,
    SharingPolicy,
    StateValue,
};

use crate::thread::async_yield_now;
use crate::thread::fiber::{
    current_courier_responsiveness,
    current_courier_runtime_ledger,
    current_fiber_record,
};
use crate::thread::yield_now;
use super::*;

fn aligned_bound_resource(len: usize, align: usize) -> MemoryResourceHandle {
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
fn combined_current_runtime_backing_plan_rounds_up_when_requested() {
    let _guard = crate::thread::runtime_test_guard();
    let exact = CurrentFiberAsyncBootstrap::uniform(
        1,
        NonZeroUsize::new(8 * 1024).expect("non-zero stack"),
        1,
    )
    .with_guard_pages(0)
    .with_sizing_strategy(RuntimeSizingStrategy::Exact)
    .backing_plan()
    .expect("exact plan should build");
    let rounded = CurrentFiberAsyncBootstrap::uniform(
        1,
        NonZeroUsize::new(8 * 1024).expect("non-zero stack"),
        1,
    )
    .with_guard_pages(0)
    .with_sizing_strategy(RuntimeSizingStrategy::GlobalNearestRoundUp)
    .backing_plan()
    .expect("rounded plan should build");

    assert!(rounded.slab.bytes >= exact.slab.bytes);
    assert!(rounded.slab.align >= exact.slab.align);
}

#[test]
fn combined_current_runtime_exact_aligned_plan_reduces_padding() {
    let _guard = crate::thread::runtime_test_guard();
    let bootstrap = CurrentFiberAsyncBootstrap::uniform(
        1,
        NonZeroUsize::new(8 * 1024).expect("non-zero stack"),
        2,
    )
    .with_guard_pages(0);
    let conservative = bootstrap
        .backing_plan()
        .expect("conservative plan should build");
    let exact = bootstrap
        .backing_plan_for_base_alignment(conservative.slab.align)
        .expect("exact-aligned plan should build");

    assert!(exact.slab.bytes <= conservative.slab.bytes);
    assert_eq!(exact.slab.align, conservative.slab.align);
}

#[test]
fn combined_current_runtime_target_planning_support_can_shrink_fiber_backing() {
    let _guard = crate::thread::runtime_test_guard();
    let bootstrap = CurrentFiberAsyncBootstrap::uniform(
        1,
        NonZeroUsize::new((12 * 1024) + 8).expect("non-zero stack"),
        1,
    )
    .with_guard_pages(0)
    .with_sizing_strategy(RuntimeSizingStrategy::Exact);
    let hosted_like = bootstrap
        .backing_plan_with_fiber_planning_support_and_allocator_layout_policy(
            FiberPlanningSupport::same_carrier(
                352,
                16,
                128,
                fusion_sys::fiber::ContextStackDirection::Down,
                false,
            ),
            AllocatorLayoutPolicy::exact_static(),
        )
        .expect("hosted-like plan should build");
    let cortex_m = bootstrap
        .backing_plan_with_fiber_planning_support_and_allocator_layout_policy(
            FiberPlanningSupport::same_carrier(
                0,
                8,
                0,
                fusion_sys::fiber::ContextStackDirection::Down,
                false,
            ),
            AllocatorLayoutPolicy::exact_static(),
        )
        .expect("cortex-m plan should build");

    assert!(cortex_m.fiber_plan.stacks.len < hosted_like.fiber_plan.stacks.len);
    assert!(cortex_m.slab.bytes <= hosted_like.slab.bytes);
}

#[test]
fn current_runtime_from_bound_slab_parts_build_both_runtimes() {
    let _guard = crate::thread::runtime_test_guard();
    let bootstrap = CurrentFiberAsyncBootstrap::uniform(
        1,
        NonZeroUsize::new(8 * 1024).expect("non-zero stack"),
        2,
    )
    .with_guard_pages(0);
    let layout = bootstrap.backing_plan().expect("backing plan should build");
    let slab = aligned_bound_resource(layout.slab.bytes, layout.slab.align);
    let runtime = bootstrap
        .from_bound_slab_parts(slab)
        .expect("combined runtime should build from one bound slab");
    let (fibers, executor) = runtime.into_parts();

    let handle = fibers
        .spawn_with_stack::<4096, _, _>(|| 7_u8)
        .expect("fiber should spawn");
    assert_eq!(handle.join().expect("fiber join should complete"), 7);
    let executor = executor
        .build_explicit()
        .expect("executor should build from split backing");
    let task = executor
        .spawn_with_poll_stack_bytes(2048, async {
            async_yield_now().await;
            41_u8
        })
        .expect("async task should spawn");

    assert_eq!(
        executor
            .block_on_with_poll_stack_bytes(2048, task)
            .expect("runtime should drive async task")
            .expect("async task should complete"),
        41
    );

    fibers
        .shutdown()
        .expect("combined current runtime should shut down fibers");
}

#[test]
fn current_runtime_from_exact_aligned_bound_slab_parts_builds() {
    let _guard = crate::thread::runtime_test_guard();
    let bootstrap = CurrentFiberAsyncBootstrap::uniform(
        1,
        NonZeroUsize::new(8 * 1024).expect("non-zero stack"),
        2,
    )
    .with_guard_pages(0);
    let conservative = bootstrap.backing_plan().expect("plan should build");
    let exact = bootstrap
        .backing_plan_for_base_alignment(conservative.slab.align)
        .expect("exact-aligned plan should build");
    let slab = aligned_bound_resource(exact.slab.bytes, exact.slab.align);
    let runtime = bootstrap
        .from_bound_slab_parts(slab)
        .expect("combined runtime should build from exact-aligned slab");
    let (fibers, executor) = runtime.into_parts();

    let handle = fibers
        .spawn_with_stack::<4096, _, _>(|| 9_u8)
        .expect("fiber should spawn");
    assert_eq!(handle.join().expect("fiber join should complete"), 9);
    let executor = executor
        .build_explicit()
        .expect("executor should build from exact-aligned split backing");
    let task = executor
        .spawn_with_poll_stack_bytes(2048, async { 43_u8 })
        .expect("async task should spawn");
    assert_eq!(
        executor
            .block_on_with_poll_stack_bytes(2048, task)
            .expect("runtime should drive task")
            .expect("task should complete"),
        43
    );

    fibers
        .shutdown()
        .expect("combined current runtime should shut down fibers");
}

#[test]
fn current_runtime_reports_configured_memory_footprint() {
    let _guard = crate::thread::runtime_test_guard();
    let bootstrap = CurrentFiberAsyncBootstrap::uniform(
        1,
        NonZeroUsize::new(8 * 1024).expect("non-zero stack"),
        2,
    )
    .with_guard_pages(0);
    let layout = bootstrap.backing_plan().expect("backing plan should build");
    let slab = aligned_bound_resource(layout.slab.bytes, layout.slab.align);
    let runtime = bootstrap
        .from_bound_slab(slab)
        .expect("combined runtime should build from one bound slab");
    let footprint = runtime
        .configured_memory_footprint()
        .expect("configured runtime footprint should build");

    assert!(footprint.fibers.total_bytes() > 0);
    assert!(footprint.executor.total_bytes() > 0);
    assert_eq!(
        footprint.total_bytes(),
        footprint.fibers.total_bytes() + footprint.executor.total_bytes()
    );

    runtime
        .fibers()
        .shutdown()
        .expect("combined current runtime should shut down fibers");
}

#[test]
fn current_runtime_singleton_grows_fiber_capacity_quiescently() {
    let _guard = crate::thread::runtime_test_guard();
    static RUNTIME: CurrentFiberAsyncSingleton = CurrentFiberAsyncSingleton::new();

    let initial_capacity = RUNTIME
        .fiber_runtime_borrow(None, None)
        .expect("singleton fiber runtime should initialize")
        .configured_capacity();
    assert_eq!(initial_capacity, 1);

    let first = RUNTIME
        .spawn_fiber_with_stack::<4096, _, _>(|| 11_u8)
        .expect("first fiber should spawn");
    while !first.is_finished().expect("fiber completion should read") {
        assert!(RUNTIME.drive_once().expect("fiber drive should succeed"));
    }

    let second = RUNTIME
        .spawn_fiber_with_stack::<4096, _, _>(|| 13_u8)
        .expect("second fiber should trigger quiescent growth");
    let grown_capacity = RUNTIME
        .fiber_runtime_borrow(None, None)
        .expect("singleton fiber runtime should remain borrowable")
        .configured_capacity();
    assert!(
        grown_capacity > initial_capacity,
        "fiber capacity should grow after quiescent slot exhaustion"
    );

    while !second.is_finished().expect("fiber completion should read") {
        assert!(RUNTIME.drive_once().expect("fiber drive should succeed"));
    }

    assert_eq!(first.join().expect("first fiber should join"), 11);
    assert_eq!(second.join().expect("second fiber should join"), 13);
    RUNTIME
        .shutdown_fibers()
        .expect("singleton fibers should shut down cleanly");
}

#[test]
fn current_runtime_singleton_grows_async_capacity_quiescently() {
    let _guard = crate::thread::runtime_test_guard();
    static RUNTIME: CurrentFiberAsyncSingleton = CurrentFiberAsyncSingleton::new();

    let initial_capacity = RUNTIME
        .async_total_capacity()
        .expect("singleton async capacity should read");
    assert_eq!(initial_capacity, 0);

    let first = RUNTIME
        .spawn_async_with_poll_stack_bytes(2048, async { 21_u8 })
        .expect("first async task should spawn");
    let after_first_capacity = RUNTIME
        .async_total_capacity()
        .expect("singleton async capacity should read");
    assert_eq!(after_first_capacity, 1);
    assert_eq!(
        RUNTIME
            .run_async_until_idle()
            .expect("singleton async runtime should run to idle"),
        1
    );
    assert!(first.is_finished().expect("first task state should read"));

    let second = RUNTIME
        .spawn_async_with_poll_stack_bytes(2048, async { 34_u8 })
        .expect("second async task should trigger segmented growth");
    let grown_capacity = RUNTIME
        .async_total_capacity()
        .expect("singleton async capacity should read");
    assert!(
        grown_capacity > after_first_capacity,
        "async capacity should grow by appending another segment after slot exhaustion"
    );

    assert_eq!(
        RUNTIME
            .run_async_until_idle()
            .expect("singleton async runtime should run to idle"),
        1
    );

    assert_eq!(first.join().expect("first task should join"), 21);
    assert_eq!(second.join().expect("second task should join"), 34);
}

#[test]
fn current_runtime_singleton_runtime_summary_combines_fiber_and_async_lanes() {
    let _guard = crate::thread::runtime_test_guard();
    static RUNTIME: CurrentFiberAsyncSingleton = CurrentFiberAsyncSingleton::new();

    let idle = RUNTIME
        .runtime_summary()
        .expect("summary should observe empty singleton");
    assert_eq!(idle.total_active_units(), 0);
    assert_eq!(idle.run_state, CourierRunState::Idle);

    let fiber = RUNTIME
        .spawn_fiber_with_stack::<4096, _, _>(|| {
            yield_now().expect("fiber should yield cleanly");
            5_u8
        })
        .expect("fiber should spawn");
    let task = RUNTIME
        .spawn_async_with_poll_stack_bytes(2048, async {
            async_yield_now().await;
            8_u8
        })
        .expect("async task should spawn");

    let active = RUNTIME
        .runtime_summary()
        .expect("summary should observe both runtime lanes");
    assert!(active.fiber_lane.is_some());
    assert!(active.async_lane.is_some());
    assert!(active.total_active_units() >= 2);
    assert!(
        matches!(
            active.run_state,
            CourierRunState::Runnable | CourierRunState::Running
        ),
        "spawned fiber and async task should make the singleton runnable"
    );

    while !fiber.is_finished().expect("fiber completion should read") {
        assert!(RUNTIME.drive_once().expect("fiber drive should succeed"));
    }
    assert_eq!(fiber.join().expect("fiber should join"), 5);
    assert_eq!(
        RUNTIME
            .block_on_with_poll_stack_bytes(2048, task)
            .expect("singleton should drive async task")
            .expect("task should complete"),
        8
    );
    let drained = RUNTIME
        .runtime_summary()
        .expect("summary should observe drained singleton");
    assert_eq!(drained.total_active_units(), 0);
    assert_eq!(drained.run_state, CourierRunState::Idle);
}

#[test]
fn combined_current_runtime_realizes_child_launch_against_domain_registry() {
    let _guard = crate::thread::runtime_test_guard();
    const ROOT_COURIER: CourierId = CourierId::new(1);
    const CHILD_COURIER: CourierId = CourierId::new(177);
    const CHILD_CONTEXT: ContextId = ContextId::new(0x620);

    let mut registry: DomainRegistry<'static, 4, 4, 4, 2, 4> =
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
            id: ROOT_COURIER,
            name: "kernel",
            caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS | CourierCaps::SPAWN_SUB_FIBERS,
            visibility: CourierVisibility::Full,
            claim_awareness: ClaimAwareness::Black,
            claim_context: Some(ClaimContextId::new(0xAAA1)),
            plan: CourierPlan::new(2, 4),
        })
        .expect("root courier should register");

    let runtime = CurrentFiberAsyncBootstrap::uniform(
        2,
        NonZeroUsize::new(8 * 1024).expect("non-zero stack"),
        1,
    )
    .with_guard_pages(0)
    .with_courier_id(CHILD_COURIER)
    .with_context_id(CHILD_CONTEXT)
    .with_runtime_sink(registry.runtime_sink())
    .with_launch_control(registry.launch_control())
    .with_child_launch(CourierChildLaunchRequest {
        parent: ROOT_COURIER,
        descriptor: CourierLaunchDescriptor {
            id: CHILD_COURIER,
            name: "httpd",
            caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS | CourierCaps::SPAWN_SUB_FIBERS,
            visibility: CourierVisibility::Scoped,
            claim_awareness: ClaimAwareness::Black,
            claim_context: Some(ClaimContextId::new(0xBBB3)),
            plan: CourierPlan::new(0, 2),
        },
        principal: PrincipalId::parse("httpd#01@web[cache.pvas-local]:443")
            .expect("principal should parse"),
        image_seal: fusion_sys::claims::LocalAdmissionSeal::new(
            ImageSealId::new(8),
            ClaimsDigest::zero(),
            ClaimsDigest::zero(),
            ClaimsDigest::zero(),
            48,
        ),
        launch_epoch: 48,
    })
    .build_current()
    .expect("combined current runtime should build");

    let handle = runtime
        .fibers()
        .spawn_with_stack::<4096, _, _>(|| {
            let ledger =
                current_courier_runtime_ledger().expect("courier runtime ledger should be visible");
            let record = current_fiber_record().expect("current fiber record should be visible");
            let responsiveness =
                current_courier_responsiveness().expect("courier responsiveness should be visible");
            (
                ledger.current_context.unwrap().context,
                record.is_root,
                responsiveness,
            )
        })
        .expect("runtime fiber should spawn");

    assert_eq!(
        runtime
            .fibers()
            .run_until_idle()
            .expect("pool should drain"),
        1
    );
    assert_eq!(
        handle.join().expect("runtime fiber should complete"),
        (CHILD_CONTEXT, true, CourierResponsiveness::Responsive)
    );

    let parent = registry
        .courier(ROOT_COURIER)
        .expect("root courier should exist");
    let child = parent
        .child_couriers()
        .next()
        .expect("parent should supervise one child courier");
    assert_eq!(child.child, CHILD_COURIER);

    let launched = registry
        .courier(CHILD_COURIER)
        .expect("launched child courier should exist");
    assert_eq!(
        launched.runtime_ledger().current_context.unwrap().context,
        CHILD_CONTEXT
    );
    assert!(
        launched
            .fibers()
            .next()
            .expect("root fiber should exist")
            .is_root
    );

    runtime
        .fibers()
        .shutdown()
        .expect("combined current runtime should shut down fibers");
}

#[test]
fn current_runtime_singleton_courier_plan_surfaces_time_slice_policy() {
    let _guard = crate::thread::runtime_test_guard();
    static RUNTIME: CurrentFiberAsyncSingleton = CurrentFiberAsyncSingleton::new()
        .with_courier_plan(CourierPlan::new(0, 1).with_time_slice_ticks(47));

    let summary = RUNTIME
        .runtime_summary()
        .expect("summary should observe courier policy");
    assert_eq!(
        summary.policy,
        CourierSchedulingPolicy::TimeSliced { quantum_ticks: 47 }
    );
}

#[test]
fn current_runtime_singleton_binds_courier_identity() {
    let _guard = crate::thread::runtime_test_guard();
    static RUNTIME: CurrentFiberAsyncSingleton =
        CurrentFiberAsyncSingleton::new().with_courier_id(CourierId::new(144));

    let fiber = RUNTIME
        .spawn_fiber_with_stack::<4096, _, _>(|| {
            crate::thread::fiber::current_courier_id()
                .expect("current fiber courier id should be visible")
                .get()
        })
        .expect("fiber should spawn");
    while !fiber.is_finished().expect("fiber completion should read") {
        assert!(RUNTIME.drive_once().expect("fiber drive should succeed"));
    }
    assert_eq!(fiber.join().expect("fiber should join"), 144);

    let task = RUNTIME
        .spawn_async_with_poll_stack_bytes(2048, async {
            crate::thread::executor::current_async_courier_id()
                .expect("current async courier id should be visible")
                .get()
        })
        .expect("async task should spawn");
    assert_eq!(
        RUNTIME
            .run_async_until_idle()
            .expect("singleton async runtime should run to idle"),
        1
    );
    assert_eq!(task.join().expect("async task should complete"), 144);
}

#[test]
fn current_runtime_singleton_respects_fiber_capacity_cap() {
    let _guard = crate::thread::runtime_test_guard();
    static RUNTIME: CurrentFiberAsyncSingleton =
        CurrentFiberAsyncSingleton::new().with_fiber_capacity(1);

    let first = RUNTIME
        .spawn_fiber_with_stack::<4096, _, _>(|| 55_u8)
        .expect("first fiber should spawn");
    while !first.is_finished().expect("fiber completion should read") {
        assert!(RUNTIME.drive_once().expect("fiber drive should succeed"));
    }

    let second = RUNTIME.spawn_fiber_with_stack::<4096, _, _>(|| 89_u8);
    assert!(matches!(
        second,
        Err(error) if error.kind() == FiberErrorKind::ResourceExhausted
    ));

    assert_eq!(first.join().expect("first fiber should join"), 55);
    RUNTIME
        .shutdown_fibers()
        .expect("singleton fibers should shut down cleanly");
}

#[test]
fn current_runtime_singleton_respects_async_capacity_cap() {
    let _guard = crate::thread::runtime_test_guard();
    static RUNTIME: CurrentFiberAsyncSingleton =
        CurrentFiberAsyncSingleton::new().with_async_capacity(1);

    let first = RUNTIME
        .spawn_async_with_poll_stack_bytes(2048, async { 144_u8 })
        .expect("first async task should spawn");
    assert_eq!(
        RUNTIME
            .run_async_until_idle()
            .expect("singleton async runtime should run to idle"),
        1
    );
    assert!(first.is_finished().expect("task completion should read"));

    let second = RUNTIME.spawn_async_with_poll_stack_bytes(2048, async { 233_u8 });
    assert!(matches!(
        second,
        Err(ExecutorError::Sync(SyncErrorKind::Busy))
    ));
    assert_eq!(
        RUNTIME
            .async_total_capacity()
            .expect("singleton async capacity should read"),
        1
    );

    assert_eq!(first.join().expect("first task should join"), 144);
}

#[test]
fn current_runtime_singleton_courier_plan_respects_async_capacity_cap() {
    let _guard = crate::thread::runtime_test_guard();
    static RUNTIME: CurrentFiberAsyncSingleton = CurrentFiberAsyncSingleton::new()
        .with_courier_plan(CourierPlan::new(0, 1).with_async_capacity(1));

    let first = RUNTIME
        .spawn_async_with_poll_stack_bytes(2048, async { 34_u8 })
        .expect("first async task should spawn");
    assert_eq!(
        RUNTIME
            .run_async_until_idle()
            .expect("singleton async runtime should run to idle"),
        1
    );
    assert!(first.is_finished().expect("task completion should read"));

    let second = RUNTIME.spawn_async_with_poll_stack_bytes(2048, async { 55_u8 });
    assert!(matches!(
        second,
        Err(ExecutorError::Sync(SyncErrorKind::Busy))
    ));

    assert_eq!(first.join().expect("first task should join"), 34);
}

#[test]
fn current_runtime_singleton_courier_plan_limits_total_runnable_units() {
    let _guard = crate::thread::runtime_test_guard();
    static RUNTIME: CurrentFiberAsyncSingleton = CurrentFiberAsyncSingleton::new()
        .with_courier_plan(
            CourierPlan::new(0, 2)
                .with_async_capacity(2)
                .with_runnable_capacity(1),
        );

    let fiber = RUNTIME
        .spawn_fiber_with_stack::<4096, _, _>(|| {
            yield_now().expect("fiber should yield cleanly");
            21_u8
        })
        .expect("first fiber should spawn");

    let task = RUNTIME.spawn_async_with_poll_stack_bytes(2048, async { 89_u8 });
    assert!(matches!(
        task,
        Err(ExecutorError::Sync(SyncErrorKind::Overflow))
    ));

    while !fiber.is_finished().expect("fiber completion should read") {
        assert!(RUNTIME.drive_once().expect("fiber drive should succeed"));
    }
    assert_eq!(fiber.join().expect("fiber should join"), 21);
    RUNTIME
        .shutdown_fibers()
        .expect("singleton fibers should shut down cleanly");
}

#[test]
fn current_runtime_singleton_block_on_drives_cross_segment_task_handles() {
    let _guard = crate::thread::runtime_test_guard();
    static RUNTIME: CurrentFiberAsyncSingleton = CurrentFiberAsyncSingleton::new();

    let first = RUNTIME
        .spawn_async_with_poll_stack_bytes(2048, async { 99_u8 })
        .expect("first async task should spawn");

    let result = RUNTIME
        .block_on_with_poll_stack_bytes(2048, async move {
            first.await.expect("first task should complete")
        })
        .expect("singleton block_on should drive tasks across every async segment");

    assert_eq!(result, 99);
    assert!(
        RUNTIME
            .async_total_capacity()
            .expect("singleton async capacity should read")
            > 1
    );
}
