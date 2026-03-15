use core::sync::atomic::{AtomicU32, Ordering};

use fusion_pal::sys::mem::{MemBase, system_mem};
use fusion_std::sync::{Mutex, Once, OnceLock, RawMutex, RwLock, Semaphore, SpinMutex, ThinMutex};
use fusion_std::thread::{
    DeterministicConstraints, EventInterest, EventNotification, EventReadiness, EventRecord,
    EventSourceHandle, Executor, ExecutorConfig, ExecutorError, ExecutorMode, GreenPool,
    GreenPoolConfig, Runtime, RuntimeConfig, RuntimeError, RuntimeProfile, ThreadConfig,
    ThreadEntryReturn, ThreadErrorKind, ThreadJoinPolicy, ThreadLifecycleCaps, ThreadPool,
    ThreadPoolConfig, system_thread, yield_now as green_yield_now,
};
use fusion_sys::fiber::FiberSystem;
#[cfg(target_os = "linux")]
use std::io::Write;
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
#[cfg(target_os = "linux")]
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::thread;
#[cfg(target_os = "linux")]
use std::time::Duration;

#[test]
fn sync_facade_mutex_and_rwlock_round_trip() {
    let mutex = Mutex::new(1_u32);
    *mutex.lock().expect("mutex should lock") += 1;
    assert_eq!(*mutex.lock().expect("mutex should lock"), 2);

    let lock = RwLock::new(7_u32);
    assert_eq!(*lock.read().expect("read lock should succeed"), 7);
    *lock.write().expect("write lock should succeed") = 9;
    assert_eq!(*lock.read().expect("read lock should succeed"), 9);
}

#[test]
fn sync_facade_once_lock_initializes_once() {
    let cell = OnceLock::new();
    let value = cell
        .get_or_init(|| 42_u32)
        .expect("once lock should initialize");
    assert_eq!(*value, 42);
    assert_eq!(
        *cell
            .get_or_init(|| 99_u32)
            .expect("once lock should reuse initialized value"),
        42
    );
}

