use super::*;

#[test]
fn automatic_carrier_selection_prefers_visible_core_count() {
    let summary = HardwareTopologySummary {
        logical_cpu_count: Some(8),
        core_count: Some(4),
        cluster_count: None,
        package_count: None,
        numa_node_count: None,
        core_class_count: None,
    };
    assert_eq!(select_automatic_carrier_count(summary), Some(4));

    let no_cores = HardwareTopologySummary {
        core_count: None,
        ..summary
    };
    assert_eq!(select_automatic_carrier_count(no_cores), Some(8));
}

#[cfg(feature = "std")]
#[test]
fn hosted_carrier_count_policy_reads_requested_topology_count() {
    let summary = HardwareTopologySummary {
        logical_cpu_count: Some(12),
        core_count: Some(6),
        cluster_count: Some(3),
        package_count: Some(2),
        numa_node_count: None,
        core_class_count: None,
    };
    assert_eq!(
        hosted_carrier_count_from_summary(summary, HostedCarrierCountPolicy::Automatic),
        Some(6)
    );
    assert_eq!(
        hosted_carrier_count_from_summary(summary, HostedCarrierCountPolicy::VisibleLogicalCpus),
        Some(12)
    );
    assert_eq!(
        hosted_carrier_count_from_summary(summary, HostedCarrierCountPolicy::VisibleCores),
        Some(6)
    );
    assert_eq!(
        hosted_carrier_count_from_summary(summary, HostedCarrierCountPolicy::VisiblePackages),
        Some(2)
    );
}

#[test]
fn per_carrier_capacity_rounds_total_budget_up() {
    assert_eq!(
        per_carrier_capacity_for_total(8, 4).expect("capacity should divide cleanly"),
        2
    );
    assert_eq!(
        per_carrier_capacity_for_total(9, 4).expect("capacity should round up"),
        3
    );
    assert_eq!(
        per_carrier_capacity_for_total(1, 8).expect("single total fiber should still admit"),
        1
    );
    assert_eq!(
        per_carrier_capacity_for_total(0, 1)
            .expect_err("zero total fibers should be rejected")
            .kind(),
        FiberError::invalid().kind()
    );
}

#[cfg(feature = "std")]
#[test]
fn hosted_class_distribution_rounds_total_slots_and_growth_chunk_up() {
    let classes = [
        HostedFiberClassConfig::new(FiberStackClass::MIN, 5)
            .expect("hosted class config should build")
            .with_growth_chunk(3)
            .expect("hosted growth chunk should build"),
        HostedFiberClassConfig::new(
            FiberStackClass::new(NonZeroUsize::new(8 * 1024).expect("non-zero class"))
                .expect("valid class"),
            9,
        )
        .expect("hosted class config should build"),
    ];

    let distributed = distribute_hosted_class_configs(&classes, 4)
        .expect("class configs should distribute across carriers");
    assert_eq!(distributed[0].class, FiberStackClass::MIN);
    assert_eq!(distributed[0].slots_per_carrier, 2);
    assert_eq!(distributed[0].growth_chunk, 1);
    assert_eq!(distributed[1].class.size_bytes().get(), 8 * 1024);
    assert_eq!(distributed[1].slots_per_carrier, 3);
    assert_eq!(distributed[1].growth_chunk, 3);
}

#[test]
fn steal_seed_randomizes_the_first_victim_choice() {
    let first = (xorshift64(initial_steal_seed(0)) % 7) + 1;
    let second = (xorshift64(initial_steal_seed(1)) % 7) + 1;
    assert_ne!(first, second);
}

#[test]
fn current_fiber_pool_join_drives_yielding_closure_to_completion() {
    let fibers =
        CurrentFiberPool::new(&FiberPoolConfig::new()).expect("current fiber pool should build");
    let stages = Arc::new(AtomicUsize::new(0));

    let task = fibers
        .spawn_with_stack::<4096, _, _>({
            let stages = Arc::clone(&stages);
            move || -> Result<u32, FiberError> {
                stages.fetch_add(1, Ordering::AcqRel);
                yield_now()?;
                stages.fetch_add(1, Ordering::AcqRel);
                Ok(42)
            }
        })
        .expect("yielding task should spawn");

    assert_eq!(
        task.join()
            .expect("current-thread join should drive the pool")
            .expect("task should complete without runtime failure"),
        42
    );
    assert_eq!(stages.load(Ordering::Acquire), 2);

    fibers
        .shutdown()
        .expect("current fiber pool should shut down cleanly");
}

