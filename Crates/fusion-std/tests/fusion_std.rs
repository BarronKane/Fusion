use fusion_std::sync::{Mutex, OnceLock, RwLock};
use fusion_std::thread::{
    DeterministicConstraints, EventInterest, EventNotification, EventReadiness, EventRecord,
    EventSourceHandle, Executor, ExecutorConfig, ExecutorError, ExecutorMode, GreenPool, Runtime,
    RuntimeConfig, RuntimeError, RuntimeProfile, ThreadPool, ThreadPoolConfig,
};
use fusion_sys::fiber::FiberSystem;
use fusion_sys::thread::ThreadErrorKind;
#[cfg(target_os = "linux")]
use std::io::Write;
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
#[cfg(target_os = "linux")]
use std::os::unix::net::UnixStream;
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
fn thread_pool_facade_reports_lower_level_support_honestly() {
    let pool = ThreadPool::new(&ThreadPoolConfig::new());
    assert_eq!(
        pool.expect_err("thread pool remains unsupported").kind(),
        ThreadErrorKind::Unsupported
    );

    let support = ThreadPool::support();
    let system_support = fusion_sys::thread::ThreadSystem::new().support();
    assert_eq!(support, system_support);
}

#[test]
fn executor_and_runtime_facades_are_honest_stubs() {
    let executor = Executor::new(ExecutorConfig::new());
    assert_eq!(executor.mode(), ExecutorMode::CurrentThread);
    assert!(matches!(
        executor.spawn(async { 5_u8 }),
        Err(ExecutorError::Unsupported)
    ));
    assert!(matches!(
        executor.spawn_local(async { 7_u8 }),
        Err(ExecutorError::Unsupported)
    ));

    assert_eq!(GreenPool::support(), FiberSystem::new().support());

    let runtime = Runtime::new(&RuntimeConfig {
        profile: RuntimeProfile::Deterministic,
        thread_pool: ThreadPoolConfig::new(),
        green: None,
        executor: ExecutorConfig::new(),
        deterministic: Some(DeterministicConstraints::strict()),
        elastic: None,
    });
    assert!(matches!(runtime, Err(RuntimeError::Unsupported)));
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