#[test]
fn sync_facade_exposes_remaining_core_primitives() {
    let once = Arc::new(Once::new());
    if once.support().implementation == fusion_std::sync::SyncImplementationKind::Unsupported {
        assert_eq!(
            once.call_once(|| {})
                .expect_err("unsupported once should fail"),
            fusion_std::sync::SyncError::unsupported()
        );
        return;
    }

    let runs = Arc::new(AtomicU32::new(0));
    let mut threads = std::vec::Vec::new();

    for _ in 0..4 {
        let once = Arc::clone(&once);
        let runs = Arc::clone(&runs);
        threads.push(thread::spawn(move || {
            once.call_once(|| {
                runs.fetch_add(1, Ordering::AcqRel);
            })
            .expect("once should coordinate");
        }));
    }

    for thread in threads {
        thread.join().expect("worker should finish");
    }

    assert_eq!(runs.load(Ordering::Acquire), 1);

    let thin = Arc::new(ThinMutex::new());
    let counter = Arc::new(AtomicU32::new(0));
    let mut thin_threads = std::vec::Vec::new();
    for _ in 0..4 {
        let thin = Arc::clone(&thin);
        let counter = Arc::clone(&counter);
        thin_threads.push(thread::spawn(move || {
            for _ in 0..100 {
                let _guard = thin.lock().expect("thin mutex should lock");
                counter.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }
    for thread in thin_threads {
        thread.join().expect("thin mutex worker should finish");
    }
    assert_eq!(counter.load(Ordering::Relaxed), 400);

    let spin = SpinMutex::new();
    assert!(spin.try_lock().expect("first spin try_lock should succeed"));
    unsafe { spin.unlock_unchecked() };

    match Semaphore::new(1, 2) {
        Ok(semaphore) => {
            assert!(
                semaphore
                    .try_acquire()
                    .expect("first acquire should evaluate")
            );
            assert!(
                !semaphore
                    .try_acquire()
                    .expect("second acquire should evaluate")
            );
            semaphore
                .release(1)
                .expect("release should restore a permit");
        }
        Err(error) => assert_eq!(error.kind, fusion_std::sync::SyncErrorKind::Unsupported),
    }
}

#[test]
fn thread_pool_facade_reports_lower_level_support_honestly() {
    let support = ThreadPool::support();
    let system_support = fusion_sys::thread::ThreadSystem::new().support();
    assert_eq!(support, system_support);

    let pool = ThreadPool::new(&ThreadPoolConfig::new()).expect("thread pool should build");
    let completed = Arc::new(AtomicU32::new(0));
    for _ in 0..6 {
        let completed = Arc::clone(&completed);
        pool.submit(move || {
            completed.fetch_add(1, Ordering::AcqRel);
        })
        .expect("pool should accept submitted work");
    }

    pool.shutdown().expect("pool should drain and shut down");
    assert_eq!(completed.load(Ordering::Acquire), 6);
}

#[repr(C)]
struct ExitContext<'a> {
    touched: &'a AtomicU32,
}

unsafe fn exit_entry(context: *mut ()) -> ThreadEntryReturn {
    let context = unsafe { &*(context.cast::<ExitContext<'_>>()) };
    context.touched.store(1, Ordering::Release);
    ThreadEntryReturn::new(17)
}

#[test]
fn thread_system_facade_spawns_and_joins_like_lower_layers() {
    let thread = system_thread();
    let support = thread.support();
    let touched = AtomicU32::new(0);
    let context = ExitContext { touched: &touched };

    let handle = unsafe {
        thread.spawn_raw(
            &ThreadConfig::new(),
            exit_entry,
            (&raw const context).cast_mut().cast(),
        )
    };

    if !support.lifecycle.caps.contains(ThreadLifecycleCaps::SPAWN) {
        assert_eq!(
            handle
                .expect_err("unsupported thread facade should reject spawn")
                .kind(),
            ThreadErrorKind::Unsupported
        );
        return;
    }

    let handle = handle.expect("thread facade should spawn on supported backend");

    let termination = thread.join(handle).expect("thread facade should join");
    assert_eq!(termination.code.map(|code| code.0), Some(17));
    assert_eq!(touched.load(Ordering::Acquire), 1);

    let detached = unsafe {
        thread.spawn_raw(
            &ThreadConfig {
                join_policy: ThreadJoinPolicy::Detached,
                ..ThreadConfig::new()
            },
            exit_entry,
            (&raw const context).cast_mut().cast(),
        )
    }
    .expect("detached thread should spawn");

    assert_eq!(
        thread
            .join(detached)
            .expect_err("detached thread should not join")
            .kind(),
        ThreadErrorKind::StateConflict
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn executor_green_pool_and_runtime_paths_are_real() {
    let current = Executor::new(ExecutorConfig::new());
    assert_eq!(current.mode(), ExecutorMode::CurrentThread);
    assert_eq!(
        current
            .block_on(async { 5_u8 })
            .expect("current-thread executor should drive one future"),
        5
    );
    assert!(matches!(
        current.spawn_local(async { 7_u8 }),
        Err(ExecutorError::Unsupported)
    ));

    let carrier = ThreadPool::new(&ThreadPoolConfig::new()).expect("carrier pool should build");
    let pool_executor = Executor::new(ExecutorConfig {
        mode: ExecutorMode::ThreadPool,
        ..ExecutorConfig::new()
    })
    .on_pool(&carrier)
    .expect("executor should bind to the carrier pool");
    let pool_task = pool_executor
        .spawn(async { 11_u8 })
        .expect("thread-pool executor should spawn work");
    assert_eq!(
        pool_task
            .join()
            .expect("thread-pool executor task should complete"),
        11
    );

    assert_eq!(GreenPool::support(), FiberSystem::new().support());
    let green = GreenPool::new(&GreenPoolConfig::new(), &carrier)
        .expect("green pool should build on the carrier pool");
    let runs = Arc::new(AtomicU32::new(0));
    let runs_for_green = Arc::clone(&runs);
    let green_job = green
        .spawn(move || {
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
        .spawn(async { 13_u8 })
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
        green: Some(GreenPoolConfig::new()),
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
        .spawn(async { 17_u8 })
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
fn green_pool_supports_guarded_stacks_and_rejects_oversized_jobs() {
    let carrier = ThreadPool::new(&ThreadPoolConfig::new()).expect("carrier pool should build");
    let page = system_mem().page_info().base_page.get();
    let guarded = GreenPool::new(
        &GreenPoolConfig {
            guard_bytes: page,
            ..GreenPoolConfig::new()
        },
        &carrier,
    )
    .expect("green pool should build with guard-backed stacks");

    let runs = Arc::new(AtomicU32::new(0));
    let runs_for_job = Arc::clone(&runs);
    let handle = guarded
        .spawn(move || {
            runs_for_job.fetch_add(1, Ordering::AcqRel);
        })
        .expect("guarded green pool should spawn a bounded job");
    let clone = handle.clone();
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
            .spawn(move || {
                std::hint::black_box(oversized);
            })
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

#[cfg(target_os = "linux")]
#[test]
fn reactor_facade_exposes_lower_level_readiness_polling() {
    let executor = Executor::new(ExecutorConfig::new());
    let reactor = executor.reactor();
    let mut poller = reactor.create().expect("reactor should create a poller");
    let (reader, mut writer) = UnixStream::pair().expect("unix stream pair should create");

    let key = reactor
        .register(
            &mut poller,
            EventSourceHandle(
                usize::try_from(reader.as_raw_fd()).expect("unix stream fd should be non-negative"),
            ),
            EventInterest::READABLE | EventInterest::ERROR | EventInterest::HANGUP,
        )
        .expect("reactor should register the reader");

    writer
        .write_all(b"x")
        .expect("writer should make the reader readable");

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
        .expect("reactor should deregister the reader");
}
