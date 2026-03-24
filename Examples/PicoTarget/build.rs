use std::env;
use std::fs;
use std::num::NonZeroUsize;
use std::path::PathBuf;

use fusion_std::thread::{
    CurrentAsyncRuntime,
    CurrentFiberPool,
    ExecutorConfig,
    FiberPoolBootstrap,
    RuntimeSizingStrategy,
};

const MAIN_FIBER_STACK_BYTES: usize = 16 * 1024;
const MAIN_FIBER_COUNT: usize = 1;
const MAIN_ASYNC_CAPACITY: usize = 1;

const BENCH_FIBER_STACK_BYTES: usize = 16 * 1024;
const BENCH_FIBER_COUNT: usize = 1;
const BENCH_ASYNC_CAPACITY: usize = 2;
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_SIZING_GLOBAL_NEAREST_ROUND_UP");

    let main_fiber = fiber_slab_request(
        MAIN_FIBER_STACK_BYTES,
        MAIN_FIBER_COUNT,
        pico_fiber_sizing(),
    );
    let main_async = async_slab_request(MAIN_ASYNC_CAPACITY, pico_async_sizing());
    let bench_fiber = fiber_slab_request(
        BENCH_FIBER_STACK_BYTES,
        BENCH_FIBER_COUNT,
        pico_fiber_sizing(),
    );
    let bench_async = async_slab_request(BENCH_ASYNC_CAPACITY, pico_async_sizing());
    let main_combined = pack_two(main_fiber, main_async);
    let bench_combined = pack_two(bench_fiber, bench_async);

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR should exist"));
    let output = format!(
        "#[allow(dead_code)] pub const MAIN_FIBER_STACK_BYTES: usize = {MAIN_FIBER_STACK_BYTES};\n\
         #[allow(dead_code)] pub const MAIN_FIBER_COUNT: usize = {MAIN_FIBER_COUNT};\n\
         #[allow(dead_code)] pub const MAIN_ASYNC_CAPACITY: usize = {MAIN_ASYNC_CAPACITY};\n\
         #[allow(dead_code)] pub const MAIN_FIBER_SLAB_BYTES: usize = {main_fiber_bytes};\n\
         #[allow(dead_code)] pub const MAIN_ASYNC_SLAB_BYTES: usize = {main_async_bytes};\n\
         #[allow(dead_code)] pub const MAIN_SLAB_BYTES: usize = {main_total_bytes};\n\
         #[allow(dead_code)] pub const MAIN_FIBER_SLAB_OFFSET: usize = {main_fiber_offset};\n\
         #[allow(dead_code)] pub const MAIN_ASYNC_SLAB_OFFSET: usize = {main_async_offset};\n\
         #[allow(dead_code)] pub const BENCH_FIBER_STACK_BYTES: usize = {BENCH_FIBER_STACK_BYTES};\n\
         #[allow(dead_code)] pub const BENCH_FIBER_COUNT: usize = {BENCH_FIBER_COUNT};\n\
         #[allow(dead_code)] pub const BENCH_ASYNC_CAPACITY: usize = {BENCH_ASYNC_CAPACITY};\n\
         #[allow(dead_code)] pub const BENCH_FIBER_SLAB_BYTES: usize = {bench_fiber_bytes};\n\
         #[allow(dead_code)] pub const BENCH_ASYNC_SLAB_BYTES: usize = {bench_async_bytes};\n\
         #[allow(dead_code)] pub const BENCH_SLAB_BYTES: usize = {bench_total_bytes};\n\
         #[allow(dead_code)] pub const BENCH_FIBER_SLAB_OFFSET: usize = {bench_fiber_offset};\n\
         #[allow(dead_code)] pub const BENCH_ASYNC_SLAB_OFFSET: usize = {bench_async_offset};\n",
        main_fiber_bytes = main_fiber.bytes,
        main_async_bytes = main_async.bytes,
        main_total_bytes = main_combined.bytes,
        main_fiber_offset = main_combined.first_offset,
        main_async_offset = main_combined.second_offset,
        bench_fiber_bytes = bench_fiber.bytes,
        bench_async_bytes = bench_async.bytes,
        bench_total_bytes = bench_combined.bytes,
        bench_fiber_offset = bench_combined.first_offset,
        bench_async_offset = bench_combined.second_offset,
    );
    fs::write(out_dir.join("pico_backing.rs"), output)
        .expect("generated pico backing constants should write");
}

#[derive(Clone, Copy)]
struct SlabRequest {
    bytes: usize,
    align: usize,
}

#[derive(Clone, Copy)]
struct PackedSlab {
    bytes: usize,
    first_offset: usize,
    second_offset: usize,
}

fn pico_fiber_sizing() -> RuntimeSizingStrategy {
    // The build script runs on the host, so current-thread fiber backing plans do not get the
    // target PAL's exact context support surface here. Keep fiber slabs conservatively rounded
    // until the target analyzer can emit them directly.
    RuntimeSizingStrategy::GlobalNearestRoundUp
}

fn pico_async_sizing() -> RuntimeSizingStrategy {
    if env::var_os("CARGO_FEATURE_SIZING_GLOBAL_NEAREST_ROUND_UP").is_some() {
        RuntimeSizingStrategy::GlobalNearestRoundUp
    } else {
        RuntimeSizingStrategy::Exact
    }
}

fn align_up(value: usize, align: usize) -> usize {
    let mask = align - 1;
    (value + mask) & !mask
}

fn pack_two(first: SlabRequest, second: SlabRequest) -> PackedSlab {
    let max_align = first.align.max(second.align);
    let mut cursor = max_align.saturating_sub(1);
    let first_offset = align_up(cursor, first.align);
    cursor = first_offset + first.bytes;
    let second_offset = align_up(cursor, second.align);
    let total = second_offset + second.bytes;
    PackedSlab {
        bytes: total,
        first_offset,
        second_offset,
    }
}

fn fiber_slab_request(
    stack_bytes: usize,
    fiber_count: usize,
    sizing: RuntimeSizingStrategy,
) -> SlabRequest {
    let config = FiberPoolBootstrap::uniform(
        fiber_count,
        NonZeroUsize::new(stack_bytes).expect("fiber stack must be non-zero"),
    )
    .config()
    .with_guard_pages(0)
    .with_sizing_strategy(sizing);
    let combined = CurrentFiberPool::backing_plan(&config)
        .expect("fiber backing plan should build")
        .combined()
        .expect("combined fiber slab layout should build");
    SlabRequest {
        bytes: combined.slab.bytes,
        align: combined.slab.align,
    }
}

fn async_slab_request(capacity: usize, sizing: RuntimeSizingStrategy) -> SlabRequest {
    let config = ExecutorConfig::new()
        .with_capacity(capacity)
        .with_sizing_strategy(sizing);
    let combined = CurrentAsyncRuntime::backing_plan(config)
        .expect("async backing plan should build")
        .combined_eager()
        .expect("combined async slab layout should build");
    SlabRequest {
        bytes: combined.slab.bytes,
        align: combined.slab.align,
    }
}
