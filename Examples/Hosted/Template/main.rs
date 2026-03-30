use fusion_std::sync::{Mutex, OnceLock, RwLock, Semaphore};
use fusion_std::thread::{
    Executor,
    ExecutorConfig,
    ExecutorMode,
    GreenPool,
    GreenPoolConfig,
    JoinSet,
    ThreadPool,
    ThreadPoolConfig,
};

const TEMPLATE_ASYNC_POLL_STACK_BYTES: usize = 2048;

fn main() {
    // Sync primitives.
    let mutex = Mutex::new(42u32);
    let rwlock = RwLock::new(100u64);
    let once: OnceLock<u32> = OnceLock::new();
    let sem = Semaphore::new(0, 4);

    if let Ok(guard) = mutex.lock() {
        core::hint::black_box(*guard);
    }
    if let Ok(guard) = rwlock.read() {
        core::hint::black_box(*guard);
    }
    if let Ok(guard) = rwlock.write() {
        core::hint::black_box(*guard);
    }
    let _ = once.get_or_try_init(|| Ok::<u32, ()>(99));
    core::hint::black_box(&sem);

    // Thread pool.
    let pool = ThreadPool::new(&ThreadPoolConfig::new());
    if let Ok(ref pool) = pool {
        let _ = pool.submit(|| {
            core::hint::black_box(123);
        });
    }

    // Green pool (requires a thread pool as carrier).
    let green = pool
        .as_ref()
        .ok()
        .and_then(|pool| GreenPool::new(&GreenPoolConfig::new(), pool).ok());
    if let Some(ref green) = green {
        let handle = green.spawn(|| 42u32);
        if let Ok(handle) = handle {
            let _ = core::hint::black_box(handle.join());
        }
    }

    // Executor — current thread.
    let executor = Executor::new(ExecutorConfig {
        mode: ExecutorMode::CurrentThread,
        ..ExecutorConfig::new()
    });
    let handle =
        executor.spawn_with_poll_stack_bytes(TEMPLATE_ASYNC_POLL_STACK_BYTES, async { 5u32 });
    if let Ok(handle) = handle {
        let _ = core::hint::black_box(handle.join());
    }

    // Executor — thread pool.
    if let Ok(ref pool) = pool {
        let executor = Executor::new(ExecutorConfig {
            mode: ExecutorMode::ThreadPool,
            ..ExecutorConfig::new()
        });
        let executor = executor.on_pool(pool);
        if let Ok(ref executor) = executor {
            let handle = executor
                .spawn_with_poll_stack_bytes(TEMPLATE_ASYNC_POLL_STACK_BYTES, async { 10u32 });
            if let Ok(handle) = handle {
                let _ = core::hint::black_box(handle.join());
            }
        }
    }

    // Executor — green pool.
    if let Some(ref green) = green {
        let executor = Executor::new(ExecutorConfig {
            mode: ExecutorMode::GreenPool,
            ..ExecutorConfig::new()
        });
        let executor = executor.on_green(green);
        if let Ok(ref executor) = executor {
            let handle = executor
                .spawn_with_poll_stack_bytes(TEMPLATE_ASYNC_POLL_STACK_BYTES, async { 15u32 });
            if let Ok(handle) = handle {
                let _ = core::hint::black_box(handle.join());
            }
        }
    }

    // JoinSet.
    let join_set: JoinSet<u32> = JoinSet::new();
    let executor = Executor::new(ExecutorConfig {
        mode: ExecutorMode::CurrentThread,
        ..ExecutorConfig::new()
    });
    let _ =
        join_set.spawn_with_poll_stack_bytes(&executor, TEMPLATE_ASYNC_POLL_STACK_BYTES, async {
            1u32
        });
    let _ =
        join_set.spawn_with_poll_stack_bytes(&executor, TEMPLATE_ASYNC_POLL_STACK_BYTES, async {
            2u32
        });
    let _ =
        join_set.spawn_with_poll_stack_bytes(&executor, TEMPLATE_ASYNC_POLL_STACK_BYTES, async {
            3u32
        });
    while let Ok(val) = join_set.join_next() {
        core::hint::black_box(val);
    }
}
