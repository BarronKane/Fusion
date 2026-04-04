#![allow(dead_code, private_interfaces)]

use core::future::Future;
use core::mem::size_of;
use core::num::NonZeroUsize;
use core::pin::Pin;
use core::ptr::NonNull;
use core::sync::atomic::{
    AtomicUsize,
    Ordering,
};
use core::task::{
    Context,
    Poll,
    Waker,
};

use std::boxed::Box;
use std::hint::black_box;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{
    SyncSender,
    sync_channel,
};
use std::sync::{
    Arc,
    Mutex,
};
use std::task::Wake;
use std::thread::{
    self,
    JoinHandle,
};
use std::vec;

use test::Bencher;
use tokio::runtime::Builder as TokioRuntimeBuilder;

use fusion_std::thread::{
    CurrentAsyncRuntime,
    CurrentFiberPool,
    EventInterest,
    EventKey,
    EventNotification,
    EventReadiness,
    EventRecord,
    EventSourceHandle,
    ExecutorConfig,
    FiberPoolBootstrap,
    FiberPoolConfig,
    FiberStackClass,
    FiberTaskAttributes,
    FiberTaskExecution,
    GreenPool,
    GreenReactorPolicy,
    HostedFiberRuntime,
    HostedFiberRuntimeConfig,
    PoolPlacement,
    Reactor,
    ThreadAsyncRuntime,
    ThreadPool,
    ThreadPoolConfig,
    async_yield_now,
    yield_now as green_yield_now,
};
use fusion_sys::fiber::{
    Fiber,
    FiberReturn,
    FiberStack,
    FiberYield,
    yield_now as fiber_yield_now,
};
use fusion_sys::sync::Semaphore;

const LOW_LEVEL_STACK_WORDS: usize = 4096;
const BENCH_POOL_STACK_BYTES: usize = 16 * 1024;
const BENCH_POOL_CAPACITY: usize = 64;
const BENCH_LIFECYCLE_GROWTH_TOTAL: usize = THROUGHPUT_BATCH_SIZE;
const OVERRIDE_STACK_BYTES: usize = 512;
const THROUGHPUT_BATCH_SIZE: usize = 16;
const THROUGHPUT_BATCH_SEMAPHORE_MAX: u32 = THROUGHPUT_BATCH_SIZE as u32;
const MULTI_YIELD_COUNT: usize = 10;
const ASYNC_CONTENTION_TASKS: usize = 32;
const ASYNC_CONTENTION_YIELDS: usize = 32;
const BENCH_ASYNC_POLL_STACK_BYTES: usize = 2048;
const REACTOR_BATCH_SMALL: usize = 16;
const REACTOR_BATCH_LARGE: usize = 64;
const INLINE_NO_YIELD_BENCH_TASK: FiberTaskAttributes =
    FiberTaskAttributes::new(FiberStackClass::MIN)
        .with_execution(FiberTaskExecution::InlineNoYield);

const EMPTY_EVENT_RECORD: EventRecord = EventRecord {
    key: EventKey(0),
    notification: EventNotification::Readiness(EventReadiness::empty()),
};

struct LowLevelYieldingFiber {
    _stack_words: Box<[u128]>,
    progress: Box<AtomicUsize>,
    fiber: Fiber,
}

#[derive(Debug)]
struct BenchPipe {
    read_fd: i32,
    write_fd: i32,
}

impl BenchPipe {
    fn new() -> Self {
        let mut fds = [0_i32; 2];
        let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) };
        assert_eq!(rc, 0, "benchmark pipe should create");
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
        assert_eq!(rc, 1, "benchmark pipe should become readable");
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
            assert_eq!(
                rc, -1,
                "benchmark pipe read should either succeed or set errno"
            );
            let errno = unsafe { *libc::__errno_location() };
            if errno == libc::EINTR {
                continue;
            }
            panic!("benchmark pipe should drain after readiness, errno={errno}");
        }
    }
}

impl Drop for BenchPipe {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.read_fd);
            libc::close(self.write_fd);
        }
    }
}

