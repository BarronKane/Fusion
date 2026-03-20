use core::sync::atomic::{AtomicU32, Ordering};

use fusion_std::sync::{
    Mutex,
    Once,
    OnceLock,
    RawMutex,
    RwLock,
    Semaphore,
    SpinMutex,
    SyncErrorKind,
    ThinMutex,
};
use fusion_std::thread::{
    Executor,
    ExecutorConfig,
    ExecutorError,
    ExecutorMode,
    ThreadConfig,
    ThreadEntryReturn,
    ThreadErrorKind,
    ThreadJoinPolicy,
    ThreadLifecycleCaps,
    ThreadPool,
    ThreadPoolConfig,
    system_thread,
};
use std::sync::Arc;
use std::thread;

use super::lock_fusion_std_tests;

#[test]
fn sync_facade_mutex_and_rwlock_round_trip() {
    let _guard = lock_fusion_std_tests();

    let mutex = Mutex::new(1_u32);
    let mut first = match mutex.lock() {
        Ok(guard) => guard,
        Err(error) => {
            assert_eq!(error.kind, SyncErrorKind::Unsupported);
            return;
        }
    };
    *first += 1;
    drop(first);

    let second = match mutex.lock() {
        Ok(guard) => guard,
        Err(error) => {
            assert_eq!(error.kind, SyncErrorKind::Unsupported);
            return;
        }
    };
    assert_eq!(*second, 2);
    drop(second);

    let lock = RwLock::new(7_u32);
    let read = match lock.read() {
        Ok(guard) => guard,
        Err(error) => {
            assert_eq!(error.kind, SyncErrorKind::Unsupported);
            return;
        }
    };
    assert_eq!(*read, 7);
    drop(read);

    let mut write = match lock.write() {
        Ok(guard) => guard,
        Err(error) => {
            assert_eq!(error.kind, SyncErrorKind::Unsupported);
            return;
        }
    };
    *write = 9;
    drop(write);

    let read = lock
        .read()
        .expect("rwlock should remain usable after write");
    assert_eq!(*read, 9);
}

#[test]
fn sync_facade_once_lock_initializes_once() {
    let _guard = lock_fusion_std_tests();

    let cell = OnceLock::new();
    let value = match cell.get_or_init(|| 42_u32) {
        Ok(value) => value,
        Err(error) => {
            assert_eq!(error.kind, SyncErrorKind::Unsupported);
            return;
        }
    };
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
    let _guard = lock_fusion_std_tests();

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
    match spin.try_lock() {
        Ok(true) => unsafe { spin.unlock_unchecked() },
        Ok(false) => {}
        Err(error) => {
            assert_eq!(error.kind, SyncErrorKind::Unsupported);
            return;
        }
    }

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
    let _guard = lock_fusion_std_tests();

    let support = ThreadPool::support();
    let system_support = fusion_sys::thread::ThreadSystem::new().support();
    assert_eq!(support, system_support);

    let pool = match ThreadPool::new(&ThreadPoolConfig::new()) {
        Ok(pool) => pool,
        Err(error) => {
            assert_eq!(error.kind(), ThreadErrorKind::Unsupported);
            return;
        }
    };
    let clone = pool
        .try_clone()
        .expect("pool handle should retain shared state");

    let completed = Arc::new(AtomicU32::new(0));
    for _ in 0..3 {
        let completed = Arc::clone(&completed);
        pool.submit(move || {
            completed.fetch_add(1, Ordering::AcqRel);
        })
        .expect("pool should accept submitted work");
    }
    for _ in 0..3 {
        let completed = Arc::clone(&completed);
        clone
            .submit(move || {
                completed.fetch_add(1, Ordering::AcqRel);
            })
            .expect("cloned pool handle should submit work");
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
    let _guard = lock_fusion_std_tests();

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
fn executor_current_thread_and_pool_paths_are_real() {
    let _guard = lock_fusion_std_tests();

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

    let carrier = match ThreadPool::new(&ThreadPoolConfig::new()) {
        Ok(pool) => pool,
        Err(error) => {
            assert_eq!(error.kind(), ThreadErrorKind::Unsupported);
            return;
        }
    };

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

    carrier
        .shutdown()
        .expect("carrier pool should shut down cleanly");
}
