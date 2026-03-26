#![allow(dead_code)]

use fusion_std::thread::{
    CurrentFiberAsyncBootstrap,
    CurrentFiberAsyncParts,
    RuntimeSizingStrategy,
};

include!(concat!(env!("OUT_DIR"), "/rp2350_backing.rs"));

static mut BENCH_SLAB_BACKING: BenchAlignedBacking = BenchAlignedBacking([0; BENCH_SLAB_BYTES]);

#[cfg(feature = "sizing-global-nearest-round-up")]
const RP2350_FIBER_SIZING: RuntimeSizingStrategy = RuntimeSizingStrategy::GlobalNearestRoundUp;
#[cfg(not(feature = "sizing-global-nearest-round-up"))]
const RP2350_FIBER_SIZING: RuntimeSizingStrategy = RuntimeSizingStrategy::Exact;

#[cfg(feature = "sizing-global-nearest-round-up")]
const RP2350_ASYNC_SIZING: RuntimeSizingStrategy = RuntimeSizingStrategy::GlobalNearestRoundUp;
#[cfg(not(feature = "sizing-global-nearest-round-up"))]
const RP2350_ASYNC_SIZING: RuntimeSizingStrategy = RuntimeSizingStrategy::Exact;

fn runtime_bootstrap() -> CurrentFiberAsyncBootstrap<'static> {
    CurrentFiberAsyncBootstrap::auto(BENCH_FIBER_COUNT, BENCH_ASYNC_CAPACITY)
        .expect("generated benchmark fiber stack metadata should exist")
        .with_guard_pages(0)
        .with_fiber_sizing_strategy(RP2350_FIBER_SIZING)
        .with_async_sizing_strategy(RP2350_ASYNC_SIZING)
}

pub fn benchmark_runtime() -> CurrentFiberAsyncParts {
    unsafe {
        runtime_bootstrap()
            .from_static_slab_parts((&raw mut BENCH_SLAB_BACKING).cast::<u8>(), BENCH_SLAB_BYTES)
    }
    .expect("benchmark runtime should build from one owning slab")
}
