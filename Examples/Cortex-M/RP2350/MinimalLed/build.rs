use std::env;
use std::fs;
use std::path::PathBuf;

use fusion_std::thread::{
    AllocatorLayoutPolicy,
    CurrentFiberAsyncBootstrap,
    ExecutorPlanningSupport,
    FiberPlanningSupport,
    FiberStackClass,
    RuntimeSizingStrategy,
    generated_default_fiber_stack_bytes,
};

const ANALYZER_BOOTSTRAP_STACK_BYTES_ENV: &str = "FUSION_FIBER_ANALYZER_BOOTSTRAP_STACK_BYTES";
const MAIN_FIBER_COUNT: usize = 1;
const MAIN_ASYNC_CAPACITY: usize = 1;
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_SIZING_GLOBAL_NEAREST_ROUND_UP");
    println!("cargo:rerun-if-env-changed=FUSION_FIBER_TASK_METADATA");
    println!("cargo:rerun-if-env-changed=FUSION_ASYNC_POLL_STACK_METADATA");
    println!("cargo:rerun-if-env-changed={ANALYZER_BOOTSTRAP_STACK_BYTES_ENV}");

    let request = current_runtime_slab_request(
        MAIN_FIBER_COUNT,
        MAIN_ASYNC_CAPACITY,
        rp2350_fiber_sizing(),
        rp2350_async_sizing(),
    );
    let stack_bytes =
        selected_stack_bytes().expect("generated minimal-led fiber stack metadata should exist");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR should exist"));
    let output = format!(
        "#[allow(dead_code)] pub const MAIN_FIBER_STACK_BYTES: usize = {stack_bytes};\n\
         #[allow(dead_code)] pub const MAIN_FIBER_COUNT: usize = {MAIN_FIBER_COUNT};\n\
         #[allow(dead_code)] pub const MAIN_ASYNC_CAPACITY: usize = {MAIN_ASYNC_CAPACITY};\n\
         #[allow(dead_code)] pub const MAIN_SLAB_ALIGN: usize = {slab_align};\n\
         #[allow(dead_code)] pub const MAIN_SLAB_BYTES: usize = {slab_bytes};\n\
         #[repr(align({slab_align}))] pub struct MainAlignedBacking(pub [u8; MAIN_SLAB_BYTES]);\n",
        slab_align = request.align,
        slab_bytes = request.bytes,
    );
    fs::write(out_dir.join("rp2350_backing.rs"), output)
        .expect("generated RP2350 backing constants should write");
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
    fiber_count: usize,
    async_capacity: usize,
    fiber_sizing: RuntimeSizingStrategy,
    async_sizing: RuntimeSizingStrategy,
) -> SlabRequest {
    let bootstrap = generated_runtime_bootstrap(fiber_count, async_capacity)
        .with_guard_pages(0)
        .with_fiber_sizing_strategy(fiber_sizing)
        .with_async_sizing_strategy(async_sizing);
    let combined = bootstrap
        .backing_plan_with_planning_support_and_allocator_layout_policy(
            FiberPlanningSupport::cortex_m(),
            ExecutorPlanningSupport::cortex_m(),
            AllocatorLayoutPolicy::exact_static(),
        )
        .expect("exact static current runtime backing plan should build");
    SlabRequest {
        bytes: combined.slab.bytes,
        align: combined.slab.align,
    }
}

fn generated_runtime_bootstrap(
    fiber_count: usize,
    async_capacity: usize,
) -> CurrentFiberAsyncBootstrap<'static> {
    if let Some(stack_size) = analyzer_bootstrap_stack_size() {
        return CurrentFiberAsyncBootstrap::uniform(fiber_count, stack_size, async_capacity);
    }
    CurrentFiberAsyncBootstrap::auto(fiber_count, async_capacity).expect(
        "generated fiber stack metadata should exist; build via `cargo pico-build` or run `fusion_std_fiber_task_pipeline` first",
    )
}

fn analyzer_bootstrap_stack_size() -> Option<core::num::NonZeroUsize> {
    let raw = env::var_os(ANALYZER_BOOTSTRAP_STACK_BYTES_ENV)?;
    let bytes = raw.to_string_lossy().parse::<usize>().ok()?;
    let bytes = core::num::NonZeroUsize::new(bytes)?;
    FiberStackClass::from_stack_bytes(bytes)
        .ok()
        .map(FiberStackClass::size_bytes)
}

fn selected_stack_bytes() -> Result<usize, String> {
    if let Some(stack_size) = analyzer_bootstrap_stack_size() {
        return Ok(stack_size.get());
    }
    generated_default_fiber_stack_bytes().map_err(|error| format!("{error:?}"))
}
