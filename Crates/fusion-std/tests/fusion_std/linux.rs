use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{
    AtomicU32,
    Ordering,
};
use core::task::{
    Context,
    Poll,
};

use std::num::NonZeroUsize;
use std::process::Command;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use fusion_pal::sys::mem::{
    MemAdviceCaps,
    MemBase,
    system_mem,
};
use fusion_std::sync::Mutex as FusionMutex;
use fusion_std::thread::{
    AsyncPollStackContract,
    CurrentAsyncRuntime,
    DeterministicConstraints,
    EventInterest,
    EventNotification,
    EventReadiness,
    EventRecord,
    EventSourceHandle,
    Executor,
    ExecutorConfig,
    ExecutorMode,
    ExplicitFiberTask,
    FiberStackBacking,
    FiberStackClass,
    FiberStackClassConfig,
    FiberTaskAttributes,
    FiberTaskPriority,
    FiberTelemetry,
    GeneratedExplicitFiberTask,
    GreenGrowth,
    GreenPool,
    GreenPoolConfig,
    GreenScheduling,
    HugePagePolicy,
    HugePageSize,
    RedDispatchPolicy,
    RedThread,
    RedThreadConfig,
    Runtime,
    RuntimeConfig,
    RuntimeError,
    RuntimeProfile,
    TaskPlacement,
    ThreadPool,
    ThreadPoolConfig,
    TieredGreenPool,
    TieredGreenPoolConfig,
    TieredTaskAttributes,
    admit_generated_fiber_task_stack_bytes,
    generated_explicit_task_contract_attributes,
    wait_for_readiness,
    yield_now as green_yield_now,
};
use fusion_sys::fiber::FiberError;
use fusion_sys::fiber::FiberSystem;

use super::lock_fusion_std_tests;

#[derive(Debug)]
struct TestPipe {
    read_fd: i32,
    write_fd: i32,
}

impl TestPipe {
    fn new() -> Self {
        let mut fds = [0_i32; 2];
        let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) };
        assert_eq!(rc, 0, "nonblocking test pipe should create");
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
        assert_eq!(rc, 1, "pipe writer should make the reader readable");
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
            assert_eq!(rc, -1, "pipe read should either succeed or report errno");
            let errno = unsafe { *libc::__errno_location() };
            if errno == libc::EINTR {
                continue;
            }
            panic!("pipe read should complete after readiness, errno={errno}");
        }
    }
}

struct ExplicitContractTask(u32);

impl ExplicitFiberTask for ExplicitContractTask {
    type Output = u32;

    const STACK_BYTES: NonZeroUsize = NonZeroUsize::new(6 * 1024).unwrap();
    const PRIORITY: FiberTaskPriority = FiberTaskPriority::new(7);

    fn run(self) -> Self::Output {
        self.0 + 1
    }
}

struct ExternalGeneratedContractTask(u32);

impl GeneratedExplicitFiberTask for ExternalGeneratedContractTask {
    type Output = u32;

    fn run(self) -> Self::Output {
        self.0 + 2
    }

    fn task_attributes() -> Result<FiberTaskAttributes, FiberError>
    where
        Self: Sized,
    {
        Ok(generated_explicit_task_contract_attributes::<Self>())
    }
}

struct ExternalGeneratedAsyncPollStackFuture;

impl Future for ExternalGeneratedAsyncPollStackFuture {
    type Output = u8;

    fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(29)
    }
}

fusion_std::declare_generated_async_poll_stack_contract!(
    ExternalGeneratedAsyncPollStackFuture,
    1792
);

fusion_std::declare_generated_fiber_task_contract!(
    ExternalGeneratedContractTask,
    NonZeroUsize::new(8 * 1024).unwrap(),
    FiberTaskPriority::new(9),
);

struct ExternalGeneratedContractOnlyTask(u32);

impl GeneratedExplicitFiberTask for ExternalGeneratedContractOnlyTask {
    type Output = u32;

    fn run(self) -> Self::Output {
        self.0 + 4
    }
}

fusion_std::declare_generated_fiber_task_contract!(
    ExternalGeneratedContractOnlyTask,
    NonZeroUsize::new(8 * 1024).unwrap(),
    FiberTaskPriority::new(10),
);

