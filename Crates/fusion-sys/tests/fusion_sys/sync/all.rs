use core::sync::atomic::{
    AtomicU32,
    Ordering,
};

use fusion_sys::sync::{
    Mutex,
    MutexCaps,
    Once,
    OnceLock,
    RawMutex,
    RwLock,
    Semaphore,
    SpinMutex,
    SyncError,
    SyncErrorKind,
    SyncFallbackKind,
    SyncImplementationKind,
    ThinMutex,
};

extern crate std;
use self::std::sync::Arc;
use self::std::thread;

#[test]
fn sync_types_are_send_and_sync_where_expected() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<SpinMutex>();
    assert_send_sync::<ThinMutex>();
    assert_send_sync::<Mutex<u32>>();
    assert_send_sync::<Once>();
    assert_send_sync::<OnceLock<u32>>();
    assert_send_sync::<RwLock<u32>>();
}

#[test]
fn spin_mutex_reports_spin_only_support_and_try_lock_behavior() {
    let lock = SpinMutex::new();
    let support = lock.support();

    assert_eq!(support.implementation, SyncImplementationKind::Emulated);
    assert_eq!(support.fallback, SyncFallbackKind::SpinOnly);
    assert!(support.caps.contains(MutexCaps::TRY_LOCK));

    assert!(lock.try_lock().expect("first try_lock should succeed"));
    assert!(
        !lock
            .try_lock()
            .expect("second try_lock should fail while held")
    );
    // SAFETY: the current thread still holds the first successful spin lock acquisition.
    unsafe { lock.unlock_unchecked() };
}

#[test]
fn thin_mutex_reports_a_usable_backend_and_serializes_access() {
    let lock = Arc::new(ThinMutex::new());
    let counter = Arc::new(AtomicU32::new(0));
    let support = lock.support();

    assert_ne!(support.implementation, SyncImplementationKind::Unsupported);

    let mut threads = self::std::vec::Vec::new();
    for _ in 0..4 {
        let lock = Arc::clone(&lock);
        let counter = Arc::clone(&counter);
        threads.push(thread::spawn(move || {
            for _ in 0..200 {
                let _guard = lock.lock().expect("thin mutex should lock");
                counter.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }

    for thread in threads {
        thread.join().expect("worker should finish");
    }

    assert_eq!(counter.load(Ordering::Relaxed), 800);
}

#[test]
fn mutex_public_api_protects_shared_data() {
    let value = Arc::new(Mutex::new(0_u32));
    let mut threads = self::std::vec::Vec::new();

    for _ in 0..4 {
        let value = Arc::clone(&value);
        threads.push(thread::spawn(move || {
            for _ in 0..200 {
                let mut guard = value.lock().expect("mutex should lock");
                *guard += 1;
            }
        }));
    }

    for thread in threads {
        thread.join().expect("worker should finish");
    }

    assert_eq!(*value.lock().expect("mutex should lock"), 800);
}

#[test]
fn once_runs_once_or_fails_honestly_when_unsupported() {
    let once = Arc::new(Once::new());

    if once.support().implementation == SyncImplementationKind::Unsupported {
        assert_eq!(once.call_once(|| {}), Err(SyncError::unsupported()));
        return;
    }

    let runs = Arc::new(AtomicU32::new(0));
    let mut threads = self::std::vec::Vec::new();
    for _ in 0..6 {
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
    assert!(once.is_completed());
}

#[test]
fn once_lock_initializes_once_or_reports_unsupported() {
    let cell = Arc::new(OnceLock::new());

    if Once::new().support().implementation == SyncImplementationKind::Unsupported {
        assert_eq!(cell.get_or_init(|| 7_u32), Err(SyncError::unsupported()));
        return;
    }

    let attempts = Arc::new(AtomicU32::new(0));
    {
        let cell = Arc::clone(&cell);
        let attempts = Arc::clone(&attempts);
        thread::spawn(move || {
            let value = cell
                .get_or_init(|| {
                    attempts.fetch_add(1, Ordering::AcqRel);
                    99_u32
                })
                .expect("once lock should initialize");
            assert_eq!(*value, 99);
        })
        .join()
        .expect("initializer thread should finish");
    }

    let mut threads = self::std::vec::Vec::new();
    for _ in 0..5 {
        let cell = Arc::clone(&cell);
        threads.push(thread::spawn(move || {
            let value = cell
                .get_or_init(|| 7_u32)
                .expect("once lock should already be initialized");
            assert_eq!(*value, 99);
        }));
    }

    for thread in threads {
        thread.join().expect("reader thread should finish");
    }

    assert_eq!(attempts.load(Ordering::Acquire), 1);
    assert_eq!(cell.get(), Some(&99_u32));
}

#[test]
fn rwlock_supports_readers_and_writers_or_reports_unsupported() {
    let lock = Arc::new(RwLock::new(5_u32));

    if lock.support().implementation == SyncImplementationKind::Unsupported {
        assert_eq!(
            lock.read().map(|_| ()).map_err(|error| error.kind),
            Err(SyncErrorKind::Unsupported)
        );
        assert_eq!(
            lock.write().map(|_| ()).map_err(|error| error.kind),
            Err(SyncErrorKind::Unsupported)
        );
        return;
    }

    let first = lock.read().expect("first read lock should succeed");
    let second = lock.read().expect("second read lock should also succeed");
    assert_eq!(*first, 5);
    assert_eq!(*second, 5);
    drop(first);
    drop(second);

    {
        let mut writer = lock.write().expect("write lock should succeed");
        *writer = 11;
    }

    assert_eq!(*lock.read().expect("read lock should succeed"), 11);
    assert!(
        lock.try_write()
            .expect("try_write should evaluate")
            .is_some()
    );
}

#[test]
fn semaphore_supports_permit_accounting_or_reports_unsupported() {
    let semaphore = Semaphore::new(1, 2);

    match semaphore {
        Ok(semaphore) => {
            assert_ne!(
                semaphore.support().implementation,
                SyncImplementationKind::Unsupported
            );
            assert_eq!(semaphore.max_permits(), 2);
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
            assert!(semaphore.try_acquire().expect("reacquire should evaluate"));
        }
        Err(error) => assert_eq!(error, SyncError::unsupported()),
    }
}
