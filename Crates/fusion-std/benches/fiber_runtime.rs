#![feature(test)]

extern crate test;

use core::mem::size_of;
use core::num::NonZeroUsize;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};
use std::boxed::Box;
use std::hint::black_box;
use std::vec;

use fusion_std::thread::{
    CurrentFiberPool,
    FiberPoolConfig,
    GreenPool,
    ThreadPool,
    ThreadPoolConfig,
    yield_now as green_yield_now,
};
use fusion_sys::fiber::{Fiber, FiberReturn, FiberStack, FiberYield, yield_now as fiber_yield_now};
use test::Bencher;
use tokio::runtime::Builder as TokioRuntimeBuilder;

const LOW_LEVEL_STACK_WORDS: usize = 4096;
const BENCH_POOL_STACK_BYTES: usize = 64 * 1024;
const BENCH_POOL_CAPACITY: usize = 64;
const OVERRIDE_STACK_BYTES: usize = 512;
const THROUGHPUT_BATCH_SIZE: usize = 16;
const MULTI_YIELD_COUNT: usize = 10;

struct LowLevelYieldingFiber {
    _stack_words: Box<[u128]>,
    progress: Box<AtomicUsize>,
    fiber: Fiber,
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

fn current_pool() -> CurrentFiberPool {
    CurrentFiberPool::new(&bench_pool_config())
        .expect("host backend should support a current-thread fiber pool")
}

fn green_pool() -> (ThreadPool, GreenPool) {
    green_pool_with_carriers(1)
}

fn green_pool_with_carriers(carrier_count: usize) -> (ThreadPool, GreenPool) {
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

const fn bench_pool_config() -> FiberPoolConfig<'static> {
    FiberPoolConfig::fixed(
        NonZeroUsize::new(BENCH_POOL_STACK_BYTES)
            .expect("benchmark fixed stack size should be non-zero"),
        BENCH_POOL_CAPACITY,
    )
}

#[bench]
fn baseline_direct_noop(b: &mut Bencher) {
    fn noop() -> usize {
        7
    }

    b.iter(|| black_box(noop()));
}

#[bench]
fn fiber_low_level_create(b: &mut Bencher) {
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

#[bench]
fn fiber_low_level_resume_yield_round_trip(b: &mut Bencher) {
    let mut fiber = LowLevelYieldingFiber::new();

    b.iter(|| fiber.resume_yielded());

    black_box(fiber.progress.load(Ordering::Acquire));
}

#[bench]
fn current_fiber_pool_spawn_join_noop(b: &mut Bencher) {
    let fibers = current_pool();
    let (): () = fibers
        .spawn(noop_job)
        .expect("warmup task should spawn")
        .join()
        .expect("warmup task should join");
    black_box(());

    b.iter(|| {
        let handle = fibers.spawn(noop_job).expect("benchmark task should spawn");
        let (): () = handle.join().expect("benchmark task should join");
        black_box(());
    });

    fibers.shutdown().expect("benchmark pool should shut down");
}

#[bench]
fn current_fiber_pool_spawn_with_stack_join_noop(b: &mut Bencher) {
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

#[bench]
fn current_fiber_pool_spawn_join_yield_once(b: &mut Bencher) {
    let fibers = current_pool();
    let (): () = fibers
        .spawn(yield_once_job)
        .expect("warmup task should spawn")
        .join()
        .expect("warmup task should join");
    black_box(());

    b.iter(|| {
        let handle = fibers
            .spawn(yield_once_job)
            .expect("benchmark task should spawn");
        let (): () = handle.join().expect("benchmark task should join");
        black_box(());
    });

    fibers.shutdown().expect("benchmark pool should shut down");
}

#[bench]
fn green_pool_spawn_join_noop(b: &mut Bencher) {
    let (_carriers, fibers) = green_pool();
    let (): () = fibers
        .spawn(noop_job)
        .expect("warmup task should spawn")
        .join()
        .expect("warmup task should join");
    black_box(());

    b.iter(|| {
        let handle = fibers.spawn(noop_job).expect("benchmark task should spawn");
        let (): () = handle.join().expect("benchmark task should join");
        black_box(());
    });

    fibers.shutdown().expect("benchmark pool should shut down");
}

#[bench]
fn green_pool_spawn_with_stack_join_noop(b: &mut Bencher) {
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

#[bench]
fn green_pool_spawn_join_yield_once(b: &mut Bencher) {
    let (_carriers, fibers) = green_pool();
    let (): () = fibers
        .spawn(yield_once_job)
        .expect("warmup task should spawn")
        .join()
        .expect("warmup task should join");
    black_box(());

    b.iter(|| {
        let handle = fibers
            .spawn(yield_once_job)
            .expect("benchmark task should spawn");
        let (): () = handle.join().expect("benchmark task should join");
        black_box(());
    });

    fibers.shutdown().expect("benchmark pool should shut down");
}

#[bench]
fn green_pool_throughput_noop_carriers_1(b: &mut Bencher) {
    bench_green_pool_steady_state_throughput(b, 1, noop_job);
}

#[bench]
fn green_pool_throughput_noop_carriers_2(b: &mut Bencher) {
    bench_green_pool_steady_state_throughput(b, 2, noop_job);
}

#[bench]
fn green_pool_throughput_noop_carriers_4(b: &mut Bencher) {
    bench_green_pool_steady_state_throughput(b, 4, noop_job);
}

#[bench]
fn green_pool_throughput_yield_once_carriers_1(b: &mut Bencher) {
    bench_green_pool_steady_state_throughput(b, 1, yield_once_job);
}

#[bench]
fn green_pool_throughput_yield_once_carriers_2(b: &mut Bencher) {
    bench_green_pool_steady_state_throughput(b, 2, yield_once_job);
}

#[bench]
fn green_pool_throughput_yield_once_carriers_4(b: &mut Bencher) {
    bench_green_pool_steady_state_throughput(b, 4, yield_once_job);
}

#[bench]
fn green_pool_lifecycle_noop_carriers_1(b: &mut Bencher) {
    bench_green_pool_lifecycle_throughput(b, 1, noop_job);
}

#[bench]
fn green_pool_lifecycle_noop_carriers_2(b: &mut Bencher) {
    bench_green_pool_lifecycle_throughput(b, 2, noop_job);
}

#[bench]
fn green_pool_lifecycle_noop_carriers_4(b: &mut Bencher) {
    bench_green_pool_lifecycle_throughput(b, 4, noop_job);
}

#[bench]
fn green_pool_lifecycle_yield_once_carriers_1(b: &mut Bencher) {
    bench_green_pool_lifecycle_throughput(b, 1, yield_once_job);
}

#[bench]
fn green_pool_lifecycle_yield_once_carriers_2(b: &mut Bencher) {
    bench_green_pool_lifecycle_throughput(b, 2, yield_once_job);
}

#[bench]
fn green_pool_lifecycle_yield_once_carriers_4(b: &mut Bencher) {
    bench_green_pool_lifecycle_throughput(b, 4, yield_once_job);
}

#[bench]
fn tokio_current_thread_spawn_join_noop(b: &mut Bencher) {
    let runtime = TokioRuntimeBuilder::new_current_thread()
        .build()
        .expect("tokio current-thread runtime should build for benches");

    runtime.block_on(async {
        let handle = tokio::spawn(async {});
        handle.await.expect("warmup task should join");
    });

    b.iter(|| {
        runtime.block_on(async {
            let handle = tokio::spawn(async {});
            handle.await.expect("benchmark task should join");
            black_box(());
        });
    });
}

#[bench]
fn tokio_current_thread_spawn_join_yield_once(b: &mut Bencher) {
    let runtime = TokioRuntimeBuilder::new_current_thread()
        .build()
        .expect("tokio current-thread runtime should build for benches");

    runtime.block_on(async {
        let handle = tokio::spawn(async {
            tokio::task::yield_now().await;
        });
        handle.await.expect("warmup task should join");
    });

    b.iter(|| {
        runtime.block_on(async {
            let handle = tokio::spawn(async {
                tokio::task::yield_now().await;
            });
            handle.await.expect("benchmark task should join");
            black_box(());
        });
    });
}

#[bench]
fn current_fiber_pool_spawn_join_yield_ten_local_state(b: &mut Bencher) {
    let fibers = current_pool();
    let (): () = fibers
        .spawn(yield_ten_local_state_job)
        .expect("warmup task should spawn")
        .join()
        .expect("warmup task should join");
    black_box(());

    b.iter(|| {
        let handle = fibers
            .spawn(yield_ten_local_state_job)
            .expect("benchmark task should spawn");
        let (): () = handle.join().expect("benchmark task should join");
        black_box(());
    });

    fibers.shutdown().expect("benchmark pool should shut down");
}

#[bench]
fn current_fiber_pool_spawn_join_recursive_stack(b: &mut Bencher) {
    let fibers = current_pool();
    let _: usize = fibers
        .spawn(recursive_stack_job)
        .expect("warmup task should spawn")
        .join()
        .expect("warmup task should join");

    b.iter(|| {
        let handle = fibers
            .spawn(recursive_stack_job)
            .expect("benchmark task should spawn");
        let depth = handle.join().expect("benchmark task should join");
        black_box(depth);
    });

    fibers.shutdown().expect("benchmark pool should shut down");
}

const fn noop_job() {}

fn yield_once_job() {
    green_yield_now().expect("benchmark task should yield cleanly");
}

fn yield_ten_local_state_job() {
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

fn recursive_stack_job() -> usize {
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

fn bench_green_pool_steady_state_throughput(b: &mut Bencher, carrier_count: usize, job: fn()) {
    let (carriers, fibers) = green_pool_with_carriers(carrier_count);
    let mut handles = Vec::with_capacity(THROUGHPUT_BATCH_SIZE);
    for _ in 0..THROUGHPUT_BATCH_SIZE {
        handles.push(
            fibers
                .spawn(job)
                .expect("warmup throughput task should spawn successfully"),
        );
    }
    while let Some(handle) = handles.pop() {
        let (): () = handle.join().expect("warmup throughput task should join cleanly");
    }
    black_box(());

    b.iter(|| {
        handles.clear();
        for _ in 0..THROUGHPUT_BATCH_SIZE {
            handles.push(
                fibers
                    .spawn(job)
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
    carriers.shutdown().expect("carrier pool should shut down");
}

fn bench_green_pool_lifecycle_throughput(b: &mut Bencher, carrier_count: usize, job: fn()) {
    b.iter(|| {
        let (carriers, fibers) = green_pool_with_carriers(carrier_count);
        let mut handles = Vec::with_capacity(THROUGHPUT_BATCH_SIZE);
        handles.clear();
        for _ in 0..THROUGHPUT_BATCH_SIZE {
            handles.push(
                fibers
                    .spawn(job)
                    .expect("lifecycle throughput task should spawn successfully"),
            );
        }
        while let Some(handle) = handles.pop() {
            let (): () = handle
                .join()
                .expect("lifecycle throughput task should join cleanly");
            black_box(());
        }
        fibers.shutdown().expect("benchmark pool should shut down");
        carriers.shutdown().expect("carrier pool should shut down");
        drop(fibers);
        black_box(());
    });
}