const EXTERNAL_GENERATED_ADMITTED_STACK_BYTES: NonZeroUsize =
    match admit_generated_fiber_task_stack_bytes(
        NonZeroUsize::new(8 * 1024).expect("non-zero generated stack"),
    ) {
        Ok(stack_bytes) => stack_bytes,
        Err(_) => panic!("generated stack bytes should admit"),
    };
const EXTERNAL_GENERATED_CLASS: FiberStackClass =
    match FiberStackClass::from_stack_bytes(EXTERNAL_GENERATED_ADMITTED_STACK_BYTES) {
        Ok(class) => class,
        Err(_) => panic!("generated class should be valid"),
    };

const EXTERNAL_GENERATED_CLASSES: [FiberStackClassConfig; 1] = [
    match FiberStackClassConfig::new(EXTERNAL_GENERATED_CLASS, 2) {
        Ok(class) => class,
        Err(_) => panic!("valid class config"),
    },
];

const EXTERNAL_GENERATED_CONFIG: GreenPoolConfig<'static> =
    match GreenPoolConfig::classed(&EXTERNAL_GENERATED_CLASSES) {
        Ok(config) => config,
        Err(_) => panic!("classed config should build"),
    };

fusion_std::assert_generated_fiber_task_supported!(
    EXTERNAL_GENERATED_CONFIG,
    ExternalGeneratedContractTask
);

const TEST_MIN_FIBER_ATTRIBUTES: FiberTaskAttributes =
    FiberTaskAttributes::new(FiberStackClass::MIN);

#[cfg(feature = "critical-safe")]
struct FeatureStrictGeneratedContractTask(u32);

#[cfg(feature = "critical-safe")]
impl GeneratedExplicitFiberTask for FeatureStrictGeneratedContractTask {
    type Output = u32;

    fn run(self) -> Self::Output {
        self.0 + 3
    }
}

#[cfg(feature = "critical-safe")]
fusion_std::declare_generated_fiber_task_contract!(
    FeatureStrictGeneratedContractTask,
    NonZeroUsize::new(8 * 1024).unwrap(),
    FiberTaskPriority::new(11),
);

#[cfg(feature = "critical-safe")]
const STRICT_GENERATED_ADMITTED_STACK_BYTES: NonZeroUsize =
    match admit_generated_fiber_task_stack_bytes(
        NonZeroUsize::new(8 * 1024).expect("non-zero strict generated stack"),
    ) {
        Ok(stack_bytes) => stack_bytes,
        Err(_) => panic!("generated stack bytes should admit"),
    };
#[cfg(feature = "critical-safe")]
const STRICT_GENERATED_CLASS: FiberStackClass =
    match FiberStackClass::from_stack_bytes(STRICT_GENERATED_ADMITTED_STACK_BYTES) {
        Ok(class) => class,
        Err(_) => panic!("generated class should be valid"),
    };

#[cfg(feature = "critical-safe")]
const STRICT_GENERATED_CLASSES: [FiberStackClassConfig; 1] = [
    match FiberStackClassConfig::new(STRICT_GENERATED_CLASS, 2) {
        Ok(class) => class,
        Err(_) => panic!("valid class config"),
    },
];

#[cfg(feature = "critical-safe")]
const STRICT_GENERATED_CONFIG: GreenPoolConfig<'static> =
    match GreenPoolConfig::classed(&STRICT_GENERATED_CLASSES) {
        Ok(config) => config,
        Err(_) => panic!("classed config should build"),
    };

#[cfg(feature = "critical-safe")]
fusion_std::assert_generated_fiber_task_supported!(
    STRICT_GENERATED_CONFIG,
    FeatureStrictGeneratedContractTask
);

impl Drop for TestPipe {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.read_fd);
            libc::close(self.write_fd);
        }
    }
}

#[test]
fn automatic_green_pool_bootstraps_and_runs_work() {
    let _guard = lock_fusion_std_tests();

    let automatic_a = GreenPool::automatic().expect("automatic green pool should bootstrap");
    let automatic_b =
        GreenPool::automatic().expect("automatic green pool should reuse the shared runtime");

    let task = automatic_a
        .spawn_with_attrs(TEST_MIN_FIBER_ATTRIBUTES, || 17_u32)
        .expect("automatic green pool should accept work");
    assert_eq!(
        task.join()
            .expect("automatic green task should complete with a result"),
        17
    );

    let second = automatic_b
        .spawn_with_attrs(TEST_MIN_FIBER_ATTRIBUTES, || 23_u32)
        .expect("shared automatic green pool should also accept work");
    assert_eq!(
        second
            .join()
            .expect("second automatic green task should complete"),
        23
    );
}