#[test]
fn current_fiber_pool_from_explicit_backing_runs_task() {
    let config = FiberPoolConfig::fixed(NonZeroUsize::new(8 * 1024).expect("non-zero stack"), 2)
        .with_guard_pages(0);
    let plan = CurrentFiberPool::backing_plan(&config).expect("backing plan should build");
    let backing = CurrentFiberPoolBacking {
        control: MemoryResourceHandle::from(
            VirtualMemoryResource::create(&ResourceRequest::anonymous_private(plan.control.bytes))
                .expect("control resource should build"),
        ),
        runtime_metadata: MemoryResourceHandle::from(
            VirtualMemoryResource::create(&ResourceRequest::anonymous_private(
                plan.runtime_metadata.bytes,
            ))
            .expect("runtime metadata resource should build"),
        ),
        stack_metadata: MemoryResourceHandle::from(
            VirtualMemoryResource::create(&ResourceRequest::anonymous_private(
                plan.stack_metadata.bytes,
            ))
            .expect("stack metadata resource should build"),
        ),
        stacks: MemoryResourceHandle::from(
            VirtualMemoryResource::create(&ResourceRequest::anonymous_private(plan.stacks.bytes))
                .expect("stack resource should build"),
        ),
        slab_owner: None,
    };
    let fibers = CurrentFiberPool::from_backing(&config, backing)
        .expect("current fiber pool should build from explicit backing");

    let task = fibers
        .spawn_with_stack::<4096, _, _>(|| -> Result<u32, FiberError> {
            yield_now()?;
            Ok(11)
        })
        .expect("yielding task should spawn");

    assert_eq!(
        task.join()
            .expect("current-thread join should drive the explicit-backed pool")
            .expect("task should complete without runtime failure"),
        11
    );

    fibers
        .shutdown()
        .expect("explicit-backed current fiber pool should shut down cleanly");
}

#[test]
fn global_nearest_round_up_fiber_sizing_inflates_backing_requests() {
    let exact = FiberPoolConfig::fixed(NonZeroUsize::new(8 * 1024).expect("non-zero stack"), 2)
        .with_guard_pages(0);
    let rounded = exact.with_sizing_strategy(RuntimeSizingStrategy::GlobalNearestRoundUp);

    let exact_plan =
        CurrentFiberPool::backing_plan(&exact).expect("exact backing plan should build");
    let rounded_plan =
        CurrentFiberPool::backing_plan(&rounded).expect("rounded backing plan should build");

    assert!(rounded_plan.control.bytes >= exact_plan.control.bytes);
    assert!(rounded_plan.runtime_metadata.bytes >= exact_plan.runtime_metadata.bytes);
    assert!(rounded_plan.stack_metadata.bytes >= exact_plan.stack_metadata.bytes);
    assert!(rounded_plan.stacks.bytes >= exact_plan.stacks.bytes);
    assert!(rounded_plan.control.bytes.is_power_of_two());
    assert!(rounded_plan.runtime_metadata.bytes.is_power_of_two());
    assert!(rounded_plan.stack_metadata.bytes.is_power_of_two());
    assert!(rounded_plan.stacks.bytes.is_power_of_two());
}

#[test]
fn global_nearest_round_up_fiber_internal_mappers_use_rounded_sizes() {
    let support = GreenPool::support();
    let alignment = support.context.min_stack_alignment.max(16);
    let exact = FiberPoolConfig::fixed(NonZeroUsize::new(8 * 1024).expect("non-zero stack"), 2)
        .with_guard_pages(0);
    let rounded = exact.with_sizing_strategy(RuntimeSizingStrategy::GlobalNearestRoundUp);

    let exact_slab = FiberStackSlab::new(&exact, alignment, support.context.stack_direction)
        .expect("exact slab should build");
    let rounded_slab = FiberStackSlab::new(&rounded, alignment, support.context.stack_direction)
        .expect("rounded slab should build");

    assert!(rounded_slab.metadata_bytes >= exact_slab.metadata_bytes);
    assert!(rounded_slab.region.len >= exact_slab.region.len);

    let (exact_region, _) =
        green_pool_runtime_regions(1, 2, GreenScheduling::Fifo, false, exact.sizing)
            .expect("exact green runtime region should build");
    let (rounded_region, _) =
        green_pool_runtime_regions(1, 2, GreenScheduling::Fifo, false, rounded.sizing)
            .expect("rounded green runtime region should build");

    assert!(rounded_region.len >= exact_region.len);

    let _ = unsafe { system_mem().unmap(exact_region) };
    let _ = unsafe { system_mem().unmap(rounded_region) };
}

#[test]
fn current_fiber_pool_from_bound_slab_runs_task() {
    let config = FiberPoolConfig::fixed(NonZeroUsize::new(8 * 1024).expect("non-zero stack"), 2)
        .with_guard_pages(0);
    let layout = CurrentFiberPool::backing_plan(&config)
        .expect("backing plan should build")
        .combined()
        .expect("combined layout should build");
    let slab = aligned_bound_resource(layout.slab.bytes, layout.slab.align);
    let fibers = CurrentFiberPool::from_bound_slab(&config, slab)
        .expect("current fiber pool should build from one bound slab");

    let task = fibers
        .spawn_with_stack::<4096, _, _>(|| -> Result<u32, FiberError> {
            yield_now()?;
            Ok(17)
        })
        .expect("yielding task should spawn");

    assert_eq!(
        task.join()
            .expect("current-thread join should drive the bound-slab pool")
            .expect("task should complete without runtime failure"),
        17
    );

    fibers
        .shutdown()
        .expect("bound-slab current fiber pool should shut down cleanly");
}

