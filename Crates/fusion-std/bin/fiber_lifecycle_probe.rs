use core::num::NonZeroUsize;
use std::env;
use std::sync::Arc;
use std::time::Instant;

use fusion_std::thread::{
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
    ThreadPool,
    ThreadPoolConfig,
    yield_now as green_yield_now,
};
use fusion_sys::sync::Semaphore;

const DEFAULT_SAMPLES: usize = 100;
const PROBE_STACK_BYTES: usize = 16 * 1024;
const PROBE_TASK_CAPACITY: usize = 16;
const INLINE_NO_YIELD_PROBE_TASK: FiberTaskAttributes =
    FiberTaskAttributes::new(FiberStackClass::MIN)
        .with_execution(FiberTaskExecution::InlineNoYield);

fn main() {
    if let Err(error) = run() {
        eprintln!("fusion_std_fiber_lifecycle_probe: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let samples = selected_samples()?;
    println!("lifecycle_probe");
    println!(
        "  config: samples={} stack_bytes={} task_capacity={}",
        samples, PROBE_STACK_BYTES, PROBE_TASK_CAPACITY
    );

    for workers in [1_usize, 2, 4] {
        print_thread_pool_report(workers, samples)?;
    }
    for carriers in [1_usize, 2, 4] {
        print_green_pool_report(carriers, samples)?;
    }

    Ok(())
}

fn selected_samples() -> Result<usize, String> {
    match env::var("FUSION_LIFECYCLE_SAMPLES") {
        Ok(raw) => raw
            .parse::<usize>()
            .map_err(|error| format!("invalid FUSION_LIFECYCLE_SAMPLES `{raw}`: {error}"))
            .and_then(|samples| {
                (samples != 0)
                    .then_some(samples)
                    .ok_or_else(|| "FUSION_LIFECYCLE_SAMPLES must be non-zero".to_owned())
            }),
        Err(env::VarError::NotPresent) => Ok(DEFAULT_SAMPLES),
        Err(error) => Err(format!("failed to read FUSION_LIFECYCLE_SAMPLES: {error}")),
    }
}

fn print_thread_pool_report(worker_count: usize, samples: usize) -> Result<(), String> {
    let startup = measure_thread_pool_startup(worker_count, samples)?;
    let shutdown = measure_thread_pool_shutdown(worker_count, samples)?;
    let dispatch_single = measure_thread_pool_dispatch(worker_count, 1, samples)?;
    let dispatch_barrier = measure_thread_pool_dispatch(worker_count, worker_count, samples)?;

    println!("thread_pool workers={worker_count}");
    print_timing("startup_only_ns", startup);
    print_timing("shutdown_only_ns", shutdown);
    print_timing("dispatch_single_round_trip_ns", dispatch_single);
    print_timing("dispatch_barrier_round_trip_ns", dispatch_barrier);
    Ok(())
}

fn print_green_pool_report(carrier_count: usize, samples: usize) -> Result<(), String> {
    let lifecycle_noop =
        measure_green_pool_lifecycle(carrier_count, GreenBatchKind::Noop, samples)?;
    let lifecycle_yield_once =
        measure_green_pool_lifecycle(carrier_count, GreenBatchKind::YieldOnce, samples)?;
    let bootstrap = measure_green_pool_bootstrap(carrier_count, samples)?;
    let shutdown = measure_green_pool_shutdown(carrier_count, samples)?;
    let dispatch_barrier = measure_thread_pool_dispatch(carrier_count, carrier_count, samples)?;
    let batch_noop = measure_green_pool_batch(carrier_count, GreenBatchKind::Noop, samples)?;
    let batch_yield_once =
        measure_green_pool_batch(carrier_count, GreenBatchKind::YieldOnce, samples)?;
    let shutdown_minus_barrier = shutdown.median.saturating_sub(dispatch_barrier.median);

    println!("green_pool carriers={carrier_count}");
    print_timing("lifecycle_noop_ns", lifecycle_noop);
    print_timing("lifecycle_yield_once_ns", lifecycle_yield_once);
    print_timing("bootstrap_only_ns", bootstrap);
    print_timing("shutdown_with_barrier_ns", shutdown);
    print_timing("carrier_barrier_ns", dispatch_barrier);
    print_timing("warm_batch_noop_ns", batch_noop);
    print_timing("warm_batch_yield_once_ns", batch_yield_once);
    println!("  shutdown_minus_barrier_estimate_ns: median={shutdown_minus_barrier}");
    Ok(())
}

fn measure_green_pool_lifecycle(
    carrier_count: usize,
    kind: GreenBatchKind,
    sample_count: usize,
) -> Result<TimingSummary, String> {
    let mut samples = Vec::with_capacity(sample_count);

    for _ in 0..sample_count {
        let start = Instant::now();
        let runtime = green_runtime(carrier_count)?;
        let fibers = runtime.fibers();
        let mut handles = Vec::with_capacity(PROBE_TASK_CAPACITY);
        for _ in 0..PROBE_TASK_CAPACITY {
            handles.push(match kind {
                GreenBatchKind::Noop => fibers
                    .spawn_with_attrs(INLINE_NO_YIELD_PROBE_TASK, || {})
                    .map_err(|error| format!("lifecycle noop batch spawn failed: {error}"))?,
                GreenBatchKind::YieldOnce => fibers
                    .spawn(|| {
                        green_yield_now().expect("green yield should work");
                    })
                    .map_err(|error| format!("lifecycle yield batch spawn failed: {error}"))?,
            });
        }
        while let Some(handle) = handles.pop() {
            handle
                .join()
                .map_err(|error| format!("lifecycle green batch join failed: {error}"))?;
        }
        let (mut carriers, fibers) = runtime.into_parts();
        fibers
            .shutdown()
            .map_err(|error| format!("green lifecycle cleanup failed: {error}"))?;
        carriers
            .shutdown()
            .map_err(|error| format!("green lifecycle carrier cleanup failed: {error}"))?;
        samples.push(duration_nanos(start));
    }

    Ok(TimingSummary::from_samples(samples))
}

fn measure_thread_pool_startup(
    worker_count: usize,
    sample_count: usize,
) -> Result<TimingSummary, String> {
    let mut samples = Vec::with_capacity(sample_count);
    for _ in 0..sample_count {
        let start = Instant::now();
        let carriers = thread_pool(worker_count)?;
        samples.push(duration_nanos(start));
        carriers
            .shutdown()
            .map_err(|error| format!("thread pool startup cleanup failed: {error}"))?;
    }
    Ok(TimingSummary::from_samples(samples))
}

fn measure_thread_pool_shutdown(
    worker_count: usize,
    sample_count: usize,
) -> Result<TimingSummary, String> {
    let mut samples = Vec::with_capacity(sample_count);
    for _ in 0..sample_count {
        let carriers = thread_pool(worker_count)?;
        let start = Instant::now();
        carriers
            .shutdown()
            .map_err(|error| format!("thread pool shutdown failed: {error}"))?;
        samples.push(duration_nanos(start));
    }
    Ok(TimingSummary::from_samples(samples))
}

fn measure_thread_pool_dispatch(
    worker_count: usize,
    completions: usize,
    sample_count: usize,
) -> Result<TimingSummary, String> {
    let carriers = thread_pool(worker_count)?;
    let completed = Arc::new(
        Semaphore::new(
            0,
            u32::try_from(completions).map_err(|_| "dispatch completions overflow")?,
        )
        .map_err(|error| format!("dispatch semaphore build failed: {error}"))?,
    );
    let mut samples = Vec::with_capacity(sample_count);

    for _ in 0..sample_count {
        drain_semaphore(&completed)?;
        let start = Instant::now();
        for _ in 0..completions {
            let signal = Arc::clone(&completed);
            carriers
                .submit(move || {
                    signal
                        .release(1)
                        .expect("dispatch completion should signal");
                })
                .map_err(|error| format!("thread pool dispatch failed: {error}"))?;
        }
        for _ in 0..completions {
            completed
                .acquire()
                .map_err(|error| format!("dispatch completion wait failed: {error}"))?;
        }
        samples.push(duration_nanos(start));
    }

    carriers
        .shutdown()
        .map_err(|error| format!("thread pool dispatch cleanup failed: {error}"))?;
    Ok(TimingSummary::from_samples(samples))
}

fn measure_green_pool_bootstrap(
    carrier_count: usize,
    sample_count: usize,
) -> Result<TimingSummary, String> {
    let carriers = thread_pool(carrier_count)?;
    let mut samples = Vec::with_capacity(sample_count);

    for _ in 0..sample_count {
        let start = Instant::now();
        let fibers = green_pool(&carriers, carrier_count)?;
        samples.push(duration_nanos(start));
        fibers
            .shutdown()
            .map_err(|error| format!("green pool bootstrap cleanup failed: {error}"))?;
        drop(fibers);
        dispatch_barrier(&carriers, carrier_count)?;
    }

    carriers
        .shutdown()
        .map_err(|error| format!("green bootstrap carrier cleanup failed: {error}"))?;
    Ok(TimingSummary::from_samples(samples))
}

fn measure_green_pool_shutdown(
    carrier_count: usize,
    sample_count: usize,
) -> Result<TimingSummary, String> {
    let carriers = thread_pool(carrier_count)?;
    let mut samples = Vec::with_capacity(sample_count);

    for _ in 0..sample_count {
        let fibers = green_pool(&carriers, carrier_count)?;
        let start = Instant::now();
        fibers
            .shutdown()
            .map_err(|error| format!("green pool shutdown failed: {error}"))?;
        drop(fibers);
        dispatch_barrier(&carriers, carrier_count)?;
        samples.push(duration_nanos(start));
    }

    carriers
        .shutdown()
        .map_err(|error| format!("green shutdown carrier cleanup failed: {error}"))?;
    Ok(TimingSummary::from_samples(samples))
}

fn measure_green_pool_batch(
    carrier_count: usize,
    kind: GreenBatchKind,
    sample_count: usize,
) -> Result<TimingSummary, String> {
    let carriers = thread_pool(carrier_count)?;
    let fibers = green_pool(&carriers, carrier_count)?;
    let mut samples = Vec::with_capacity(sample_count);

    for _ in 0..sample_count {
        let start = Instant::now();
        let mut handles = Vec::with_capacity(PROBE_TASK_CAPACITY);
        for _ in 0..PROBE_TASK_CAPACITY {
            handles.push(match kind {
                GreenBatchKind::Noop => fibers
                    .spawn_with_attrs(INLINE_NO_YIELD_PROBE_TASK, || {})
                    .map_err(|error| format!("warm noop batch spawn failed: {error}"))?,
                GreenBatchKind::YieldOnce => fibers
                    .spawn(|| {
                        green_yield_now().expect("green yield should work");
                    })
                    .map_err(|error| format!("warm yield batch spawn failed: {error}"))?,
            });
        }
        while let Some(handle) = handles.pop() {
            handle
                .join()
                .map_err(|error| format!("warm green batch join failed: {error}"))?;
        }
        samples.push(duration_nanos(start));
    }

    fibers
        .shutdown()
        .map_err(|error| format!("green batch cleanup failed: {error}"))?;
    drop(fibers);
    dispatch_barrier(&carriers, carrier_count)?;
    carriers
        .shutdown()
        .map_err(|error| format!("green batch carrier cleanup failed: {error}"))?;
    Ok(TimingSummary::from_samples(samples))
}

fn dispatch_barrier(carriers: &ThreadPool, worker_count: usize) -> Result<(), String> {
    let completed = Arc::new(
        Semaphore::new(
            0,
            u32::try_from(worker_count).map_err(|_| "worker count overflow")?,
        )
        .map_err(|error| format!("barrier semaphore build failed: {error}"))?,
    );
    for _ in 0..worker_count {
        let signal = Arc::clone(&completed);
        carriers
            .submit(move || {
                signal.release(1).expect("barrier completion should signal");
            })
            .map_err(|error| format!("barrier dispatch failed: {error}"))?;
    }
    for _ in 0..worker_count {
        completed
            .acquire()
            .map_err(|error| format!("barrier wait failed: {error}"))?;
    }
    Ok(())
}

fn drain_semaphore(semaphore: &Semaphore) -> Result<(), String> {
    while semaphore
        .try_acquire()
        .map_err(|error| format!("semaphore drain failed: {error}"))?
    {}
    Ok(())
}

fn thread_pool(worker_count: usize) -> Result<ThreadPool, String> {
    ThreadPool::new(&ThreadPoolConfig {
        min_threads: worker_count,
        max_threads: worker_count,
        ..ThreadPoolConfig::new()
    })
    .map_err(|error| format!("failed to build thread pool ({worker_count} workers): {error}"))
}

fn green_pool(carriers: &ThreadPool, carrier_count: usize) -> Result<GreenPool, String> {
    let growth_chunk = PROBE_TASK_CAPACITY.div_ceil(carrier_count).max(1);
    GreenPool::new(
        &FiberPoolConfig::fixed_growing(
            NonZeroUsize::new(PROBE_STACK_BYTES).expect("non-zero probe stack bytes"),
            PROBE_TASK_CAPACITY,
            growth_chunk,
        )
        .expect("probe fixed-growing config should build")
        .with_reactor_policy(GreenReactorPolicy::Disabled),
        carriers,
    )
    .map_err(|error| format!("failed to build green pool ({carrier_count} carriers): {error}"))
}

fn green_runtime(carrier_count: usize) -> Result<HostedFiberRuntime, String> {
    let growth_chunk = PROBE_TASK_CAPACITY.div_ceil(carrier_count).max(1);
    FiberPoolBootstrap::fixed_growing_with_stack(
        NonZeroUsize::new(PROBE_STACK_BYTES).expect("non-zero probe stack bytes"),
        PROBE_TASK_CAPACITY,
        growth_chunk,
    )
    .expect("probe fixed-growing config should build")
    .build_hosted_with(
        HostedFiberRuntimeConfig::new(carrier_count).with_placement(PoolPlacement::Inherit),
    )
    .map_err(|error| {
        format!("failed to build hosted fiber runtime ({carrier_count} carriers): {error}")
    })
}

fn duration_nanos(start: Instant) -> u128 {
    start.elapsed().as_nanos()
}

fn print_timing(label: &str, summary: TimingSummary) {
    println!(
        "  {label}: median={} min={} max={} p95={}",
        summary.median, summary.min, summary.max, summary.p95
    );
}

#[derive(Debug, Clone, Copy)]
enum GreenBatchKind {
    Noop,
    YieldOnce,
}

#[derive(Debug, Clone, Copy)]
struct TimingSummary {
    median: u128,
    min: u128,
    max: u128,
    p95: u128,
}

impl TimingSummary {
    fn from_samples(mut samples: Vec<u128>) -> Self {
        samples.sort_unstable();
        let len = samples.len();
        let median = samples[len / 2];
        let p95_index = ((len - 1) * 95) / 100;
        Self {
            median,
            min: samples[0],
            max: samples[len - 1],
            p95: samples[p95_index],
        }
    }
}