#[derive(Debug, Default)]
struct CrossThreadWakeState {
    ready: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

impl CrossThreadWakeState {
    fn signal(&self) {
        self.ready.store(true, Ordering::Release);
        let waker = self.waker.lock().expect("wake state should lock").take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

struct CrossThreadWakeFuture {
    state: Arc<CrossThreadWakeState>,
}

impl Future for CrossThreadWakeFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.state.ready.load(Ordering::Acquire) {
            return Poll::Ready(());
        }
        {
            let mut registered = self.state.waker.lock().expect("wake state should lock");
            if registered
                .as_ref()
                .is_none_or(|waker| !waker.will_wake(cx.waker()))
            {
                *registered = Some(cx.waker().clone());
            }
        }
        if self.state.ready.load(Ordering::Acquire) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

impl LowLevelYieldingFiber {
    fn new() -> Self {
        let mut stack_words = vec![0_u128; LOW_LEVEL_STACK_WORDS].into_boxed_slice();
        let progress = Box::new(AtomicUsize::new(0));
        let stack = FiberStack::new(
            // SAFETY: the boxed slice remains alive for the lifetime of the fiber.
            unsafe { NonNull::new_unchecked(stack_words.as_mut_ptr().cast()) },
            stack_words.len() * size_of::<u128>(),
        )
        .expect("benchmark stack should be valid");
        let progress_ptr = core::ptr::from_ref(Box::as_ref(&progress))
            .cast_mut()
            .cast();
        let fiber = Fiber::new(stack, low_level_yield_loop, progress_ptr)
            .expect("host backend should support low-level fibers for this benchmark");
        Self {
            _stack_words: stack_words,
            progress,
            fiber,
        }
    }

    fn resume_yielded(&mut self) {
        match self
            .fiber
            .resume()
            .expect("benchmark fiber should resume cleanly")
        {
            FiberYield::Yielded => {}
            FiberYield::Completed(_) => {
                panic!("benchmark yield fiber completed unexpectedly");
            }
        }
    }
}

unsafe fn low_level_yield_loop(context: *mut ()) -> FiberReturn {
    let progress = unsafe { &*context.cast::<AtomicUsize>() };
    loop {
        progress.fetch_add(1, Ordering::Relaxed);
        fiber_yield_now().expect("benchmark fiber should yield back to caller");
    }
}

#[derive(Debug)]
struct BenchThreadNotify {
    thread: thread::Thread,
    notified: AtomicBool,
}

impl Wake for BenchThreadNotify {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.notified.store(true, Ordering::Release);
        self.thread.unpark();
    }
}

fn bench_block_on<F>(future: F) -> F::Output
where
    F: Future,
{
    let notify = Arc::new(BenchThreadNotify {
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

pub fn current_pool() -> CurrentFiberPool {
    CurrentFiberPool::new(&bench_pool_config())
        .expect("host backend should support a current-thread fiber pool")
}

pub fn green_pool() -> (ThreadPool, GreenPool) {
    green_pool_with_carriers(1)
}

pub fn green_pool_with_carriers(carrier_count: usize) -> (ThreadPool, GreenPool) {
    let carriers = ThreadPool::new(&ThreadPoolConfig {
        min_threads: carrier_count,
        max_threads: carrier_count,
        ..ThreadPoolConfig::new()
    })
    .expect("carrier pool should build for benches");
    let fibers = GreenPool::new(&bench_pool_config(), &carriers)
        .expect("green pool should build for benches");
    (carriers, fibers)
}

pub fn green_pool_lifecycle_with_carriers(carrier_count: usize) -> HostedFiberRuntime {
    let per_carrier_growth = BENCH_LIFECYCLE_GROWTH_TOTAL.div_ceil(carrier_count).max(1);
    FiberPoolBootstrap::fixed_growing_with_stack(
        NonZeroUsize::new(BENCH_POOL_STACK_BYTES)
            .expect("benchmark fixed stack size should be non-zero"),
        THROUGHPUT_BATCH_SIZE,
        per_carrier_growth,
    )
    .expect("lifecycle bench fixed-growing config should build")
    .build_hosted_with(
        HostedFiberRuntimeConfig::new(carrier_count).with_placement(PoolPlacement::Inherit),
    )
    .expect("hosted fiber runtime should build for lifecycle benches")
}

pub fn green_pool_bootstrap_with_warm_carriers(carrier_count: usize) -> ThreadPool {
    ThreadPool::new(&ThreadPoolConfig {
        min_threads: carrier_count,
        max_threads: carrier_count,
        ..ThreadPoolConfig::new()
    })
    .expect("carrier pool should build for green bootstrap benches")
}

pub fn thread_async_runtime(worker_count: usize) -> ThreadAsyncRuntime {
    ThreadAsyncRuntime::with_executor_config(
        &ThreadPoolConfig {
            min_threads: worker_count,
            max_threads: worker_count,
            ..ThreadPoolConfig::new()
        },
        ExecutorConfig::thread_pool().with_capacity(THROUGHPUT_BATCH_SIZE),
    )
    .expect("thread async runtime should build for benches")
}

const fn bench_pool_config() -> FiberPoolConfig<'static> {
    FiberPoolConfig::fixed(
        NonZeroUsize::new(BENCH_POOL_STACK_BYTES)
            .expect("benchmark fixed stack size should be non-zero"),
        BENCH_POOL_CAPACITY,
    )
}

pub fn baseline_direct_noop(b: &mut Bencher) {
    fn noop() -> usize {
        7
    }

    b.iter(|| black_box(noop()));
}

pub fn fiber_low_level_create(b: &mut Bencher) {
    let mut stack_words = vec![0_u128; LOW_LEVEL_STACK_WORDS].into_boxed_slice();
    let progress = AtomicUsize::new(0);
    let progress_ptr = (&raw const progress).cast_mut().cast();

    b.iter(|| {
        let stack = FiberStack::new(
            // SAFETY: the backing allocation stays alive across every iteration.
            unsafe { NonNull::new_unchecked(stack_words.as_mut_ptr().cast()) },
            stack_words.len() * size_of::<u128>(),
        )
        .expect("benchmark stack should be valid");
        let fiber = Fiber::new(stack, low_level_yield_loop, progress_ptr)
            .expect("host backend should support low-level fibers for this benchmark");
        black_box(fiber.state());
    });
}

pub fn fiber_low_level_resume_yield_round_trip(b: &mut Bencher) {
    let mut fiber = LowLevelYieldingFiber::new();

    b.iter(|| fiber.resume_yielded());

    black_box(fiber.progress.load(Ordering::Acquire));
}

pub fn current_fiber_pool_spawn_join_noop(b: &mut Bencher) {
    let fibers = current_pool();
    let (): () = fibers
        .spawn_with_stack::<BENCH_POOL_STACK_BYTES, _, _>(noop_job)
        .expect("warmup task should spawn")
        .join()
        .expect("warmup task should join");
    black_box(());

    b.iter(|| {
        let handle = fibers
            .spawn_with_stack::<BENCH_POOL_STACK_BYTES, _, _>(noop_job)
            .expect("benchmark task should spawn");
        let (): () = handle.join().expect("benchmark task should join");
        black_box(());
    });

    fibers.shutdown().expect("benchmark pool should shut down");
}

pub fn current_fiber_pool_spawn_with_stack_join_noop(b: &mut Bencher) {
    let fibers = current_pool();
    let (): () = fibers
        .spawn_with_stack::<OVERRIDE_STACK_BYTES, _, _>(noop_job)
        .expect("warmup task should spawn")
        .join()
        .expect("warmup task should join");
    black_box(());

    b.iter(|| {
        let handle = fibers
            .spawn_with_stack::<OVERRIDE_STACK_BYTES, _, _>(noop_job)
            .expect("benchmark task should spawn");
        let (): () = handle.join().expect("benchmark task should join");
        black_box(());
    });

    fibers.shutdown().expect("benchmark pool should shut down");
}

pub fn current_fiber_pool_spawn_join_yield_once(b: &mut Bencher) {
    let fibers = current_pool();
    let (): () = fibers
        .spawn_with_stack::<BENCH_POOL_STACK_BYTES, _, _>(yield_once_job)
        .expect("warmup task should spawn")
        .join()
        .expect("warmup task should join");
    black_box(());

    b.iter(|| {
        let handle = fibers
            .spawn_with_stack::<BENCH_POOL_STACK_BYTES, _, _>(yield_once_job)
            .expect("benchmark task should spawn");
        let (): () = handle.join().expect("benchmark task should join");
        black_box(());
    });

    fibers.shutdown().expect("benchmark pool should shut down");
}

pub fn current_async_runtime_spawn_join_noop(b: &mut Bencher) {
    let runtime = CurrentAsyncRuntime::new();
    let (): () = runtime
        .spawn_with_poll_stack_bytes(BENCH_ASYNC_POLL_STACK_BYTES, async_noop_job())
        .expect("warmup task should spawn")
        .join()
        .expect("warmup task should join");
    black_box(());

    b.iter(|| {
        let handle = runtime
            .spawn_with_poll_stack_bytes(BENCH_ASYNC_POLL_STACK_BYTES, async_noop_job())
            .expect("benchmark task should spawn");
        let (): () = handle.join().expect("benchmark task should join");
        black_box(());
    });
}

pub fn current_async_runtime_spawn_join_yield_once(b: &mut Bencher) {
    let runtime = CurrentAsyncRuntime::new();
    let (): () = runtime
        .spawn_with_poll_stack_bytes(BENCH_ASYNC_POLL_STACK_BYTES, async_yield_once_job())
        .expect("warmup task should spawn")
        .join()
        .expect("warmup task should join");
    black_box(());

    b.iter(|| {
        let handle = runtime
            .spawn_with_poll_stack_bytes(BENCH_ASYNC_POLL_STACK_BYTES, async_yield_once_job())
            .expect("benchmark task should spawn");
        let (): () = handle.join().expect("benchmark task should join");
        black_box(());
    });
}

pub fn current_async_runtime_cross_thread_wake_once(b: &mut Bencher) {
    let runtime = CurrentAsyncRuntime::new();
    let (tx, worker) = cross_thread_wake_worker();
    cross_thread_wake_round_fusion(&runtime, &tx);

    b.iter(|| cross_thread_wake_round_fusion(&runtime, &tx));

    drop(tx);
    worker.join().expect("wake worker should shut down cleanly");
}

pub fn green_pool_spawn_join_noop(b: &mut Bencher) {
    let (_carriers, fibers) = green_pool();
    let (): () = fibers
        .spawn_with_attrs(INLINE_NO_YIELD_BENCH_TASK, noop_job)
        .expect("warmup task should spawn")
        .join()
        .expect("warmup task should join");
    black_box(());

    b.iter(|| {
        let handle = fibers
            .spawn_with_attrs(INLINE_NO_YIELD_BENCH_TASK, noop_job)
            .expect("benchmark task should spawn");
        let (): () = handle.join().expect("benchmark task should join");
        black_box(());
    });

    fibers.shutdown().expect("benchmark pool should shut down");
}

pub fn green_pool_spawn_with_stack_join_noop(b: &mut Bencher) {
    let (_carriers, fibers) = green_pool();
    let (): () = fibers
        .spawn_with_stack::<OVERRIDE_STACK_BYTES, _, _>(noop_job)
        .expect("warmup task should spawn")
        .join()
        .expect("warmup task should join");
    black_box(());

    b.iter(|| {
        let handle = fibers
            .spawn_with_stack::<OVERRIDE_STACK_BYTES, _, _>(noop_job)
            .expect("benchmark task should spawn");
        let (): () = handle.join().expect("benchmark task should join");
        black_box(());
    });

    fibers.shutdown().expect("benchmark pool should shut down");
}

pub fn green_pool_spawn_join_yield_once(b: &mut Bencher) {
    let (_carriers, fibers) = green_pool();
    let (): () = fibers
        .spawn_with_stack::<BENCH_POOL_STACK_BYTES, _, _>(yield_once_job)
        .expect("warmup task should spawn")
        .join()
        .expect("warmup task should join");
    black_box(());

    b.iter(|| {
        let handle = fibers
            .spawn_with_stack::<BENCH_POOL_STACK_BYTES, _, _>(yield_once_job)
            .expect("benchmark task should spawn");
        let (): () = handle.join().expect("benchmark task should join");
        black_box(());
    });

    fibers.shutdown().expect("benchmark pool should shut down");
}

pub fn green_pool_throughput_noop_carriers_1(b: &mut Bencher) {
    bench_green_pool_steady_state_inline_noop(b, 1);
}

pub fn green_pool_throughput_noop_carriers_2(b: &mut Bencher) {
    bench_green_pool_steady_state_inline_noop(b, 2);
}

pub fn green_pool_throughput_noop_carriers_4(b: &mut Bencher) {
    bench_green_pool_steady_state_inline_noop(b, 4);
}

pub fn green_pool_throughput_yield_once_carriers_1(b: &mut Bencher) {
    bench_green_pool_steady_state_throughput(b, 1, yield_once_job);
}

pub fn green_pool_throughput_yield_once_carriers_2(b: &mut Bencher) {
    bench_green_pool_steady_state_throughput(b, 2, yield_once_job);
}

pub fn green_pool_throughput_yield_once_carriers_4(b: &mut Bencher) {
    bench_green_pool_steady_state_throughput(b, 4, yield_once_job);
}

pub fn green_pool_lifecycle_noop_carriers_1(b: &mut Bencher) {
    bench_green_pool_lifecycle_inline_noop(b, 1);
}

pub fn green_pool_lifecycle_noop_carriers_2(b: &mut Bencher) {
    bench_green_pool_lifecycle_inline_noop(b, 2);
}

pub fn green_pool_lifecycle_noop_carriers_4(b: &mut Bencher) {
    bench_green_pool_lifecycle_inline_noop(b, 4);
}

pub fn green_pool_lifecycle_yield_once_carriers_1(b: &mut Bencher) {
    bench_green_pool_lifecycle_throughput(b, 1, yield_once_job);
}

pub fn green_pool_lifecycle_yield_once_carriers_2(b: &mut Bencher) {
    bench_green_pool_lifecycle_throughput(b, 2, yield_once_job);
}

pub fn green_pool_lifecycle_yield_once_carriers_4(b: &mut Bencher) {
    bench_green_pool_lifecycle_throughput(b, 4, yield_once_job);
}

pub fn thread_pool_lifecycle_only_workers_1(b: &mut Bencher) {
    bench_thread_pool_lifecycle_only(b, 1);
}

pub fn thread_pool_lifecycle_only_workers_2(b: &mut Bencher) {
    bench_thread_pool_lifecycle_only(b, 2);
}

pub fn thread_pool_lifecycle_only_workers_4(b: &mut Bencher) {
    bench_thread_pool_lifecycle_only(b, 4);
}

pub fn thread_pool_dispatch_round_trip_workers_1(b: &mut Bencher) {
    bench_thread_pool_dispatch_round_trip(b, 1);
}

pub fn thread_pool_dispatch_round_trip_workers_2(b: &mut Bencher) {
    bench_thread_pool_dispatch_round_trip(b, 2);
}

pub fn thread_pool_dispatch_round_trip_workers_4(b: &mut Bencher) {
    bench_thread_pool_dispatch_round_trip(b, 4);
}

pub fn thread_pool_throughput_noop_workers_1(b: &mut Bencher) {
    bench_thread_pool_steady_state_batch_noop(b, 1);
}

pub fn thread_pool_throughput_noop_workers_2(b: &mut Bencher) {
    bench_thread_pool_steady_state_batch_noop(b, 2);
}

pub fn thread_pool_throughput_noop_workers_4(b: &mut Bencher) {
    bench_thread_pool_steady_state_batch_noop(b, 4);
}

pub fn thread_pool_lifecycle_batch_noop_workers_1(b: &mut Bencher) {
    bench_thread_pool_lifecycle_batch_noop(b, 1);
}

pub fn thread_pool_lifecycle_batch_noop_workers_2(b: &mut Bencher) {
    bench_thread_pool_lifecycle_batch_noop(b, 2);
}

pub fn thread_pool_lifecycle_batch_noop_workers_4(b: &mut Bencher) {
    bench_thread_pool_lifecycle_batch_noop(b, 4);
}

pub fn green_pool_bootstrap_only_carriers_1(b: &mut Bencher) {
    bench_green_pool_bootstrap_only(b, 1);
}

pub fn green_pool_bootstrap_only_carriers_2(b: &mut Bencher) {
    bench_green_pool_bootstrap_only(b, 2);
}

pub fn green_pool_bootstrap_only_carriers_4(b: &mut Bencher) {
    bench_green_pool_bootstrap_only(b, 4);
}

pub fn thread_async_runtime_lifecycle_noop_workers_1(b: &mut Bencher) {
    bench_thread_async_runtime_lifecycle_throughput(b, 1, async_noop_job);
}

pub fn thread_async_runtime_lifecycle_noop_workers_2(b: &mut Bencher) {
    bench_thread_async_runtime_lifecycle_throughput(b, 2, async_noop_job);
}

pub fn thread_async_runtime_lifecycle_noop_workers_4(b: &mut Bencher) {
    bench_thread_async_runtime_lifecycle_throughput(b, 4, async_noop_job);
}

pub fn thread_async_runtime_lifecycle_yield_once_workers_1(b: &mut Bencher) {
    bench_thread_async_runtime_lifecycle_throughput(b, 1, async_yield_once_job);
}

pub fn thread_async_runtime_lifecycle_yield_once_workers_2(b: &mut Bencher) {
    bench_thread_async_runtime_lifecycle_throughput(b, 2, async_yield_once_job);
}

pub fn thread_async_runtime_lifecycle_yield_once_workers_4(b: &mut Bencher) {
    bench_thread_async_runtime_lifecycle_throughput(b, 4, async_yield_once_job);
}

pub fn tokio_current_thread_spawn_join_noop(b: &mut Bencher) {
    let runtime = TokioRuntimeBuilder::new_current_thread()
        .build()
        .expect("tokio current-thread runtime should build for benches");

    runtime.block_on(async {
        let handle = tokio::spawn(tokio_async_noop_job());
        handle.await.expect("warmup task should join");
    });

    b.iter(|| {
        runtime.block_on(async {
            let handle = tokio::spawn(tokio_async_noop_job());
            handle.await.expect("benchmark task should join");
            black_box(());
        });
    });
}

pub fn tokio_current_thread_spawn_join_yield_once(b: &mut Bencher) {
    let runtime = TokioRuntimeBuilder::new_current_thread()
        .build()
        .expect("tokio current-thread runtime should build for benches");

    runtime.block_on(async {
        let handle = tokio::spawn(tokio_async_yield_once_job());
        handle.await.expect("warmup task should join");
    });

    b.iter(|| {
        runtime.block_on(async {
            let handle = tokio::spawn(tokio_async_yield_once_job());
            handle.await.expect("benchmark task should join");
            black_box(());
        });
    });
}

pub fn tokio_current_thread_cross_thread_wake_once(b: &mut Bencher) {
    let runtime = TokioRuntimeBuilder::new_current_thread()
        .build()
        .expect("tokio current-thread runtime should build for benches");
    let (tx, worker) = cross_thread_wake_worker();
    cross_thread_wake_round_tokio_current(&runtime, &tx);

    b.iter(|| cross_thread_wake_round_tokio_current(&runtime, &tx));

    drop(tx);
    worker.join().expect("wake worker should shut down cleanly");
}

pub fn tokio_multi_thread_lifecycle_noop_workers_1(b: &mut Bencher) {
    bench_tokio_multi_thread_lifecycle_throughput(b, 1, tokio_async_noop_job);
}

pub fn tokio_multi_thread_lifecycle_noop_workers_2(b: &mut Bencher) {
    bench_tokio_multi_thread_lifecycle_throughput(b, 2, tokio_async_noop_job);
}

pub fn tokio_multi_thread_lifecycle_noop_workers_4(b: &mut Bencher) {
    bench_tokio_multi_thread_lifecycle_throughput(b, 4, tokio_async_noop_job);
}

pub fn tokio_multi_thread_lifecycle_yield_once_workers_1(b: &mut Bencher) {
    bench_tokio_multi_thread_lifecycle_throughput(b, 1, tokio_async_yield_once_job);
}

pub fn tokio_multi_thread_lifecycle_yield_once_workers_2(b: &mut Bencher) {
    bench_tokio_multi_thread_lifecycle_throughput(b, 2, tokio_async_yield_once_job);
}

pub fn tokio_multi_thread_lifecycle_yield_once_workers_4(b: &mut Bencher) {
    bench_tokio_multi_thread_lifecycle_throughput(b, 4, tokio_async_yield_once_job);
}

pub fn current_fiber_pool_spawn_join_yield_ten_local_state(b: &mut Bencher) {
    let fibers = current_pool();
    let (): () = fibers
        .spawn_with_stack::<BENCH_POOL_STACK_BYTES, _, _>(yield_ten_local_state_job)
        .expect("warmup task should spawn")
        .join()
        .expect("warmup task should join");
    black_box(());

    b.iter(|| {
        let handle = fibers
            .spawn_with_stack::<BENCH_POOL_STACK_BYTES, _, _>(yield_ten_local_state_job)
            .expect("benchmark task should spawn");
        let (): () = handle.join().expect("benchmark task should join");
        black_box(());
    });

    fibers.shutdown().expect("benchmark pool should shut down");
}

pub fn current_fiber_pool_spawn_join_recursive_stack(b: &mut Bencher) {
    let fibers = current_pool();
    let _: usize = fibers
        .spawn_with_stack::<BENCH_POOL_STACK_BYTES, _, _>(recursive_stack_job)
        .expect("warmup task should spawn")
        .join()
        .expect("warmup task should join");

    b.iter(|| {
        let handle = fibers
            .spawn_with_stack::<BENCH_POOL_STACK_BYTES, _, _>(recursive_stack_job)
            .expect("benchmark task should spawn");
        let depth = handle.join().expect("benchmark task should join");
        black_box(depth);
    });

    fibers.shutdown().expect("benchmark pool should shut down");
}

pub fn current_async_runtime_contention_yield_32x32(b: &mut Bencher) {
    let runtime = CurrentAsyncRuntime::new();
    current_async_contention_round(&runtime);

    b.iter(|| current_async_contention_round(&runtime));
}

pub fn tokio_current_thread_contention_yield_32x32(b: &mut Bencher) {
    let runtime = TokioRuntimeBuilder::new_current_thread()
        .build()
        .expect("tokio current-thread runtime should build for benches");
    tokio_current_contention_round(&runtime);

    b.iter(|| tokio_current_contention_round(&runtime));
}

pub fn reactor_readiness_batch_16(b: &mut Bencher) {
    bench_reactor_batch_ready(b, REACTOR_BATCH_SMALL);
}

pub fn reactor_readiness_batch_64(b: &mut Bencher) {
    bench_reactor_batch_ready(b, REACTOR_BATCH_LARGE);
}

const fn noop_job() {}

async fn async_noop_job() {
    core::future::ready(()).await;
}

async fn tokio_async_noop_job() {
    core::future::ready(()).await;
}

pub fn yield_once_job() {
    green_yield_now().expect("benchmark task should yield cleanly");
}

async fn async_yield_once_job() {
    async_yield_now().await;
}

async fn tokio_async_yield_once_job() {
    tokio::task::yield_now().await;
}

async fn async_contention_job() -> usize {
    let mut local = [0_u64; 16];
    let mut acc = 0usize;
    let mut round = 0usize;
    while round < ASYNC_CONTENTION_YIELDS {
        let mut index = 0usize;
        while index < local.len() {
            local[index] = local[index]
                .wrapping_add((round as u64).wrapping_mul(13))
                .wrapping_add(index as u64);
            index += 1;
        }
        acc ^= usize::try_from(local[round % local.len()])
            .expect("contention checksum lane should fit in usize");
        async_yield_now().await;
        round += 1;
    }
    acc
}

async fn tokio_async_contention_job() -> usize {
    let mut local = [0_u64; 16];
    let mut acc = 0usize;
    let mut round = 0usize;
    while round < ASYNC_CONTENTION_YIELDS {
        let mut index = 0usize;
        while index < local.len() {
            local[index] = local[index]
                .wrapping_add((round as u64).wrapping_mul(13))
                .wrapping_add(index as u64);
            index += 1;
        }
        acc ^= usize::try_from(local[round % local.len()])
            .expect("contention checksum lane should fit in usize");
        tokio::task::yield_now().await;
        round += 1;
    }
    acc
}

pub fn yield_ten_local_state_job() {
    let mut local = [0_u64; 128];
    let mut round = 0usize;
    while round < MULTI_YIELD_COUNT {
        let mut index = 0usize;
        while index < local.len() {
            local[index] = local[index]
                .wrapping_add((round as u64).wrapping_mul(17))
                .wrapping_add(index as u64);
            index += 1;
        }
        black_box(local[round % local.len()]);
        green_yield_now().expect("benchmark task should yield cleanly");
        round += 1;
    }
}

pub fn recursive_stack_job() -> usize {
    fn recurse(depth: usize) -> usize {
        let local = [u8::try_from(depth).expect("benchmark recursion depth should fit in u8"); 96];
        black_box(local[0]);
        if depth == 0 {
            0
        } else {
            recurse(depth - 1).saturating_add(1)
        }
    }

    recurse(32)
}

pub fn bench_green_pool_steady_state_throughput(b: &mut Bencher, carrier_count: usize, job: fn()) {
    let (carriers, fibers) = green_pool_with_carriers(carrier_count);
    let mut handles = Vec::with_capacity(THROUGHPUT_BATCH_SIZE);
    for _ in 0..THROUGHPUT_BATCH_SIZE {
        handles.push(
            fibers
                .spawn_with_stack::<BENCH_POOL_STACK_BYTES, _, _>(job)
                .expect("warmup throughput task should spawn successfully"),
        );
    }
    while let Some(handle) = handles.pop() {
        let (): () = handle
            .join()
            .expect("warmup throughput task should join cleanly");
    }
    black_box(());

    b.iter(|| {
        handles.clear();
        for _ in 0..THROUGHPUT_BATCH_SIZE {
            handles.push(
                fibers
                    .spawn_with_stack::<BENCH_POOL_STACK_BYTES, _, _>(job)
                    .expect("throughput task should spawn successfully"),
            );
        }
        while let Some(handle) = handles.pop() {
            let (): () = handle.join().expect("throughput task should join cleanly");
            black_box(());
        }
        black_box(());
    });

    fibers.shutdown().expect("benchmark pool should shut down");
    drop(fibers);
    carriers.shutdown().expect("carrier pool should shut down");
}

pub fn bench_green_pool_steady_state_inline_noop(b: &mut Bencher, carrier_count: usize) {
    let (carriers, fibers) = green_pool_with_carriers(carrier_count);
    let mut handles = Vec::with_capacity(THROUGHPUT_BATCH_SIZE);
    for _ in 0..THROUGHPUT_BATCH_SIZE {
        handles.push(
            fibers
                .spawn_with_attrs(INLINE_NO_YIELD_BENCH_TASK, noop_job)
                .expect("warmup throughput task should spawn successfully"),
        );
    }
    while let Some(handle) = handles.pop() {
        let (): () = handle
            .join()
            .expect("warmup throughput task should join cleanly");
    }
    black_box(());

    b.iter(|| {
        handles.clear();
        for _ in 0..THROUGHPUT_BATCH_SIZE {
            handles.push(
                fibers
                    .spawn_with_attrs(INLINE_NO_YIELD_BENCH_TASK, noop_job)
                    .expect("throughput task should spawn successfully"),
            );
        }
        while let Some(handle) = handles.pop() {
            let (): () = handle.join().expect("throughput task should join cleanly");
            black_box(());
        }
        black_box(());
    });

    fibers.shutdown().expect("benchmark pool should shut down");
    drop(fibers);
    carriers.shutdown().expect("carrier pool should shut down");
}

pub fn bench_green_pool_lifecycle_throughput(b: &mut Bencher, carrier_count: usize, job: fn()) {
    b.iter(|| {
        let runtime = green_pool_lifecycle_with_carriers(carrier_count);
        let fibers = runtime.fibers();
        let mut handles = Vec::with_capacity(THROUGHPUT_BATCH_SIZE);
        handles.clear();
        for _ in 0..THROUGHPUT_BATCH_SIZE {
            handles.push(
                fibers
                    .spawn_with_stack::<BENCH_POOL_STACK_BYTES, _, _>(job)
                    .expect("lifecycle throughput task should spawn successfully"),
            );
        }
        while let Some(handle) = handles.pop() {
            let (): () = handle
                .join()
                .expect("lifecycle throughput task should join cleanly");
            black_box(());
        }
        let (mut carriers, fibers) = runtime.into_parts();
        fibers.shutdown().expect("benchmark pool should shut down");
        carriers.shutdown().expect("carrier pool should shut down");
        black_box(());
    });
}

pub fn bench_green_pool_lifecycle_inline_noop(b: &mut Bencher, carrier_count: usize) {
    b.iter(|| {
        let runtime = green_pool_lifecycle_with_carriers(carrier_count);
        let fibers = runtime.fibers();
        let mut handles = Vec::with_capacity(THROUGHPUT_BATCH_SIZE);
        handles.clear();
        for _ in 0..THROUGHPUT_BATCH_SIZE {
            handles.push(
                fibers
                    .spawn_with_attrs(INLINE_NO_YIELD_BENCH_TASK, noop_job)
                    .expect("lifecycle throughput task should spawn successfully"),
            );
        }
        while let Some(handle) = handles.pop() {
            let (): () = handle
                .join()
                .expect("lifecycle throughput task should join cleanly");
            black_box(());
        }
        let (mut carriers, fibers) = runtime.into_parts();
        fibers.shutdown().expect("benchmark pool should shut down");
        carriers.shutdown().expect("carrier pool should shut down");
        black_box(());
    });
}

pub fn bench_thread_pool_lifecycle_only(b: &mut Bencher, worker_count: usize) {
    b.iter(|| {
        let carriers = ThreadPool::new(&ThreadPoolConfig {
            min_threads: worker_count,
            max_threads: worker_count,
            ..ThreadPoolConfig::new()
        })
        .expect("carrier pool should build for lifecycle-only benches");
        carriers
            .shutdown()
            .expect("carrier pool should shut down cleanly");
        black_box(());
    });
}

pub fn bench_thread_pool_dispatch_round_trip(b: &mut Bencher, worker_count: usize) {
    let carriers = ThreadPool::new(&ThreadPoolConfig {
        min_threads: worker_count,
        max_threads: worker_count,
        ..ThreadPoolConfig::new()
    })
    .expect("carrier pool should build for dispatch benches");
    let completed = Arc::new(Semaphore::new(0, 1).expect("dispatch semaphore should build"));

    b.iter(|| {
        while completed
            .try_acquire()
            .expect("dispatch semaphore should drain")
        {}
        let signal = Arc::clone(&completed);
        carriers
            .submit(move || {
                signal
                    .release(1)
                    .expect("dispatch completion should signal");
            })
            .expect("carrier dispatch should succeed");
        completed
            .acquire()
            .expect("dispatch completion should be observed");
        black_box(());
    });

    carriers
        .shutdown()
        .expect("carrier pool should shut down after dispatch bench");
}

pub fn drain_completion_semaphore(completed: &Semaphore) {
    while completed
        .try_acquire()
        .expect("batch completion semaphore should drain")
    {}
}

pub fn submit_thread_pool_noop_batch(carriers: &ThreadPool, completed: &Arc<Semaphore>) {
    for _ in 0..THROUGHPUT_BATCH_SIZE {
        let signal = Arc::clone(completed);
        carriers
            .submit(move || {
                signal.release(1).expect("batch completion should signal");
            })
            .expect("thread-pool batch submit should succeed");
    }
}

pub fn await_thread_pool_noop_batch(completed: &Semaphore) {
    for _ in 0..THROUGHPUT_BATCH_SIZE {
        completed
            .acquire()
            .expect("thread-pool batch completion should be observed");
    }
}

pub fn bench_thread_pool_steady_state_batch_noop(b: &mut Bencher, worker_count: usize) {
    let carriers = ThreadPool::new(&ThreadPoolConfig {
        min_threads: worker_count,
        max_threads: worker_count,
        ..ThreadPoolConfig::new()
    })
    .expect("carrier pool should build for throughput benches");
    let completed = Arc::new(
        Semaphore::new(0, THROUGHPUT_BATCH_SEMAPHORE_MAX)
            .expect("batch completion semaphore should build"),
    );

    submit_thread_pool_noop_batch(&carriers, &completed);
    await_thread_pool_noop_batch(&completed);
    black_box(());

    b.iter(|| {
        drain_completion_semaphore(&completed);
        submit_thread_pool_noop_batch(&carriers, &completed);
        await_thread_pool_noop_batch(&completed);
        black_box(());
    });

    carriers
        .shutdown()
        .expect("carrier pool should shut down after throughput bench");
}

pub fn bench_thread_pool_lifecycle_batch_noop(b: &mut Bencher, worker_count: usize) {
    b.iter(|| {
        let carriers = ThreadPool::new(&ThreadPoolConfig {
            min_threads: worker_count,
            max_threads: worker_count,
            ..ThreadPoolConfig::new()
        })
        .expect("carrier pool should build for lifecycle batch benches");
        let completed = Arc::new(
            Semaphore::new(0, THROUGHPUT_BATCH_SEMAPHORE_MAX)
                .expect("batch completion semaphore should build"),
        );
        submit_thread_pool_noop_batch(&carriers, &completed);
        await_thread_pool_noop_batch(&completed);
        carriers
            .shutdown()
            .expect("carrier pool should shut down cleanly");
        black_box(());
    });
}

pub fn bench_green_pool_bootstrap_only(b: &mut Bencher, carrier_count: usize) {
    let carriers = green_pool_bootstrap_with_warm_carriers(carrier_count);
    b.iter(|| {
        let per_carrier_growth = BENCH_LIFECYCLE_GROWTH_TOTAL.div_ceil(carrier_count).max(1);
        let config = FiberPoolConfig::fixed_growing(
            NonZeroUsize::new(BENCH_POOL_STACK_BYTES)
                .expect("benchmark fixed stack size should be non-zero"),
            THROUGHPUT_BATCH_SIZE,
            per_carrier_growth,
        )
        .expect("bootstrap-only fixed-growing config should build")
        .with_reactor_policy(GreenReactorPolicy::Disabled);
        let fibers = GreenPool::new(&config, &carriers)
            .expect("green pool should build for bootstrap-only benches");
        fibers
            .shutdown()
            .expect("green pool should shut down cleanly");
        drop(fibers);
        black_box(());
    });
    carriers
        .shutdown()
        .expect("carrier pool should shut down after bootstrap-only benches");
}

pub fn cross_thread_wake_worker() -> (SyncSender<Arc<CrossThreadWakeState>>, JoinHandle<()>) {
    let (tx, rx) = sync_channel::<Arc<CrossThreadWakeState>>(0);
    let worker = thread::spawn(move || {
        while let Ok(state) = rx.recv() {
            state.signal();
        }
    });
    (tx, worker)
}

pub fn cross_thread_wake_round_fusion(
    runtime: &CurrentAsyncRuntime,
    tx: &SyncSender<Arc<CrossThreadWakeState>>,
) {
    let state = Arc::new(CrossThreadWakeState::default());
    let handle = runtime
        .spawn_with_poll_stack_bytes(
            BENCH_ASYNC_POLL_STACK_BYTES,
            CrossThreadWakeFuture {
                state: Arc::clone(&state),
            },
        )
        .expect("cross-thread wake future should spawn");
    tx.send(state)
        .expect("wake worker should accept one signal request");
    let (): () = handle.join().expect("cross-thread wake future should join");
    black_box(());
}

pub fn cross_thread_wake_round_tokio_current(
    runtime: &tokio::runtime::Runtime,
    tx: &SyncSender<Arc<CrossThreadWakeState>>,
) {
    runtime.block_on(async {
        let state = Arc::new(CrossThreadWakeState::default());
        let handle = tokio::spawn(CrossThreadWakeFuture {
            state: Arc::clone(&state),
        });
        tx.send(state)
            .expect("wake worker should accept one signal request");
        handle
            .await
            .expect("tokio cross-thread wake future should join");
        black_box(());
    });
}

pub fn current_async_contention_round(runtime: &CurrentAsyncRuntime) {
    let mut handles = Vec::with_capacity(ASYNC_CONTENTION_TASKS);
    for _ in 0..ASYNC_CONTENTION_TASKS {
        handles.push(
            runtime
                .spawn_with_poll_stack_bytes(BENCH_ASYNC_POLL_STACK_BYTES, async_contention_job())
                .expect("contention task should spawn"),
        );
    }
    while let Some(handle) = handles.pop() {
        let checksum = handle.join().expect("contention task should join");
        black_box(checksum);
    }
}

pub fn tokio_current_contention_round(runtime: &tokio::runtime::Runtime) {
    runtime.block_on(async {
        let mut handles = Vec::with_capacity(ASYNC_CONTENTION_TASKS);
        for _ in 0..ASYNC_CONTENTION_TASKS {
            handles.push(tokio::spawn(tokio_async_contention_job()));
        }
        while let Some(handle) = handles.pop() {
            let checksum = handle.await.expect("contention task should join");
            black_box(checksum);
        }
    });
}

pub fn bench_thread_async_runtime_lifecycle_throughput<F, Fut>(
    b: &mut Bencher,
    worker_count: usize,
    future_factory: F,
) where
    F: Fn() -> Fut + Copy,
    Fut: Future<Output = ()> + Send + 'static,
{
    b.iter(|| {
        let runtime = thread_async_runtime(worker_count);
        let mut handles = Vec::with_capacity(THROUGHPUT_BATCH_SIZE);
        for _ in 0..THROUGHPUT_BATCH_SIZE {
            handles.push(
                runtime
                    .spawn_with_poll_stack_bytes(BENCH_ASYNC_POLL_STACK_BYTES, future_factory())
                    .expect("throughput task should spawn"),
            );
        }
        bench_block_on(async move {
            while let Some(handle) = handles.pop() {
                let (): () = handle.await.expect("throughput task should join");
                black_box(());
            }
        });
        drop(runtime);
    });
}

pub fn bench_tokio_multi_thread_lifecycle_throughput<F, Fut>(
    b: &mut Bencher,
    worker_count: usize,
    future_factory: F,
) where
    F: Fn() -> Fut + Copy,
    Fut: Future<Output = ()> + Send + 'static,
{
    b.iter(|| {
        let runtime = TokioRuntimeBuilder::new_multi_thread()
            .worker_threads(worker_count)
            .build()
            .expect("tokio multi-thread runtime should build for benches");
        let mut handles = Vec::with_capacity(THROUGHPUT_BATCH_SIZE);
        for _ in 0..THROUGHPUT_BATCH_SIZE {
            handles.push(runtime.spawn(future_factory()));
        }
        bench_block_on(async move {
            while let Some(handle) = handles.pop() {
                handle.await.expect("throughput task should join");
                black_box(());
            }
        });
        drop(runtime);
    });
}

pub fn bench_reactor_batch_ready(b: &mut Bencher, source_count: usize) {
    let reactor = Reactor::new();
    let mut poller = reactor
        .create()
        .expect("reactor should create a poller for benches");
    let pipes = (0..source_count)
        .map(|_| BenchPipe::new())
        .collect::<Vec<_>>();
    let keys = pipes
        .iter()
        .map(|pipe| {
            reactor
                .register(
                    &mut poller,
                    pipe.source(),
                    EventInterest::READABLE | EventInterest::ERROR | EventInterest::HANGUP,
                )
                .expect("bench reactor should register pipe")
        })
        .collect::<Vec<_>>();
    let mut events = vec![EMPTY_EVENT_RECORD; source_count];

    for (index, pipe) in pipes.iter().enumerate() {
        pipe.write_byte(u8::try_from(index % 251).expect("bench byte should fit"));
    }
    let warmup = reactor
        .poll(
            &mut poller,
            &mut events,
            Some(core::time::Duration::from_millis(0)),
        )
        .expect("warmup reactor poll should succeed");
    assert_eq!(
        warmup, source_count,
        "all warmup pipe sources should be ready"
    );
    for pipe in &pipes {
        black_box(pipe.read_byte());
    }

    b.iter(|| {
        for (index, pipe) in pipes.iter().enumerate() {
            pipe.write_byte(u8::try_from(index % 251).expect("bench byte should fit"));
        }
        let ready = reactor
            .poll(
                &mut poller,
                &mut events,
                Some(core::time::Duration::from_millis(0)),
            )
            .expect("reactor poll should succeed");
        black_box(ready);
        for pipe in &pipes {
            black_box(pipe.read_byte());
        }
    });

    for key in keys {
        reactor
            .deregister(&mut poller, key)
            .expect("bench reactor should deregister pipe");
    }
}