#[test]
fn current_fiber_pool_from_exact_aligned_bound_slab_runs_task() {
    let config = FiberPoolConfig::fixed(NonZeroUsize::new(8 * 1024).expect("non-zero stack"), 2)
        .with_guard_pages(0);
    let conservative = CurrentFiberPool::backing_plan(&config)
        .expect("backing plan should build")
        .combined()
        .expect("conservative layout should build");
    let exact = CurrentFiberPool::backing_plan(&config)
        .expect("backing plan should build")
        .combined_for_base_alignment(conservative.slab.align)
        .expect("exact-aligned layout should build");
    let slab = aligned_bound_resource(exact.slab.bytes, exact.slab.align);
    let fibers = CurrentFiberPool::from_bound_slab(&config, slab)
        .expect("current fiber pool should build from exact-aligned slab");

    let task = fibers
        .spawn_with_stack::<4096, _, _>(|| -> Result<u32, FiberError> {
            yield_now()?;
            Ok(19)
        })
        .expect("yielding task should spawn");

    assert_eq!(
        task.join()
            .expect("current-thread join should drive the exact-aligned bound-slab pool")
            .expect("task should complete without runtime failure"),
        19
    );

    fibers
        .shutdown()
        .expect("exact-aligned bound-slab current fiber pool should shut down cleanly");
}

#[test]
fn current_fiber_pool_from_bound_slab_reuses_slots_across_many_noop_spawns() {
    let config = FiberPoolConfig::fixed(NonZeroUsize::new(8 * 1024).expect("non-zero stack"), 1)
        .with_guard_pages(0);
    let layout = CurrentFiberPool::backing_plan(&config)
        .expect("backing plan should build")
        .combined()
        .expect("combined layout should build");
    let slab = aligned_bound_resource(layout.slab.bytes, layout.slab.align);
    let fibers = CurrentFiberPool::from_bound_slab(&config, slab)
        .expect("current fiber pool should build from one bound slab");

    for _ in 0..128 {
        let handle = fibers
            .spawn_with_stack::<4096, _, _>(|| 1_u32)
            .expect("noop task should spawn repeatedly");
        assert_eq!(handle.join().expect("noop task should join repeatedly"), 1);
    }

    fibers
        .shutdown()
        .expect("bound-slab current fiber pool should shut down cleanly");
}

#[test]
fn uniform_bootstrap_uses_requested_stack_size() {
    let bootstrap = FiberPoolBootstrap::uniform(
        4,
        NonZeroUsize::new(16 * 1024).expect("non-zero uniform stack"),
    );
    assert_eq!(
        bootstrap.config().stack_backing,
        FiberStackBacking::Fixed {
            stack_size: NonZeroUsize::new(16 * 1024).expect("non-zero uniform stack"),
        }
    );
    assert_eq!(bootstrap.config().max_fibers_per_carrier, 4);
}

#[test]
fn current_fiber_pool_run_until_idle_drives_multiple_ready_segments() {
    let fibers =
        CurrentFiberPool::new(&FiberPoolConfig::new()).expect("current fiber pool should build");
    let total = Arc::new(AtomicUsize::new(0));

    let first = fibers
        .spawn_with_stack::<4096, _, _>({
            let total = Arc::clone(&total);
            move || {
                total.fetch_add(1, Ordering::AcqRel);
                yield_now().expect("first task should yield cleanly");
                total.fetch_add(10, Ordering::AcqRel);
            }
        })
        .expect("first current-thread task should spawn");
    let second = fibers
        .spawn_with_stack::<4096, _, _>({
            let total = Arc::clone(&total);
            move || {
                total.fetch_add(100, Ordering::AcqRel);
            }
        })
        .expect("second current-thread task should spawn");

    assert_eq!(
        fibers
            .run_until_idle()
            .expect("current-thread pool should drive until idle"),
        3
    );
    assert_eq!(total.load(Ordering::Acquire), 111);
    first
        .join()
        .expect("first task should already be complete after run_until_idle");
    second
        .join()
        .expect("second task should already be complete after run_until_idle");

    fibers
        .shutdown()
        .expect("current fiber pool should shut down cleanly");
}

