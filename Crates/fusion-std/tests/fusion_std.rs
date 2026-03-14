use fusion_std::sync::{Mutex, OnceLock, RwLock};
use fusion_std::thread::{
    DeterministicConstraints, Executor, ExecutorConfig, ExecutorError, ExecutorMode, GreenPool,
    Runtime, RuntimeConfig, RuntimeError, RuntimeProfile, ThreadPool, ThreadPoolConfig,
};
use fusion_sys::fiber::FiberSystem;
use fusion_sys::thread::ThreadErrorKind;

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
        *cell.get_or_init(|| 99_u32)
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