#[test]
fn downstream_generated_task_contracts_work_without_runtime_type_lookup() {
    let _guard = lock_fusion_std_tests();

    let carrier = ThreadPool::new(&ThreadPoolConfig::new()).expect("carrier pool should build");
    let green =
        GreenPool::new(&EXTERNAL_GENERATED_CONFIG, &carrier).expect("green pool should build");

    let handle = green
        .spawn_generated(ExternalGeneratedContractTask(5))
        .expect("external generated task should spawn from declared contract");
    assert_eq!(
        handle
            .join()
            .expect("external generated task should complete"),
        7
    );

    green
        .shutdown()
        .expect("green pool should shut down cleanly");
    carrier
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[test]
fn downstream_generated_contract_first_spawn_works_without_runtime_metadata_override() {
    let _guard = lock_fusion_std_tests();

    let carrier = ThreadPool::new(&ThreadPoolConfig::new()).expect("carrier pool should build");
    let classes = [
        FiberStackClassConfig::new(FiberStackClass::MIN, 4).expect("valid class config"),
        FiberStackClassConfig::new(EXTERNAL_GENERATED_CLASS, 2).expect("valid class config"),
    ];
    let green = GreenPool::new(
        &GreenPoolConfig::classed(&classes).expect("classed green config should build"),
        &carrier,
    )
    .expect("green pool should build");

    green
        .validate_generated_task_contract::<ExternalGeneratedContractOnlyTask>()
        .expect("live pool should accept compile-time generated contracts");

    let handle = green
        .spawn_generated_contract(ExternalGeneratedContractOnlyTask(38))
        .expect("contract-first generated task should spawn");
    assert_eq!(
        handle
            .join()
            .expect("contract-first generated task should complete"),
        42
    );

    green
        .shutdown()
        .expect("green pool should shut down cleanly");
    carrier
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[test]
fn downstream_generated_async_poll_stack_contracts_work_without_runtime_type_lookup() {
    let _guard = lock_fusion_std_tests();

    let runtime = CurrentAsyncRuntime::new();
    let handle = runtime
        .spawn_generated(ExternalGeneratedAsyncPollStackFuture)
        .expect("external generated async contract should spawn");
    assert_eq!(
        handle.admission().poll_stack,
        AsyncPollStackContract::Explicit { bytes: 1792 }
    );
    assert_eq!(handle.join().expect("task should complete"), 29);
}

#[cfg(feature = "critical-safe")]
#[test]
fn strict_generated_contract_feature_bypasses_runtime_metadata_lookup() {
    let _guard = lock_fusion_std_tests();

    let carrier = ThreadPool::new(&ThreadPoolConfig::new()).expect("carrier pool should build");
    let green =
        GreenPool::new(&STRICT_GENERATED_CONFIG, &carrier).expect("green pool should build");

    green
        .validate_generated_task::<FeatureStrictGeneratedContractTask>()
        .expect("strict generated task should validate from compile-time contract");

    let handle = green
        .spawn_generated(FeatureStrictGeneratedContractTask(5))
        .expect("strict generated task should spawn from compile-time contract");
    assert_eq!(
        handle
            .join()
            .expect("strict generated task should complete"),
        8
    );

    green
        .shutdown()
        .expect("green pool should shut down cleanly");
    carrier
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[test]
fn executor_green_pool_and_runtime_paths_are_real() {
    let _guard = lock_fusion_std_tests();

    let carrier = ThreadPool::new(&ThreadPoolConfig::new()).expect("carrier pool should build");

    assert_eq!(GreenPool::support(), FiberSystem::new().support());
    let green = GreenPool::new(&GreenPoolConfig::new(), &carrier)
        .expect("green pool should build on the carrier pool");
    let runs = Arc::new(AtomicU32::new(0));
    let runs_for_green = Arc::clone(&runs);
    let green_job = green
        .spawn_with_attrs(TEST_MIN_FIBER_ATTRIBUTES, move || {
            runs_for_green.fetch_add(1, Ordering::AcqRel);
            green_yield_now().expect("green task should yield cooperatively");
            runs_for_green.fetch_add(1, Ordering::AcqRel);
        })
        .expect("green pool should spawn a job");
    green_job
        .join()
        .expect("green job should finish after yielding once");
    assert_eq!(runs.load(Ordering::Acquire), 2);

    let green_executor = Executor::new(ExecutorConfig {
        mode: ExecutorMode::GreenPool,
        ..ExecutorConfig::new()
    })
    .on_green(&green)
    .expect("executor should bind to the green pool");
    let green_task = green_executor
        .spawn_with_poll_stack_bytes(2048, async { 13_u8 })
        .expect("green-backed executor should spawn work");
    assert_eq!(
        green_task
            .join()
            .expect("green-backed executor task should complete"),
        13
    );

    let runtime = Runtime::new(&RuntimeConfig {
        profile: RuntimeProfile::Deterministic,
        thread_pool: ThreadPoolConfig::new(),
        green: Some(GreenPoolConfig::fixed(
            NonZeroUsize::new(64 * 1024).expect("non-zero fixed fiber stack"),
            64,
        )),
        executor: ExecutorConfig {
            mode: ExecutorMode::GreenPool,
            ..ExecutorConfig::new()
        },
        deterministic: Some(DeterministicConstraints::strict()),
        elastic: None,
    })
    .expect("runtime should build a carrier pool, green pool, and executor");
    assert!(runtime.thread_pool().is_some());
    assert!(runtime.green_pool().is_some());
    assert_eq!(
        runtime
            .stats()
            .expect("runtime stats should remain observable")
            .carrier_workers,
        1
    );

    let runtime_task = runtime
        .executor()
        .spawn_with_poll_stack_bytes(2048, async { 17_u8 })
        .expect("runtime executor should spawn onto the green pool");
    assert_eq!(
        runtime_task
            .join()
            .expect("runtime executor task should complete"),
        17
    );

    let unsupported_hybrid = Runtime::new(&RuntimeConfig {
        profile: RuntimeProfile::Balanced,
        thread_pool: ThreadPoolConfig::new(),
        green: Some(GreenPoolConfig::new()),
        executor: ExecutorConfig {
            mode: ExecutorMode::Hybrid,
            ..ExecutorConfig::new()
        },
        deterministic: None,
        elastic: None,
    });
    assert!(matches!(unsupported_hybrid, Err(RuntimeError::Unsupported)));

    green
        .shutdown()
        .expect("green pool should shut down cleanly");
    carrier
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[test]
fn deterministic_runtime_accepts_priority_class_backed_green_pools() {
    let _guard = lock_fusion_std_tests();

    let priority_classes = [
        FiberStackClassConfig::new(FiberStackClass::MIN, 16).expect("valid class config"),
        FiberStackClassConfig::new(
            FiberStackClass::new(
                NonZeroUsize::new(8 * 1024).expect("non-zero priority stack class"),
            )
            .expect("priority stack class should be valid"),
            8,
        )
        .expect("valid class config"),
        FiberStackClassConfig::new(
            FiberStackClass::new(
                NonZeroUsize::new(16 * 1024).expect("non-zero async dispatch stack class"),
            )
            .expect("async dispatch stack class should be valid"),
            4,
        )
        .expect("valid class config"),
    ];
    let priority_green = GreenPoolConfig::classed(&priority_classes)
        .expect("classed green config should build")
        .with_growth(GreenGrowth::Fixed)
        .with_scheduling(GreenScheduling::Priority);
    let priority_runtime = Runtime::new(&RuntimeConfig {
        profile: RuntimeProfile::Deterministic,
        thread_pool: ThreadPoolConfig::new(),
        green: Some(priority_green),
        executor: ExecutorConfig {
            mode: ExecutorMode::GreenPool,
            ..ExecutorConfig::new()
        },
        deterministic: Some(DeterministicConstraints::strict()),
        elastic: None,
    })
    .expect("deterministic runtime should accept strict-priority class-backed green pools");
    let priority_task = priority_runtime
        .executor()
        .spawn_with_poll_stack_bytes(2048, async { 19_u8 })
        .expect("priority runtime executor should spawn onto the green pool");
    assert_eq!(
        priority_task
            .join()
            .expect("priority runtime executor task should complete"),
        19
    );
}

#[test]
fn explicit_fiber_task_uses_compile_time_stack_contract() {
    let _guard = lock_fusion_std_tests();

    let carrier = ThreadPool::new(&ThreadPoolConfig::new()).expect("carrier pool should build");
    let classes = [
        FiberStackClassConfig::new(FiberStackClass::MIN, 8).expect("valid class config"),
        FiberStackClassConfig::new(
            FiberStackClass::new(NonZeroUsize::new(8 * 1024).expect("non-zero explicit class"))
                .expect("explicit class should be valid"),
            4,
        )
        .expect("valid class config"),
    ];
    let green = GreenPool::new(
        &GreenPoolConfig::classed(&classes).expect("classed green config should build"),
        &carrier,
    )
    .expect("green pool should build");

    let task = green
        .spawn_explicit(ExplicitContractTask(41))
        .expect("explicit task should spawn");
    assert_eq!(task.join().expect("explicit task should complete"), 42);

    green
        .shutdown()
        .expect("green pool should shut down cleanly");
    carrier
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[test]
fn tiered_green_pool_routes_auto_and_explicit_work_honestly() {
    let _guard = lock_fusion_std_tests();

    let tiered = TieredGreenPool::new(&TieredGreenPoolConfig::new())
        .expect("tiered green scheduler should build");

    let negative_priority =
        TieredTaskAttributes::new(FiberTaskAttributes::new(FiberStackClass::MIN))
            .with_placement(TaskPlacement::Auto);
    let negative_priority = TieredTaskAttributes {
        fiber: negative_priority
            .fiber
            .with_priority(FiberTaskPriority::new(-5)),
        ..negative_priority
    };
    assert_eq!(
        tiered.resolve_tier(negative_priority),
        fusion_std::thread::CarrierTier::Efficiency
    );
    assert_eq!(
        tiered.resolve_tier(negative_priority.with_placement(TaskPlacement::Tier(
            fusion_std::thread::CarrierTier::Performance,
        )),),
        fusion_std::thread::CarrierTier::Performance
    );

    let started = Arc::new(AtomicU32::new(0));
    let gate = Arc::new(AtomicU32::new(0));
    let low_started = Arc::clone(&started);
    let low_gate = Arc::clone(&gate);
    let handle = tiered
        .spawn_with_task(negative_priority, move || {
            low_started.store(1, Ordering::Release);
            while low_gate.load(Ordering::Acquire) == 0 {
                green_yield_now().expect("tiered green task should yield cooperatively");
            }
            11_u32
        })
        .expect("tiered green scheduler should accept efficiency-tier work");
    while started.load(Ordering::Acquire) == 0 {
        thread::yield_now();
    }

    let stats = tiered
        .stats()
        .expect("tiered green stats should remain observable");
    assert_eq!(stats.performance_green_threads, 0);
    assert_eq!(stats.efficiency_green_threads, 1);

    gate.store(1, Ordering::Release);
    assert_eq!(
        handle.join().expect("tiered green task should complete"),
        11
    );
    tiered
        .shutdown()
        .expect("tiered green scheduler should shut down cleanly");
}

#[test]
fn red_thread_executes_native_urgent_work() {
    let _guard = lock_fusion_std_tests();

    let red = RedThread::spawn(&RedThreadConfig::new(), || 23_u32)
        .expect("red thread should spawn on hosted native threads");
    let admission = red.admission();
    assert_eq!(
        admission.reservation,
        fusion_sys::thread::ThreadGuarantee::Verified
    );
    assert_eq!(red.join().expect("red thread should join cleanly"), 23);
}

#[test]
fn red_thread_rejects_unimplemented_queue_policy() {
    let _guard = lock_fusion_std_tests();

    let error = RedThread::spawn(
        &RedThreadConfig::new().with_dispatch(RedDispatchPolicy::QueueIfBusy),
        || 1_u32,
    )
    .expect_err("queued urgent dispatch should stay unsupported until real reservation exists");
    assert_eq!(
        error.kind(),
        fusion_sys::thread::ThreadError::unsupported().kind()
    );
}

#[test]
fn priority_green_pool_rejects_multi_carrier_topology_until_domain_semantics_exist() {
    let _guard = lock_fusion_std_tests();

    let carriers = ThreadPool::new(&ThreadPoolConfig {
        min_threads: 2,
        max_threads: 2,
        placement: fusion_std::thread::PoolPlacement::Inherit,
        ..ThreadPoolConfig::new()
    })
    .expect("two-carrier pool should build");
    let priority_classes =
        [FiberStackClassConfig::new(FiberStackClass::MIN, 4).expect("valid class config")];
    let config = GreenPoolConfig::classed(&priority_classes)
        .expect("classed green config should build")
        .with_scheduling(GreenScheduling::Priority);

    let error = GreenPool::new(&config, &carriers)
        .expect_err("multi-carrier priority should stay unsupported until domains exist");
    assert_eq!(error.kind(), FiberError::unsupported().kind());

    carriers
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[test]
fn green_pool_supports_guarded_stacks_and_rejects_oversized_jobs() {
    let _guard = lock_fusion_std_tests();

    let carrier = ThreadPool::new(&ThreadPoolConfig::new()).expect("carrier pool should build");
    let guarded = GreenPool::new(
        &GreenPoolConfig {
            guard_pages: 1,
            ..GreenPoolConfig::new()
        },
        &carrier,
    )
    .expect("green pool should build with guard-backed stacks");

    let runs = Arc::new(AtomicU32::new(0));
    let runs_for_job = Arc::clone(&runs);
    let handle = guarded
        .spawn_with_attrs(TEST_MIN_FIBER_ATTRIBUTES, move || {
            runs_for_job.fetch_add(1, Ordering::AcqRel);
        })
        .expect("guarded green pool should spawn a bounded job");
    let clone = handle
        .try_clone()
        .expect("green handle should clone honestly");
    handle
        .join()
        .expect("first handle should observe green completion");
    clone
        .join()
        .expect("cloned handle should also observe green completion");
    assert_eq!(runs.load(Ordering::Acquire), 1);

    let oversized = [0_u8; 1024];
    assert_eq!(
        guarded
            .spawn_with_attrs(
                FiberTaskAttributes::new(
                    FiberStackClass::new(
                        NonZeroUsize::new(2 * 1024 * 1024).expect("non-zero oversized class"),
                    )
                    .expect("oversized class should be valid"),
                ),
                move || {
                    std::hint::black_box(oversized);
                },
            )
            .expect_err("oversized green jobs should be rejected honestly")
            .kind(),
        fusion_sys::fiber::FiberError::unsupported().kind()
    );

    guarded
        .shutdown()
        .expect("guarded green pool should shut down cleanly");
    carrier
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[test]
fn elastic_fiber_pool_builds_and_runs_jobs() {
    let _guard = lock_fusion_std_tests();

    let carrier = ThreadPool::new(&ThreadPoolConfig::new()).expect("carrier pool should build");
    let fibers = GreenPool::new(
        &GreenPoolConfig {
            stack_backing: FiberStackBacking::Elastic {
                initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
                max_size: NonZeroUsize::new(64 * 1024).expect("non-zero max stack"),
            },
            ..GreenPoolConfig::new()
        },
        &carrier,
    )
    .expect("elastic green pool should build");

    let handle = fibers
        .spawn_with_attrs(TEST_MIN_FIBER_ATTRIBUTES, || 7_u32)
        .expect("elastic green pool should spawn");
    assert_eq!(handle.join().expect("elastic fiber should finish"), 7);

    fibers
        .shutdown()
        .expect("elastic green pool should shut down cleanly");
    carrier
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[test]
fn elastic_fiber_pool_accepts_manual_huge_page_advice() {
    let _guard = lock_fusion_std_tests();

    if !system_mem()
        .support()
        .advice
        .contains(MemAdviceCaps::HUGE_PAGE)
    {
        return;
    }

    let carrier = ThreadPool::new(&ThreadPoolConfig::new()).expect("carrier pool should build");
    let fibers = GreenPool::new(
        &GreenPoolConfig {
            stack_backing: FiberStackBacking::Elastic {
                initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
                max_size: NonZeroUsize::new(4 * 1024 * 1024).expect("non-zero max stack"),
            },
            huge_pages: HugePagePolicy::Enabled {
                size: HugePageSize::TwoMiB,
            },
            ..GreenPoolConfig::new()
        },
        &carrier,
    )
    .expect("huge-page-advised elastic green pool should build");

    let handle = fibers
        .spawn_with_attrs(TEST_MIN_FIBER_ATTRIBUTES, || 9_u32)
        .expect("huge-page-advised elastic pool should spawn");
    assert_eq!(
        handle
            .join()
            .expect("huge-page-advised elastic fiber should finish"),
        9
    );

    fibers
        .shutdown()
        .expect("huge-page-advised green pool should shut down cleanly");
    carrier
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[test]
fn elastic_fiber_fault_probe_survives_real_growth() {
    let _guard = lock_fusion_std_tests();

    let probe = env!("CARGO_BIN_EXE_fusion_std_fiber_fault_probe");
    let output = Command::new(probe)
        .output()
        .expect("fiber fault probe binary should execute");
    assert!(
        output.status.success(),
        "fiber fault probe failed: status={:?}, stdout={}, stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn fiber_pool_stack_stats_follow_telemetry_policy() {
    let _guard = lock_fusion_std_tests();

    let carrier = ThreadPool::new(&ThreadPoolConfig::new()).expect("carrier pool should build");
    let disabled =
        GreenPool::new(&GreenPoolConfig::new(), &carrier).expect("default green pool should build");
    assert!(disabled.stack_stats().is_none());
    disabled
        .shutdown()
        .expect("default green pool should shut down cleanly");

    let enabled = GreenPool::new(
        &GreenPoolConfig {
            telemetry: FiberTelemetry::Full,
            ..GreenPoolConfig::new()
        },
        &carrier,
    )
    .expect("telemetry-enabled green pool should build");
    let started = Arc::new(AtomicU32::new(0));
    let gate = Arc::new(AtomicU32::new(0));
    let started_for_job = Arc::clone(&started);
    let gate_for_job = Arc::clone(&gate);
    let handle = enabled
        .spawn_with_attrs(TEST_MIN_FIBER_ATTRIBUTES, move || {
            started_for_job.store(1, Ordering::Release);
            while gate_for_job.load(Ordering::Acquire) == 0 {
                green_yield_now().expect("fiber should yield cooperatively");
            }
            7_u32
        })
        .expect("telemetry-enabled green pool should spawn");
    while started.load(Ordering::Acquire) == 0 {
        thread::yield_now();
    }

    let stats = enabled
        .stack_stats()
        .expect("telemetry-enabled pool should report stack stats");
    assert_eq!(stats.total_growth_events, 0);
    assert_eq!(stats.peak_committed_pages, 1);
    assert_eq!(stats.committed_distribution.as_slice(), &[(1, 1)]);
    assert_eq!(stats.at_capacity_count, 0);

    gate.store(1, Ordering::Release);
    assert_eq!(handle.join().expect("fiber should complete"), 7);
    let drained = {
        let mut snapshot = None;
        for _ in 0..100 {
            let stats = enabled
                .stack_stats()
                .expect("telemetry-enabled pool should report stack stats");
            if stats.peak_committed_pages == 0 && stats.committed_distribution.is_empty() {
                snapshot = Some(stats);
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        snapshot.unwrap_or_else(|| {
            enabled
                .stack_stats()
                .expect("telemetry-enabled pool should report stack stats")
        })
    };
    assert_eq!(drained.total_growth_events, 0);
    assert_eq!(drained.peak_committed_pages, 0);
    assert!(drained.committed_distribution.is_empty());
    assert_eq!(drained.at_capacity_count, 0);

    enabled
        .shutdown()
        .expect("telemetry-enabled green pool should shut down cleanly");
    carrier
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[test]
fn green_pool_supports_typed_child_results_and_cooperative_join() {
    let _guard = lock_fusion_std_tests();

    let carrier = ThreadPool::new(&ThreadPoolConfig::new()).expect("carrier pool should build");
    let fibers = GreenPool::new(&GreenPoolConfig::new(), &carrier)
        .expect("green pool should build on the carrier pool");
    let child_pool = fibers
        .try_clone()
        .expect("green pool should clone honestly");

    let parent = fibers
        .spawn_with_attrs(TEST_MIN_FIBER_ATTRIBUTES, move || {
            let child = child_pool
                .spawn_with_attrs(TEST_MIN_FIBER_ATTRIBUTES, || 21_u32)
                .expect("child fiber should spawn");
            child
                .join()
                .expect("parent fiber should join child cooperatively")
                * 2
        })
        .expect("parent fiber should spawn");

    assert_eq!(
        parent
            .join()
            .expect("parent fiber should produce a typed result"),
        42
    );

    fibers
        .shutdown()
        .expect("green pool should shut down cleanly");
    carrier
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[test]
fn reactor_facade_exposes_lower_level_readiness_polling() {
    let _guard = lock_fusion_std_tests();

    let executor = Executor::new(ExecutorConfig::new());
    let reactor = executor.reactor();
    let mut poller = reactor.create().expect("reactor should create a poller");
    let pipe = TestPipe::new();

    let key = reactor
        .register(
            &mut poller,
            pipe.source(),
            EventInterest::READABLE | EventInterest::ERROR | EventInterest::HANGUP,
        )
        .expect("reactor should register the pipe reader");

    pipe.write_byte(b'x');

    let mut events = [EventRecord {
        key,
        notification: EventNotification::Readiness(EventReadiness::empty()),
    }; 4];
    let ready = reactor
        .poll(&mut poller, &mut events, Some(Duration::from_secs(1)))
        .expect("reactor poll should succeed");
    assert!(ready >= 1);

    let readiness = match events[0].notification {
        EventNotification::Readiness(readiness) => readiness,
        EventNotification::Completion(_) => {
            panic!("linux reactor façade should surface readiness notifications")
        }
    };
    assert!(readiness.contains(EventReadiness::READABLE));

    reactor
        .deregister(&mut poller, key)
        .expect("reactor should deregister the pipe reader");
}

#[test]
fn fibers_wait_on_pipe_readiness_and_resume_cleanly() {
    let _guard = lock_fusion_std_tests();

    let carrier = ThreadPool::new(&ThreadPoolConfig::new()).expect("carrier pool should build");
    let fibers = GreenPool::new(&GreenPoolConfig::new(), &carrier)
        .expect("green pool should build on the carrier pool");
    let pipe = Arc::new(TestPipe::new());

    let server = fibers
        .spawn_with_attrs(TEST_MIN_FIBER_ATTRIBUTES, {
            let pipe = Arc::clone(&pipe);
            move || {
                wait_for_readiness(
                    pipe.source(),
                    EventInterest::READABLE | EventInterest::ERROR | EventInterest::HANGUP,
                )
                .expect("fiber should park on pipe readiness and resume");
                pipe.read_byte()
            }
        })
        .expect("pipe-waiting fiber should spawn");

    let client = thread::spawn(move || {
        pipe.write_byte(b'p');
    });

    assert_eq!(
        server
            .join()
            .expect("fiber should complete after readiness wakeup"),
        b'p'
    );
    client.join().expect("pipe writer thread should finish");

    fibers
        .shutdown()
        .expect("green pool should shut down cleanly");
    carrier
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}

#[test]
fn fibers_reject_readiness_park_while_cooperative_mutex_is_held() {
    let _guard = lock_fusion_std_tests();

    let carrier = ThreadPool::new(&ThreadPoolConfig::new()).expect("carrier pool should build");
    let fibers = GreenPool::new(&GreenPoolConfig::new(), &carrier)
        .expect("green pool should build on the carrier pool");
    let pipe = Arc::new(TestPipe::new());
    let lock = Arc::new(FusionMutex::new(()));

    let task = fibers
        .spawn_with_attrs(TEST_MIN_FIBER_ATTRIBUTES, {
            let pipe = Arc::clone(&pipe);
            let lock = Arc::clone(&lock);
            move || -> Result<(), FiberError> {
                let _guard = lock.lock().expect("cooperative mutex should lock");
                let error = wait_for_readiness(
                    pipe.source(),
                    EventInterest::READABLE | EventInterest::ERROR | EventInterest::HANGUP,
                )
                .expect_err("park should reject while a cooperative mutex is held");
                assert_eq!(error.kind(), FiberError::state_conflict().kind());
                Ok(())
            }
        })
        .expect("pipe-waiting fiber should spawn");

    task.join()
        .expect("task should complete without runtime failure")
        .expect("task should observe the expected readiness rejection");

    fibers
        .shutdown()
        .expect("green pool should shut down cleanly");
    carrier
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}
