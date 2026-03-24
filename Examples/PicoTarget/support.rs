#![allow(dead_code)]

use core::num::NonZeroUsize;
use core::ptr::addr_of_mut;

use fusion_std::thread::{
    CurrentAsyncRuntime,
    CurrentFiberPool,
    ExecutorConfig,
    ExecutorError,
    FiberPoolBootstrap,
    RuntimeSizingStrategy,
};

include!(concat!(env!("OUT_DIR"), "/pico_backing.rs"));

#[repr(align(64))]
pub struct AlignedBacking<const N: usize>(pub [u8; N]);

#[cfg(feature = "sizing-global-nearest-round-up")]
const PICO_SIZING: RuntimeSizingStrategy = RuntimeSizingStrategy::GlobalNearestRoundUp;
#[cfg(not(feature = "sizing-global-nearest-round-up"))]
const PICO_SIZING: RuntimeSizingStrategy = RuntimeSizingStrategy::Exact;

pub fn build_current_fiber_pool(
    stack_bytes: usize,
    fiber_count: usize,
    slab: *mut u8,
    slab_bytes: usize,
) -> CurrentFiberPool {
    let config = FiberPoolBootstrap::uniform(
        fiber_count,
        NonZeroUsize::new(stack_bytes).expect("fiber stack must be non-zero"),
    )
    .config()
    .with_guard_pages(0)
    .with_sizing_strategy(PICO_SIZING);
    unsafe { CurrentFiberPool::from_static_slab(&config, slab, slab_bytes) }
        .expect("current-thread fiber pool should build from one owning slab")
}

pub fn build_current_async_runtime(
    capacity: usize,
    slab: *mut u8,
    slab_bytes: usize,
) -> Result<CurrentAsyncRuntime, ExecutorError> {
    let config = ExecutorConfig::new()
        .with_capacity(capacity)
        .with_sizing_strategy(PICO_SIZING);
    unsafe { CurrentAsyncRuntime::from_static_slab(config, slab, slab_bytes) }
}

pub unsafe fn main_fiber_pool(slab: *mut AlignedBacking<MAIN_SLAB_BYTES>) -> CurrentFiberPool {
    build_current_fiber_pool(
        MAIN_FIBER_STACK_BYTES,
        MAIN_FIBER_COUNT,
        unsafe {
            addr_of_mut!((*slab).0)
                .cast::<u8>()
                .add(MAIN_FIBER_SLAB_OFFSET)
        },
        MAIN_FIBER_SLAB_BYTES,
    )
}

pub unsafe fn main_async_runtime(
    slab: *mut AlignedBacking<MAIN_SLAB_BYTES>,
) -> Result<CurrentAsyncRuntime, ExecutorError> {
    build_current_async_runtime(
        MAIN_ASYNC_CAPACITY,
        unsafe {
            addr_of_mut!((*slab).0)
                .cast::<u8>()
                .add(MAIN_ASYNC_SLAB_OFFSET)
        },
        MAIN_ASYNC_SLAB_BYTES,
    )
}

pub unsafe fn bench_fiber_pool(slab: *mut AlignedBacking<BENCH_SLAB_BYTES>) -> CurrentFiberPool {
    build_current_fiber_pool(
        BENCH_FIBER_STACK_BYTES,
        BENCH_FIBER_COUNT,
        unsafe {
            addr_of_mut!((*slab).0)
                .cast::<u8>()
                .add(BENCH_FIBER_SLAB_OFFSET)
        },
        BENCH_FIBER_SLAB_BYTES,
    )
}

pub unsafe fn bench_async_runtime(
    slab: *mut AlignedBacking<BENCH_SLAB_BYTES>,
) -> Result<CurrentAsyncRuntime, ExecutorError> {
    build_current_async_runtime(
        BENCH_ASYNC_CAPACITY,
        unsafe {
            addr_of_mut!((*slab).0)
                .cast::<u8>()
                .add(BENCH_ASYNC_SLAB_OFFSET)
        },
        BENCH_ASYNC_SLAB_BYTES,
    )
}