#[test]
fn current_fiber_pool_runtime_summary_reports_active_fiber_lane_state() {
    let fibers =
        CurrentFiberPool::new(&FiberPoolConfig::new()).expect("current fiber pool should build");
    let idle = fibers
        .runtime_summary()
        .expect("summary should observe empty pool");
    assert_eq!(idle.total_active_units(), 0);
    assert_eq!(idle.run_state, CourierRunState::Idle);

    let task = fibers
        .spawn_with_stack::<4096, _, _>(|| {
            yield_now().expect("task should yield cleanly");
            7_u8
        })
        .expect("task should spawn");
    let active = fibers
        .runtime_summary()
        .expect("summary should observe spawned fiber");
    assert!(active.fiber_lane.is_some());
    assert!(active.total_active_units() >= 1);
    assert!(
        matches!(
            active.run_state,
            CourierRunState::Runnable | CourierRunState::Running
        ),
        "spawned fiber should make the lane runnable"
    );

    let _ = fibers.run_until_idle().expect("pool should drain");
    assert_eq!(task.join().expect("task should complete"), 7);
    let drained = fibers
        .runtime_summary()
        .expect("summary should observe drained pool");
    assert_eq!(drained.total_active_units(), 0);
    assert_eq!(drained.run_state, CourierRunState::Idle);

    fibers
        .shutdown()
        .expect("current fiber pool should shut down cleanly");
}

#[test]
fn current_fiber_pool_binds_current_courier_identity() {
    let fibers = CurrentFiberPool::new(&FiberPoolConfig::new().with_courier_id(CourierId::new(77)))
        .expect("current fiber pool should build");
    assert_eq!(fibers.courier_id(), Some(CourierId::new(77)));

    let task = fibers
        .spawn_with_stack::<4096, _, _>(|| {
            current_courier_id()
                .expect("current courier id should be visible")
                .get()
        })
        .expect("task should spawn");

    assert_eq!(fibers.run_until_idle().expect("pool should drain"), 1);
    assert_eq!(task.join().expect("task should complete"), 77);

    fibers
        .shutdown()
        .expect("current fiber pool should shut down cleanly");
}

#[test]
fn current_fiber_pool_publishes_courier_truth_to_runtime_sink() {
    const COURIER: CourierId = CourierId::new(77);
    const CONTEXT: ContextId = ContextId::new(0x220);

    let sink_state = StdMutex::new(TestRuntimeSinkState {
        ledger: CourierRuntimeLedger::new(),
        fiber: None,
        responsiveness: CourierResponsiveness::Responsive,
        metadata: None,
        obligation: None,
    });
    let fibers = CurrentFiberPool::new(
        &FiberPoolConfig::new()
            .with_courier_id(COURIER)
            .with_context_id(CONTEXT)
            .with_runtime_sink(test_runtime_sink(&sink_state)),
    )
    .expect("current fiber pool should build");

    let handle = fibers
        .spawn_with_stack::<4096, _, _>(|| 7_u8)
        .expect("task should spawn");

    assert_eq!(fibers.run_until_idle().expect("pool should drain"), 1);
    assert_eq!(handle.join().expect("task should complete"), 7);

    let sink_state = sink_state
        .lock()
        .expect("test runtime sink mutex should lock");
    assert_eq!(sink_state.ledger.current_context.unwrap().context, CONTEXT);
    assert_eq!(sink_state.ledger.active_runnable_units, 0);
    let record = sink_state
        .fiber
        .expect("runtime sink should retain the fiber record");
    assert_eq!(record.state, fusion_sys::fiber::FiberState::Completed);
    assert!(record.is_root);

    fibers
        .shutdown()
        .expect("current fiber pool should shut down cleanly");
}

#[test]
fn current_fiber_pool_realizes_child_launch_against_domain_registry() {
    const ROOT_COURIER: CourierId = CourierId::new(1);
    const CHILD_COURIER: CourierId = CourierId::new(77);
    const CHILD_CONTEXT: ContextId = ContextId::new(0x220);

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
            claim_context: Some(ClaimContextId::new(0xAAA0)),
            plan: CourierPlan::new(2, 4),
        })
        .expect("root courier should register");
    let runtime_sink = registry.runtime_sink();
    let launch_control = registry.launch_control();
    let fibers = CurrentFiberPool::new(
        &FiberPoolConfig::new()
            .with_courier_id(CHILD_COURIER)
            .with_context_id(CHILD_CONTEXT)
            .with_runtime_sink(runtime_sink)
            .with_launch_control(launch_control)
            .with_child_launch(CourierChildLaunchRequest {
                parent: ROOT_COURIER,
                descriptor: CourierLaunchDescriptor {
                    id: CHILD_COURIER,
                    name: "httpd",
                    caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS | CourierCaps::SPAWN_SUB_FIBERS,
                    visibility: CourierVisibility::Scoped,
                    claim_awareness: ClaimAwareness::Black,
                    claim_context: Some(ClaimContextId::new(0xBBB0)),
                    plan: CourierPlan::new(0, 2),
                },
                principal: PrincipalId::parse("httpd#01@web[cache.pvas-local]:443")
                    .expect("principal should parse"),
                image_seal: fusion_sys::claims::LocalAdmissionSeal::new(
                    ImageSealId::new(7),
                    ClaimsDigest::zero(),
                    ClaimsDigest::zero(),
                    ClaimsDigest::zero(),
                    47,
                ),
                launch_epoch: 47,
            }),
    )
    .expect("current fiber pool should build");

    let handle = fibers
        .spawn_with_stack::<4096, _, _>(|| 7_u8)
        .expect("task should spawn");

    assert_eq!(fibers.run_until_idle().expect("pool should drain"), 1);
    assert_eq!(handle.join().expect("task should complete"), 7);

    let parent = registry.courier(ROOT_COURIER).unwrap();
    let child = parent.child_couriers().next().unwrap();
    assert_eq!(child.child, CHILD_COURIER);
    let launched = registry.courier(CHILD_COURIER).unwrap();
    assert_eq!(
        launched.runtime_ledger().current_context.unwrap().context,
        CHILD_CONTEXT
    );
    assert!(launched.fibers().next().unwrap().is_root);

    fibers
        .shutdown()
        .expect("current fiber pool should shut down cleanly");
}

