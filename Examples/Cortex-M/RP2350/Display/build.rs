use std::env;
use std::fs;
use std::num::NonZeroUsize;
use std::path::PathBuf;

use fusion_std::thread::{
    CurrentFiberPool,
    FiberPlanningSupport,
    FiberPoolConfig,
    FiberStackClass,
    RuntimeSizingStrategy,
    generated_default_fiber_stack_bytes,
};

const ANALYZER_BOOTSTRAP_STACK_BYTES_ENV: &str = "FUSION_FIBER_ANALYZER_BOOTSTRAP_STACK_BYTES";
const DISPLAY_FIBER_COUNT: usize = 2;
const MIN_DISPLAY_FIBER_STACK_BYTES: usize = 32 * 1024;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_SIZING_GLOBAL_NEAREST_ROUND_UP");
    println!("cargo:rerun-if-env-changed=FUSION_FIBER_TASK_METADATA");
    println!("cargo:rerun-if-env-changed=FUSION_ASYNC_POLL_STACK_METADATA");
    println!("cargo:rerun-if-env-changed={ANALYZER_BOOTSTRAP_STACK_BYTES_ENV}");

    let stack_bytes =
        selected_stack_bytes().expect("generated display fiber stack metadata should exist");
    let fiber_slab = fiber_pool_slab_request(stack_bytes, rp2350_fiber_sizing());

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR should exist"));
    let output = format!(
        "#[allow(dead_code)] pub const DISPLAY_FIBER_STACK_BYTES: usize = {stack_bytes};\n\
         #[allow(dead_code)] pub const DISPLAY_FIBER_COUNT: usize = {fiber_count};\n\
         #[allow(dead_code)] pub const FIBER_POOL_SLAB_ALIGN: usize = {fiber_align};\n\
         #[allow(dead_code)] pub const FIBER_POOL_SLAB_BYTES: usize = {fiber_bytes};\n\
         #[repr(align({fiber_align}))] pub struct FiberPoolAlignedBacking(pub [u8; FIBER_POOL_SLAB_BYTES]);\n",
        fiber_count = DISPLAY_FIBER_COUNT,
        fiber_align = fiber_slab.align,
        fiber_bytes = fiber_slab.bytes,
    );
    fs::write(out_dir.join("rp2350_backing.rs"), output)
        .expect("generated RP2350 display backing constants should write");
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

fn fiber_pool_slab_request(stack_bytes: usize, sizing: RuntimeSizingStrategy) -> SlabRequest {
    let config = FiberPoolConfig::fixed(
        NonZeroUsize::new(stack_bytes).expect("display fiber stack should be non-zero"),
        DISPLAY_FIBER_COUNT,
    )
    .with_guard_pages(0)
    .with_sizing_strategy(sizing);
    let combined = CurrentFiberPool::backing_plan_with_planning_support(
        &config,
        FiberPlanningSupport::cortex_m(),
    )
    .expect("exact static display fiber-pool backing plan should build")
    .combined()
    .expect("display fiber-pool backing should combine");
    SlabRequest {
        bytes: combined.slab.bytes,
        align: combined.slab.align,
    }
}

fn analyzer_bootstrap_stack_size() -> Option<NonZeroUsize> {
    let raw = env::var_os(ANALYZER_BOOTSTRAP_STACK_BYTES_ENV)?;
    let bytes = raw.to_string_lossy().parse::<usize>().ok()?;
    let bytes = NonZeroUsize::new(bytes)?;
    FiberStackClass::from_stack_bytes(bytes)
        .ok()
        .map(FiberStackClass::size_bytes)
}

fn selected_stack_bytes() -> Result<usize, String> {
    if let Some(stack_size) = analyzer_bootstrap_stack_size() {
        return Ok(stack_size.get().max(MIN_DISPLAY_FIBER_STACK_BYTES));
    }
    generated_default_fiber_stack_bytes()
        .map(|bytes| bytes.max(MIN_DISPLAY_FIBER_STACK_BYTES))
        .map_err(|error| format!("{error:?}"))
}
