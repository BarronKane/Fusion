use std::env;
use std::fs;
use std::num::NonZeroUsize;
use std::path::PathBuf;

use fusion_std::thread::{
    AllocatorLayoutPolicy,
    CurrentFiberAsyncBootstrap,
    FiberPlanningSupport,
    RuntimeSizingStrategy,
};

const BENCH_FIBER_STACK_BYTES: usize = 16 * 1024;
const BENCH_FIBER_COUNT: usize = 1;
const BENCH_ASYNC_CAPACITY: usize = 2;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_SIZING_GLOBAL_NEAREST_ROUND_UP");

    let request = current_runtime_slab_request(
        BENCH_FIBER_STACK_BYTES,
        BENCH_FIBER_COUNT,
        BENCH_ASYNC_CAPACITY,
        rp2350_fiber_sizing(),
        rp2350_async_sizing(),
    );

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR should exist"));
    let output = format!(
        "#[allow(dead_code)] pub const BENCH_FIBER_STACK_BYTES: usize = {BENCH_FIBER_STACK_BYTES};\n\
         #[allow(dead_code)] pub const BENCH_FIBER_COUNT: usize = {BENCH_FIBER_COUNT};\n\
         #[allow(dead_code)] pub const BENCH_ASYNC_CAPACITY: usize = {BENCH_ASYNC_CAPACITY};\n\
         #[allow(dead_code)] pub const BENCH_SLAB_ALIGN: usize = {slab_align};\n\
         #[allow(dead_code)] pub const BENCH_SLAB_BYTES: usize = {slab_bytes};\n\
         #[repr(align({slab_align}))] pub struct BenchAlignedBacking(pub [u8; BENCH_SLAB_BYTES]);\n",
        slab_align = request.align,
        slab_bytes = request.bytes,
    );
    fs::write(out_dir.join("rp2350_backing.rs"), output)
        .expect("generated RP2350 benchmark backing constants should write");
}

#[derive(Clone, Copy)]
struct SlabRequest {
    bytes: usize,
    align: usize,
}

fn rp2350_fiber_sizing() -> RuntimeSizingStrategy {
    if env::var_os("CARGO_FEATURE_SIZING_GLOBAL_NEAREST_ROUND_UP").is_some() {
        RuntimeSizingStrategy::GlobalNearestRoundUp
    } else {
        RuntimeSizingStrategy::Exact
    }
}

fn rp2350_async_sizing() -> RuntimeSizingStrategy {
    if env::var_os("CARGO_FEATURE_SIZING_GLOBAL_NEAREST_ROUND_UP").is_some() {
        RuntimeSizingStrategy::GlobalNearestRoundUp
    } else {
        RuntimeSizingStrategy::Exact
    }
}

fn current_runtime_slab_request(
    stack_bytes: usize,
    fiber_count: usize,
    async_capacity: usize,
    fiber_sizing: RuntimeSizingStrategy,
    async_sizing: RuntimeSizingStrategy,
) -> SlabRequest {
    let bootstrap = CurrentFiberAsyncBootstrap::uniform(
        fiber_count,
        NonZeroUsize::new(stack_bytes).expect("fiber stack must be non-zero"),
        async_capacity,
    )
    .with_guard_pages(0)
    .with_fiber_sizing_strategy(fiber_sizing)
    .with_async_sizing_strategy(async_sizing);
    let combined = bootstrap
        .backing_plan_with_fiber_planning_support_and_allocator_layout_policy(
            FiberPlanningSupport::cortex_m(),
            AllocatorLayoutPolicy::exact_static(),
        )
        .expect("exact static benchmark runtime backing plan should build");
    SlabRequest {
        bytes: combined.slab.bytes,
        align: combined.slab.align,
    }
}