#[test]
fn current_fiber_pool_spawns_generated_contract_tasks() {
    let classes = [
        FiberStackClassConfig::new(SUPPORTED_GENERATED_CONTRACT_CLASS, 1)
            .expect("valid class config"),
    ];
    let fibers = CurrentFiberPool::new(
        &FiberPoolConfig::classed(&classes).expect("classed config should build"),
    )
    .expect("current fiber pool should build");

    fibers
        .spawn_generated_contract(SupportedGeneratedContractTask)
        .expect("generated-contract task should spawn")
        .join()
        .expect("generated-contract task should complete");

    fibers
        .shutdown()
        .expect("current fiber pool should shut down cleanly");
}

#[test]
fn current_fiber_pool_spawn_with_stack_admits_closure_override() {
    let classes = [FiberStackClassConfig::new(
        FiberStackClass::new(NonZeroUsize::new(4 * 1024).expect("non-zero class"))
            .expect("valid class"),
        1,
    )
    .expect("valid class config")];
    let fibers = CurrentFiberPool::new(
        &FiberPoolConfig::classed(&classes).expect("classed config should build"),
    )
    .expect("current fiber pool should build");

    assert_eq!(
        fibers
            .spawn_with_stack::<4096, _, _>(|| 7_u32)
            .expect("stack-constrained closure should spawn")
            .join()
            .expect("stack-constrained closure should complete"),
        7
    );

    let error = fibers
        .spawn_with_stack::<8192, _, _>(|| ())
        .expect_err("unsupported stack class should be rejected");
    assert_eq!(error.kind(), FiberError::unsupported().kind());

    fibers
        .shutdown()
        .expect("current fiber pool should shut down cleanly");
}

#[test]
fn current_fiber_pool_handles_report_execution_mode() {
    let fibers =
        CurrentFiberPool::new(&FiberPoolConfig::new()).expect("current fiber pool should build");

    let inline = fibers
        .spawn_explicit(SupportedInlineNoYieldTask)
        .expect("inline task should spawn");
    let inline_admission = inline
        .admission()
        .expect("inline admission should be observable");
    assert_eq!(
        inline
            .execution()
            .expect("inline execution should be observable"),
        FiberTaskExecution::InlineNoYield
    );
    assert!(
        inline
            .runs_inline()
            .expect("inline realization should be observable")
    );
    assert_eq!(
        inline_admission.execution,
        FiberTaskExecution::InlineNoYield
    );
    assert_eq!(inline_admission.priority, FiberTaskPriority::DEFAULT);
    assert_eq!(inline_admission.yield_budget, None);
    assert_eq!(inline.join().expect("inline task should complete"), 17);

    let yielding = fibers
        .spawn_with_stack::<4096, _, _>(|| -> Result<(), FiberError> {
            yield_now()?;
            Ok(())
        })
        .expect("yielding task should spawn");
    let yielding_admission = yielding
        .admission()
        .expect("fiber admission should be observable");
    assert_eq!(
        yielding
            .execution()
            .expect("fiber execution should be observable"),
        FiberTaskExecution::Fiber
    );
    assert!(
        !yielding
            .runs_inline()
            .expect("fiber realization should be observable")
    );
    assert_eq!(yielding_admission.execution, FiberTaskExecution::Fiber);
    assert_eq!(yielding_admission.priority, FiberTaskPriority::DEFAULT);
    assert_eq!(yielding_admission.yield_budget, None);
    yielding
        .join()
        .expect("yielding task should complete")
        .expect("yielding task should not fail");

    fibers
        .shutdown()
        .expect("current fiber pool should shut down cleanly");
}

#[test]
fn current_fiber_pool_runs_no_yield_tasks_inline_without_stack_admission() {
    let fibers = CurrentFiberPool::new(
        &FiberPoolConfig::fixed(NonZeroUsize::new(4 * 1024).expect("non-zero stack"), 1)
            .with_telemetry(FiberTelemetry::Full),
    )
    .expect("current fiber pool should build");

    assert_eq!(
        fibers
            .spawn_explicit(SupportedInlineNoYieldTask)
            .expect("inline no-yield task should spawn")
            .join()
            .expect("inline no-yield task should complete"),
        17
    );
    assert_eq!(
        fibers
            .stack_stats()
            .expect("telemetry should be enabled")
            .peak_used_bytes,
        0
    );

    fibers
        .shutdown()
        .expect("current fiber pool should shut down cleanly");
}

