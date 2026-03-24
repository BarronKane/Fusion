use core::future::Future;
use core::task::{Context, Poll, Waker};
use std::env;
use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Wake;
use std::thread;
use std::time::Instant;

use fusion_std::thread::{ExecutorConfig, ThreadAsyncRuntime, ThreadPoolConfig, async_yield_now};

const DEFAULT_SAMPLES: usize = 100;
const PROBE_TASK_CAPACITY: usize = 16;

fn main() {
    if let Err(error) = run() {
        eprintln!("fusion_std_async_lifecycle_probe: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let samples = selected_samples()?;
    println!("async_lifecycle_probe");
    println!(
        "  config: samples={} task_capacity={}",
        samples, PROBE_TASK_CAPACITY
    );

    for workers in [1_usize, 2, 4] {
        print_thread_async_report(workers, samples)?;
    }

    Ok(())
}

fn selected_samples() -> Result<usize, String> {
    match env::var("FUSION_ASYNC_LIFECYCLE_SAMPLES") {
        Ok(raw) => raw
            .parse::<usize>()
            .map_err(|error| format!("invalid FUSION_ASYNC_LIFECYCLE_SAMPLES `{raw}`: {error}"))
            .and_then(|samples| {
                (samples != 0)
                    .then_some(samples)
                    .ok_or_else(|| "FUSION_ASYNC_LIFECYCLE_SAMPLES must be non-zero".to_owned())
            }),
        Err(env::VarError::NotPresent) => Ok(DEFAULT_SAMPLES),
        Err(error) => Err(format!(
            "failed to read FUSION_ASYNC_LIFECYCLE_SAMPLES: {error}"
        )),
    }
}

fn print_thread_async_report(worker_count: usize, samples: usize) -> Result<(), String> {
    let startup = measure_thread_async_startup(worker_count, samples)?;
    let shutdown = measure_thread_async_shutdown(worker_count, samples)?;
    let batch_noop = measure_thread_async_batch(worker_count, AsyncBatchKind::Noop, samples)?;
    let batch_yield_once =
        measure_thread_async_batch(worker_count, AsyncBatchKind::YieldOnce, samples)?;
    let lifecycle_noop =
        measure_thread_async_lifecycle(worker_count, AsyncBatchKind::Noop, samples)?;
    let lifecycle_yield_once =
        measure_thread_async_lifecycle(worker_count, AsyncBatchKind::YieldOnce, samples)?;

    println!("thread_async workers={worker_count}");
    print_timing("startup_only_ns", startup);
    print_timing("shutdown_only_ns", shutdown);
    print_timing("warm_batch_noop_ns", batch_noop);
    print_timing("warm_batch_yield_once_ns", batch_yield_once);
    print_timing("lifecycle_noop_ns", lifecycle_noop);
    print_timing("lifecycle_yield_once_ns", lifecycle_yield_once);
    println!(
        "  lifecycle_minus_warm_batch_noop_estimate_ns: median={}",
        lifecycle_noop.median.saturating_sub(batch_noop.median)
    );
    println!(
        "  lifecycle_minus_warm_batch_yield_once_estimate_ns: median={}",
        lifecycle_yield_once
            .median
            .saturating_sub(batch_yield_once.median)
    );
    Ok(())
}

fn measure_thread_async_startup(
    worker_count: usize,
    sample_count: usize,
) -> Result<TimingSummary, String> {
    let mut samples = Vec::with_capacity(sample_count);
    for _ in 0..sample_count {
        let start = Instant::now();
        let runtime = thread_async_runtime(worker_count)?;
        samples.push(duration_nanos(start));
        drop(runtime);
    }
    Ok(TimingSummary::from_samples(samples))
}

fn measure_thread_async_shutdown(
    worker_count: usize,
    sample_count: usize,
) -> Result<TimingSummary, String> {
    let mut samples = Vec::with_capacity(sample_count);
    for _ in 0..sample_count {
        let runtime = thread_async_runtime(worker_count)?;
        let start = Instant::now();
        drop(runtime);
        samples.push(duration_nanos(start));
    }
    Ok(TimingSummary::from_samples(samples))
}

fn measure_thread_async_batch(
    worker_count: usize,
    kind: AsyncBatchKind,
    sample_count: usize,
) -> Result<TimingSummary, String> {
    let runtime = thread_async_runtime(worker_count)?;
    let mut samples = Vec::with_capacity(sample_count);
    for _ in 0..sample_count {
        let start = Instant::now();
        let mut handles = Vec::with_capacity(PROBE_TASK_CAPACITY);
        for _ in 0..PROBE_TASK_CAPACITY {
            handles.push(spawn_probe_task(&runtime, kind)?);
        }
        bench_block_on(async move {
            while let Some(handle) = handles.pop() {
                let (): () = handle.await.map_err(|error| format!("{error:?}")).unwrap();
                black_box(());
            }
        });
        samples.push(duration_nanos(start));
    }
    drop(runtime);
    Ok(TimingSummary::from_samples(samples))
}

fn measure_thread_async_lifecycle(
    worker_count: usize,
    kind: AsyncBatchKind,
    sample_count: usize,
) -> Result<TimingSummary, String> {
    let mut samples = Vec::with_capacity(sample_count);
    for _ in 0..sample_count {
        let start = Instant::now();
        let runtime = thread_async_runtime(worker_count)?;
        let mut handles = Vec::with_capacity(PROBE_TASK_CAPACITY);
        for _ in 0..PROBE_TASK_CAPACITY {
            handles.push(spawn_probe_task(&runtime, kind)?);
        }
        bench_block_on(async move {
            while let Some(handle) = handles.pop() {
                let (): () = handle.await.map_err(|error| format!("{error:?}")).unwrap();
                black_box(());
            }
        });
        drop(runtime);
        samples.push(duration_nanos(start));
    }
    Ok(TimingSummary::from_samples(samples))
}

fn spawn_probe_task(
    runtime: &ThreadAsyncRuntime,
    kind: AsyncBatchKind,
) -> Result<fusion_std::thread::TaskHandle<()>, String> {
    match kind {
        AsyncBatchKind::Noop => runtime
            .spawn(async {})
            .map_err(|error| format!("noop async batch spawn failed: {error:?}")),
        AsyncBatchKind::YieldOnce => runtime
            .spawn(async {
                async_yield_now().await;
            })
            .map_err(|error| format!("yield-once async batch spawn failed: {error:?}")),
    }
}

fn thread_async_runtime(worker_count: usize) -> Result<ThreadAsyncRuntime, String> {
    ThreadAsyncRuntime::with_executor_config(
        &ThreadPoolConfig {
            min_threads: worker_count,
            max_threads: worker_count,
            ..ThreadPoolConfig::new()
        },
        ExecutorConfig::thread_pool().with_capacity(PROBE_TASK_CAPACITY),
    )
    .map_err(|error| {
        format!("failed to build thread async runtime ({worker_count} workers): {error:?}")
    })
}

#[derive(Debug)]
struct ProbeThreadNotify {
    thread: thread::Thread,
    notified: AtomicBool,
}

impl Wake for ProbeThreadNotify {
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
    let notify = Arc::new(ProbeThreadNotify {
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
enum AsyncBatchKind {
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
