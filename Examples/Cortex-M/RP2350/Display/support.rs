#![allow(dead_code)]

use core::num::NonZeroUsize;

use fusion_std::thread::{CurrentFiberPool, FiberPoolConfig, RuntimeSizingStrategy};

include!(concat!(env!("OUT_DIR"), "/rp2350_backing.rs"));

static mut FIBER_POOL_SLAB_BACKING: FiberPoolAlignedBacking =
    FiberPoolAlignedBacking([0; FIBER_POOL_SLAB_BYTES]);

#[cfg(feature = "sizing-global-nearest-round-up")]
const RP2350_FIBER_SIZING: RuntimeSizingStrategy = RuntimeSizingStrategy::GlobalNearestRoundUp;
#[cfg(not(feature = "sizing-global-nearest-round-up"))]
const RP2350_FIBER_SIZING: RuntimeSizingStrategy = RuntimeSizingStrategy::Exact;

fn fiber_pool_config() -> FiberPoolConfig<'static> {
    FiberPoolConfig::fixed(
        NonZeroUsize::new(DISPLAY_FIBER_STACK_BYTES)
            .expect("display fiber stack should be non-zero"),
        DISPLAY_FIBER_COUNT,
    )
    .with_guard_pages(0)
    .with_sizing_strategy(RP2350_FIBER_SIZING)
}

pub fn main_fibers() -> CurrentFiberPool {
    unsafe {
        CurrentFiberPool::from_static_slab(
            &fiber_pool_config(),
            (&raw mut FIBER_POOL_SLAB_BACKING).cast::<u8>(),
            FIBER_POOL_SLAB_BYTES,
        )
    }
    .expect("display fiber pool should build from explicit static backing")
}