#[test]
fn fiber_pool_bootstrap_fixed_builds_current_thread_pool() {
    let fibers = FiberPoolBootstrap::fixed(2)
        .build_current()
        .expect("bootstrap should build one current-thread pool");
    assert_eq!(fibers.active_count(), 0);
    fibers
        .shutdown()
        .expect("current fiber pool should shut down cleanly");
}

#[test]
fn fixed_growing_config_commits_by_requested_chunk() {
    let config =
        FiberPoolConfig::fixed_growing(NonZeroUsize::new(4 * 1024).expect("non-zero stack"), 8, 2)
            .expect("fixed growing config should build");
    let slab = FiberStackSlab::new(
        &config,
        align_of::<usize>(),
        FiberSystem::new().support().context.stack_direction,
    )
    .expect("fixed growing slab should build");

    assert_eq!(slab.initial_slots, 2);
    assert_eq!(slab.chunk_size, 2);
    assert!(matches!(slab.growth, GreenGrowth::OnDemand));
}

#[test]
fn fixed_growing_config_rejects_invalid_chunk() {
    assert!(matches!(
        FiberPoolConfig::fixed_growing(
            NonZeroUsize::new(4 * 1024).expect("non-zero stack"),
            4,
            0,
        ),
        Err(error) if error.kind() == FiberError::invalid().kind()
    ));
    assert!(matches!(
        FiberPoolConfig::fixed_growing(
            NonZeroUsize::new(4 * 1024).expect("non-zero stack"),
            4,
            5,
        ),
        Err(error) if error.kind() == FiberError::invalid().kind()
    ));
}

#[test]
fn current_fiber_pool_fixed_growing_runs_tasks() {
    let fibers = CurrentFiberPool::new(
        &FiberPoolConfig::fixed_growing(NonZeroUsize::new(4 * 1024).expect("non-zero stack"), 4, 1)
            .expect("fixed growing config should build"),
    )
    .expect("current fixed-growing pool should build");

    assert_eq!(
        fibers
            .spawn_with_stack::<4096, _, _>(|| 11_u32)
            .expect("task should spawn")
            .join()
            .expect("task should complete"),
        11
    );

    fibers
        .shutdown()
        .expect("current fiber pool should shut down cleanly");
}

