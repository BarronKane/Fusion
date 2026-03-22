use core::any::type_name_of_val;
use core::num::NonZeroUsize;
use core::sync::atomic::{AtomicU64, Ordering};
use std::hint::black_box;

use fusion_std::thread::{
    CurrentFiberPool,
    FiberPoolConfig,
    FiberPoolMemoryFootprint,
    FiberTelemetry,
    GreenPool,
    ThreadPool,
    ThreadPoolConfig,
    generated_fiber_task_metadata_by_type_name,
};
use fusion_sys::fiber::Fiber;

const BENCH_STACK_BYTES: usize = 64 * 1024;
const BENCH_CAPACITY: usize = 64;

static METRIC_SEED: AtomicU64 = AtomicU64::new(1);

fn main() {
    if let Err(error) = run() {
        eprintln!("fusion_std_fiber_metrics_probe: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    print_memory_report()?;
    let mut underpredictions = 0usize;
    underpredictions += usize::from(print_stack_accuracy_report(
        "closure_small",
        small_probe_closure,
    )?);
    underpredictions += usize::from(print_stack_accuracy_report(
        "closure_medium",
        medium_probe_closure,
    )?);
    underpredictions += usize::from(print_stack_accuracy_report(
        "closure_large",
        large_probe_closure,
    )?);
    if underpredictions != 0 {
        return Err(format!(
            "{underpredictions} closure probes exceeded the raw generated stack prediction"
        ));
    }
    Ok(())
}

fn print_memory_report() -> Result<(), String> {
    println!("memory_footprint");
    println!(
        "  low_level_fiber_struct_bytes={}",
        core::mem::size_of::<Fiber>()
    );

    let current = benchmark_current_pool()?;
    print_pool_memory("current_pool", current.memory_footprint());
    current
        .shutdown()
        .map_err(|error| format!("failed to shut down current-thread benchmark pool: {error}"))?;

    for carriers in [1_usize, 2, 4] {
        let (carrier_pool, fibers) = benchmark_green_pool(carriers)?;
        print_pool_memory(
            &format!("green_pool_carriers_{carriers}"),
            fibers.memory_footprint(),
        );
        fibers
            .shutdown()
            .map_err(|error| format!("failed to shut down carrier-backed pool: {error}"))?;
        drop(fibers);
        drop(carrier_pool);
    }

    Ok(())
}

fn print_pool_memory(label: &str, footprint: FiberPoolMemoryFootprint) {
    println!(
        "  {label}: carriers={} capacity={} total_bytes={} stack_reserved_bytes={} stack_usable_bytes={} stack_metadata_bytes={} runtime_metadata_bytes={} control_bytes={} per_fiber_total={}/{} per_fiber_bookkeeping={}/{}",
        footprint.carrier_count,
        footprint.task_capacity,
        footprint.total_bytes(),
        footprint.stack.reserved_stack_bytes,
        footprint.stack.usable_stack_bytes,
        footprint.stack.metadata_bytes,
        footprint.runtime_metadata_bytes,
        footprint.control_bytes,
        footprint.total_bytes(),
        footprint.task_capacity.max(1),
        footprint.stack.metadata_bytes + footprint.runtime_metadata_bytes + footprint.control_bytes,
        footprint.task_capacity.max(1),
    );
}

fn print_stack_accuracy_report<F, T>(label: &str, make_job: fn() -> F) -> Result<bool, String>
where
    F: FnOnce() -> T + Send + 'static,
    T: 'static,
{
    let metadata_job = make_job();
    let type_name = type_name_of_val(&metadata_job);
    let metadata = generated_fiber_task_metadata_by_type_name(type_name).map_err(|_| {
        format!(
            "missing generated metadata for `{type_name}`; run `cargo run -p fusion-std --bin fusion_std_fiber_task_pipeline -- --bin fusion_std_fiber_metrics_probe` first"
        )
    })?;
    let actual_peak_used_bytes = measure_runtime_watermark(make_job)?;
    let underpredicted = actual_peak_used_bytes > metadata.stack_bytes;
    let slack = if underpredicted {
        0
    } else {
        metadata.stack_bytes - actual_peak_used_bytes
    };

    println!(
        "stack_accuracy {label}: type_name=\"{type_name}\" predicted_stack_bytes={} actual_peak_used_bytes={} slack_bytes={} status={}",
        metadata.stack_bytes,
        actual_peak_used_bytes,
        slack,
        if underpredicted {
            "UNDERPREDICTED"
        } else {
            "ok"
        },
    );
    Ok(underpredicted)
}

fn measure_runtime_watermark<F, T>(job: fn() -> F) -> Result<usize, String>
where
    F: FnOnce() -> T + Send + 'static,
    T: 'static,
{
    let pool = benchmark_current_pool()?;
    let handle = pool
        .spawn(job())
        .map_err(|error| format!("failed to spawn closure probe: {error}"))?;
    black_box(
        handle
            .join()
            .map_err(|error| format!("failed to join closure probe: {error}"))?,
    );
    let stats = pool
        .stack_stats()
        .ok_or_else(|| "current-thread pool telemetry was disabled unexpectedly".to_owned())?;
    pool.shutdown()
        .map_err(|error| format!("failed to shut down probe pool: {error}"))?;
    Ok(stats.peak_used_bytes)
}

fn benchmark_current_pool() -> Result<CurrentFiberPool, String> {
    CurrentFiberPool::new(
        &FiberPoolConfig::fixed(
            NonZeroUsize::new(BENCH_STACK_BYTES).expect("non-zero benchmark stack bytes"),
            BENCH_CAPACITY,
        )
        .with_telemetry(FiberTelemetry::Full),
    )
    .map_err(|error| format!("failed to build current-thread benchmark pool: {error}"))
}

fn benchmark_green_pool(carriers: usize) -> Result<(ThreadPool, GreenPool), String> {
    let carrier_pool = ThreadPool::new(&ThreadPoolConfig {
        min_threads: carriers,
        max_threads: carriers,
        ..ThreadPoolConfig::new()
    })
    .map_err(|error| format!("failed to build carrier pool ({carriers} workers): {error}"))?;
    let fibers = GreenPool::new(
        &FiberPoolConfig::fixed(
            NonZeroUsize::new(BENCH_STACK_BYTES).expect("non-zero benchmark stack bytes"),
            BENCH_CAPACITY,
        ),
        &carrier_pool,
    )
    .map_err(|error| format!("failed to build green pool ({carriers} workers): {error}"))?;
    Ok((carrier_pool, fibers))
}

#[inline(never)]
fn small_probe_body(seed: u64) -> u64 {
    let mut local = [0_u8; 192];
    let mut index = 0usize;
    while index < local.len() {
        local[index] = low_byte(seed.wrapping_add(index as u64));
        index += 1;
    }
    black_box(local[0]);
    fib_mix(seed ^ u64::from(local[local.len() - 1]), 24)
}

#[inline(never)]
fn medium_probe_body(seed: u64) -> u64 {
    let mut a = [0_u8; 384];
    let mut b = [0_u8; 640];
    let mut index = 0usize;
    while index < a.len() {
        a[index] = low_byte(seed.wrapping_add(index as u64));
        index += 1;
    }
    index = 0;
    while index < b.len() {
        b[index] = low_byte(seed.wrapping_mul(3).wrapping_add(index as u64));
        index += 1;
    }
    black_box((a[7], b[11]));
    small_probe_body(seed ^ u64::from(a[31]) ^ u64::from(b[63]))
}

#[inline(never)]
fn large_probe_body(seed: u64) -> u64 {
    let mut a = [0_u8; 768];
    let mut b = [0_u8; 1024];
    let mut c = [0_u8; 512];
    let mut index = 0usize;
    while index < a.len() {
        a[index] = low_byte(seed.wrapping_add((index * 3) as u64));
        index += 1;
    }
    index = 0;
    while index < b.len() {
        b[index] = low_byte(seed.wrapping_mul(5).wrapping_add(index as u64));
        index += 1;
    }
    index = 0;
    while index < c.len() {
        c[index] = low_byte(seed.wrapping_mul(7).wrapping_add(index as u64));
        index += 1;
    }
    black_box((a[3], b[5], c[7]));
    medium_probe_body(seed ^ u64::from(a[111]) ^ u64::from(b[222]) ^ u64::from(c[63]))
}

#[inline(never)]
const fn fib_mix(mut seed: u64, rounds: usize) -> u64 {
    let mut left = seed | 1;
    let mut right = seed.wrapping_add(1);
    let mut index = 0usize;
    while index < rounds {
        let next = left.wrapping_add(right);
        left = right ^ seed.rotate_left(7);
        right = next ^ left.rotate_right(3);
        seed = seed.wrapping_add(next ^ 0x9e37_79b9);
        index += 1;
    }
    left ^ right ^ seed
}

fn next_seed() -> u64 {
    METRIC_SEED.fetch_add(1, Ordering::AcqRel)
}

const fn low_byte(value: u64) -> u8 {
    value.to_le_bytes()[0]
}

fn small_probe_closure() -> impl FnOnce() -> u64 + Send + 'static {
    let seed = next_seed();
    move || small_probe_body(seed)
}

fn medium_probe_closure() -> impl FnOnce() -> u64 + Send + 'static {
    let seed = next_seed();
    move || medium_probe_body(seed)
}

fn large_probe_closure() -> impl FnOnce() -> u64 + Send + 'static {
    let seed = next_seed();
    move || large_probe_body(seed)
}