#[cfg(feature = "std")]
#[test]
fn hosted_fiber_runtime_bootstrap_builds_automatic_carriers() {
    let _guard = crate::thread::hosted_test_guard();
    let runtime = FiberPoolBootstrap::fixed(2)
        .build_hosted()
        .expect("hosted runtime should build");
    assert_eq!(
        runtime.carriers().bootstrap(),
        HostedCarrierBootstrap::Direct
    );
    assert!(
        runtime
            .carriers()
            .worker_count()
            .expect("worker count should be observable")
            >= 1
    );
    assert_eq!(runtime.fibers().active_count(), 0);
    let (mut carriers, fibers) = runtime.into_parts();
    fibers
        .shutdown()
        .expect("hosted fiber pool should shut down cleanly");
    carriers
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[cfg(feature = "std")]
#[test]
fn hosted_fiber_runtime_fixed_growing_builds_from_total_budget() {
    let _guard = crate::thread::hosted_test_guard();
    let runtime = HostedFiberRuntime::fixed_growing_with_config(
        4,
        1,
        HostedFiberRuntimeConfig::new(1).with_placement(PoolPlacement::Inherit),
    )
    .expect("fixed growing hosted runtime should build");
    assert_eq!(runtime.fibers().active_count(), 0);
    let (mut carriers, fibers) = runtime.into_parts();
    fibers
        .shutdown()
        .expect("hosted fiber pool should shut down cleanly");
    carriers
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[cfg(feature = "std")]
#[test]
fn green_pool_skips_yield_budget_watchdog_without_budgeted_tasks() {
    let carriers = ThreadPool::new(&ThreadPoolConfig {
        min_threads: 2,
        max_threads: 2,
        placement: PoolPlacement::Inherit,
        ..ThreadPoolConfig::new()
    })
    .expect("carrier pool should build");
    let fibers = GreenPool::new(
        &FiberPoolConfig::fixed_growing(NonZeroUsize::new(4 * 1024).expect("non-zero stack"), 2, 1)
            .expect("fixed growing config should build")
            .with_reactor_policy(GreenReactorPolicy::Disabled),
        &carriers,
    )
    .expect("green pool should build");

    assert!(
        !fibers
            .inner
            .yield_budget_runtime
            .watchdog_started
            .load(Ordering::Acquire)
    );
    fibers
        .spawn_with_stack::<4096, _, _>(|| ())
        .expect("task should spawn")
        .join()
        .expect("task should complete");
    assert!(
        !fibers
            .inner
            .yield_budget_runtime
            .watchdog_started
            .load(Ordering::Acquire)
    );

    fibers
        .shutdown()
        .expect("green pool should shut down cleanly");
    carriers
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[cfg(feature = "std")]
#[test]
fn green_pool_starts_yield_budget_watchdog_for_budgeted_tasks() {
    let _guard = crate::thread::hosted_test_guard();
    let carriers = ThreadPool::new(&ThreadPoolConfig {
        min_threads: 2,
        max_threads: 2,
        placement: PoolPlacement::Inherit,
        ..ThreadPoolConfig::new()
    })
    .expect("carrier pool should build");
    let fibers = GreenPool::new(
        &FiberPoolConfig::fixed_growing(NonZeroUsize::new(4 * 1024).expect("non-zero stack"), 2, 1)
            .expect("fixed growing config should build")
            .with_reactor_policy(GreenReactorPolicy::Disabled)
            .with_yield_budget_policy(FiberYieldBudgetPolicy::Notify(record_yield_budget_event)),
        &carriers,
    )
    .expect("green pool should build");

    fibers
        .spawn_with_attrs(
            FiberTaskAttributes::new(FiberStackClass::MIN)
                .with_yield_budget(Duration::from_millis(5)),
            || (),
        )
        .expect("budgeted task should spawn")
        .join()
        .expect("budgeted task should complete");

    for _ in 0..1_000 {
        if fibers
            .inner
            .yield_budget_runtime
            .watchdog_started
            .load(Ordering::Acquire)
        {
            break;
        }
        std::thread::yield_now();
    }
    assert!(
        fibers
            .inner
            .yield_budget_runtime
            .watchdog_started
            .load(Ordering::Acquire)
    );

    fibers
        .shutdown()
        .expect("green pool should shut down cleanly");
    carriers
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[cfg(feature = "std")]
#[test]
fn hosted_green_yield_once_batch_fits_with_16k_stacks() {
    let _guard = crate::thread::hosted_test_guard();
    let carriers = ThreadPool::new(&ThreadPoolConfig {
        min_threads: 4,
        max_threads: 4,
        placement: PoolPlacement::Inherit,
        ..ThreadPoolConfig::new()
    })
    .expect("carrier pool should build");
    let fibers = GreenPool::new(
        &FiberPoolConfig::fixed_growing(
            NonZeroUsize::new(16 * 1024).expect("non-zero stack"),
            16,
            4,
        )
        .expect("fixed growing config should build")
        .with_telemetry(FiberTelemetry::Full)
        .with_reactor_policy(GreenReactorPolicy::Disabled),
        &carriers,
    )
    .expect("green pool should build");

    let mut handles = Vec::new();
    for _ in 0..16 {
        handles.push(
            fibers
                .spawn_with_stack::<4096, _, _>(|| {
                    yield_now().expect("yield should work");
                })
                .expect("task should spawn"),
        );
    }
    for handle in handles {
        handle.join().expect("task should complete");
    }

    let stats = fibers.stack_stats().expect("telemetry should be enabled");
    assert!(stats.peak_used_bytes <= 8 * 1024);

    fibers
        .shutdown()
        .expect("green pool should shut down cleanly");
    carriers
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[cfg(feature = "std")]
#[test]
fn hosted_green_inline_no_yield_spawn_join_stress_completes() {
    let _guard = crate::thread::hosted_test_guard();
    let carriers = ThreadPool::new(&ThreadPoolConfig {
        min_threads: 1,
        max_threads: 1,
        placement: PoolPlacement::Inherit,
        ..ThreadPoolConfig::new()
    })
    .expect("carrier pool should build");
    let fibers = GreenPool::new(
        &FiberPoolConfig::fixed_growing(NonZeroUsize::new(4 * 1024).expect("non-zero stack"), 4, 1)
            .expect("fixed growing config should build")
            .with_reactor_policy(GreenReactorPolicy::Disabled),
        &carriers,
    )
    .expect("green pool should build");

    for _ in 0..1_000 {
        fibers
            .spawn_explicit(SupportedInlineNoYieldTask)
            .expect("inline no-yield task should spawn")
            .join()
            .expect("inline no-yield task should complete");
    }

    fibers
        .shutdown()
        .expect("green pool should shut down cleanly");
    carriers
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[cfg(feature = "std")]
#[test]
fn hosted_green_inline_no_yield_rapid_reuse_stays_alive() {
    let _guard = crate::thread::hosted_test_guard();
    let carriers = ThreadPool::new(&ThreadPoolConfig {
        min_threads: 1,
        max_threads: 1,
        placement: PoolPlacement::Inherit,
        ..ThreadPoolConfig::new()
    })
    .expect("carrier pool should build");
    let fibers = GreenPool::new(
        &FiberPoolConfig::fixed(NonZeroUsize::new(16 * 1024).expect("non-zero stack"), 64),
        &carriers,
    )
    .expect("green pool should build");

    for _ in 0..1_000 {
        fibers
            .spawn_explicit(SupportedInlineNoYieldTask)
            .expect("inline no-yield task should spawn")
            .join()
            .expect("inline no-yield task should complete");
    }

    fibers
        .shutdown()
        .expect("green pool should shut down cleanly");
    carriers
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[cfg(feature = "std")]
#[test]
fn hosted_green_yield_once_rapid_reuse_stays_alive() {
    let _guard = crate::thread::hosted_test_guard();
    let carriers = ThreadPool::new(&ThreadPoolConfig {
        min_threads: 1,
        max_threads: 1,
        placement: PoolPlacement::Inherit,
        ..ThreadPoolConfig::new()
    })
    .expect("carrier pool should build");
    let fibers = GreenPool::new(
        &FiberPoolConfig::fixed(NonZeroUsize::new(16 * 1024).expect("non-zero stack"), 64),
        &carriers,
    )
    .expect("green pool should build");

    for _ in 0..1_000 {
        fibers
            .spawn_with_stack::<4096, _, _>(|| {
                yield_now().expect("yield should work");
            })
            .expect("yielding task should spawn")
            .join()
            .expect("yielding task should complete");
    }

    fibers
        .shutdown()
        .expect("green pool should shut down cleanly");
    carriers
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[cfg(feature = "std")]
#[test]
fn hosted_fiber_runtime_classed_builds_from_total_budget() {
    let _guard = crate::thread::hosted_test_guard();
    let runtime = HostedFiberRuntime::classed(&[
        HostedFiberClassConfig::new(FiberStackClass::MIN, 2)
            .expect("hosted class config should build"),
        HostedFiberClassConfig::new(
            FiberStackClass::new(NonZeroUsize::new(8 * 1024).expect("non-zero class"))
                .expect("valid class"),
            2,
        )
        .expect("hosted class config should build"),
    ])
    .expect("classed hosted runtime should build");
    assert_eq!(runtime.fibers().active_count(), 0);
    let (mut carriers, fibers) = runtime.into_parts();
    fibers
        .shutdown()
        .expect("hosted fiber pool should shut down cleanly");
    carriers
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[cfg(feature = "std")]
#[test]
fn hosted_runtime_config_defaults_to_fusion_carrier_shape() {
    let _guard = crate::thread::hosted_test_guard();
    let automatic = HostedFiberRuntimeConfig::automatic();
    assert!(automatic.carrier_count >= 1);
    assert_eq!(automatic.name_prefix, Some("fusion-fiber"));

    let explicit = HostedFiberRuntimeConfig::new(2)
        .with_placement(PoolPlacement::PerCore)
        .with_name_prefix(Some("fusion-explicit"));
    assert_eq!(explicit.carrier_count, 2);
    assert_eq!(explicit.placement, PoolPlacement::PerCore);
    assert_eq!(explicit.name_prefix, Some("fusion-explicit"));
}

#[cfg(feature = "std")]
#[test]
fn hosted_fiber_runtime_respects_explicit_carrier_config() {
    let _guard = crate::thread::hosted_test_guard();
    let runtime = FiberPoolBootstrap::fixed(2)
        .build_hosted_with(
            HostedFiberRuntimeConfig::new(1)
                .with_placement(PoolPlacement::Inherit)
                .with_name_prefix(Some("fusion-test")),
        )
        .expect("hosted runtime should build from explicit carrier config");
    assert_eq!(
        runtime
            .carriers()
            .worker_count()
            .expect("worker count should be observable"),
        1
    );
    assert_eq!(
        runtime.carriers().bootstrap(),
        HostedCarrierBootstrap::Direct
    );
    let (mut carriers, fibers) = runtime.into_parts();
    fibers
        .shutdown()
        .expect("hosted fiber pool should shut down cleanly");
    carriers
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[cfg(feature = "std")]
#[test]
fn hosted_fiber_runtime_can_use_composed_thread_pool_bootstrap() {
    let _guard = crate::thread::hosted_test_guard();
    let runtime = FiberPoolBootstrap::fixed(2)
        .build_hosted_with(
            HostedFiberRuntimeConfig::new(1)
                .with_bootstrap(HostedCarrierBootstrap::ThreadPool)
                .with_placement(PoolPlacement::Inherit),
        )
        .expect("hosted runtime should build from composed carrier config");
    assert_eq!(
        runtime.carriers().bootstrap(),
        HostedCarrierBootstrap::ThreadPool
    );
    assert_eq!(
        runtime
            .carriers()
            .worker_count()
            .expect("worker count should be observable"),
        1
    );
    assert!(runtime.carriers().thread_pool().is_some());
    let (mut carriers, fibers) = runtime.into_parts();
    fibers
        .shutdown()
        .expect("hosted fiber pool should shut down cleanly");
    carriers
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}
