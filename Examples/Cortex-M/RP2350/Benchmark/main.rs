#![no_std]
#![no_main]

use core::hint::black_box;
use core::panic::PanicInfo;

use cortex_m_rt::{entry, exception};
use fusion_std::thread::{async_yield_now, yield_now as fiber_yield_now};
use fusion_sys::thread::system_monotonic_time;

mod support;
use support::benchmark_runtime;

const ITERATIONS: u32 = 512;

const OUTPUT_MAGIC: u32 = 0x4655_4245;
const OUTPUT_STATE_EMPTY: u32 = 0;
const OUTPUT_STATE_RUNNING: u32 = 1;
const OUTPUT_STATE_DONE: u32 = 2;

const BENCH_BASELINE_DIRECT_NOOP: u32 = 1;
const BENCH_CURRENT_FIBER_SPAWN_JOIN_NOOP: u32 = 2;
const BENCH_CURRENT_FIBER_SPAWN_JOIN_YIELD_ONCE: u32 = 3;
const BENCH_CURRENT_ASYNC_SPAWN_JOIN_NOOP: u32 = 4;
const BENCH_CURRENT_ASYNC_SPAWN_JOIN_YIELD_ONCE: u32 = 5;
#[repr(C)]
#[derive(Clone, Copy)]
struct BenchRecord {
    bench_id: u32,
    iterations: u32,
    total_nanos: u32,
    average_nanos: u32,
}

impl BenchRecord {
    const EMPTY: Self = Self {
        bench_id: 0,
        iterations: 0,
        total_nanos: 0,
        average_nanos: 0,
    };
}

#[repr(C)]
struct BenchOutput {
    magic: u32,
    state: u32,
    count: u32,
    reserved: u32,
    records: [BenchRecord; 8],
}

#[unsafe(no_mangle)]
static mut FUSION_RP2350_BENCH_OUTPUT: BenchOutput = BenchOutput {
    magic: OUTPUT_MAGIC,
    state: OUTPUT_STATE_EMPTY,
    count: 0,
    reserved: 0,
    records: [BenchRecord::EMPTY; 8],
};

fn now_nanos() -> u64 {
    system_monotonic_time()
        .now()
        .expect("monotonic runtime time should exist on RP2350")
        .as_nanos() as u64
}

fn measure_nanos(mut job: impl FnMut()) -> u32 {
    let start = now_nanos();
    job();
    let elapsed = now_nanos().saturating_sub(start);
    u32::try_from(elapsed).unwrap_or(u32::MAX)
}

fn write_record(index: usize, bench_id: u32, total_nanos: u32) {
    let average_nanos = total_nanos / ITERATIONS.max(1);
    unsafe {
        FUSION_RP2350_BENCH_OUTPUT.records[index] = BenchRecord {
            bench_id,
            iterations: ITERATIONS,
            total_nanos,
            average_nanos,
        };
        FUSION_RP2350_BENCH_OUTPUT.count = u32::try_from(index + 1).unwrap_or(u32::MAX);
    }
}

fn run_benchmarks() {
    unsafe {
        FUSION_RP2350_BENCH_OUTPUT.state = OUTPUT_STATE_RUNNING;
        FUSION_RP2350_BENCH_OUTPUT.count = 0;
        FUSION_RP2350_BENCH_OUTPUT.records = [BenchRecord::EMPTY; 8];
        FUSION_RP2350_BENCH_OUTPUT.reserved = 1;
    }

    let total = measure_nanos(|| {
        for _ in 0..ITERATIONS {
            black_box(());
        }
    });
    write_record(0, BENCH_BASELINE_DIRECT_NOOP, total);
    unsafe {
        FUSION_RP2350_BENCH_OUTPUT.reserved = 2;
    }

    let runtime = benchmark_runtime();
    let (fibers, runtime) = runtime.into_parts();
    unsafe {
        FUSION_RP2350_BENCH_OUTPUT.reserved = 3;
    }
    let total = measure_nanos(|| {
        for iteration in 0..ITERATIONS {
            unsafe {
                FUSION_RP2350_BENCH_OUTPUT.reserved = 0x1000 + iteration;
            }
            let handle = fibers
                .spawn(|| black_box(1_u32))
                .expect("noop fiber should spawn");
            unsafe {
                FUSION_RP2350_BENCH_OUTPUT.reserved = 0x2000 + iteration;
            }
            let _ = handle.join().expect("noop fiber should join");
            unsafe {
                FUSION_RP2350_BENCH_OUTPUT.reserved = 0x3000 + iteration;
            }
        }
    });
    write_record(1, BENCH_CURRENT_FIBER_SPAWN_JOIN_NOOP, total);
    unsafe {
        FUSION_RP2350_BENCH_OUTPUT.reserved = 4;
    }

    let total = measure_nanos(|| {
        for iteration in 0..ITERATIONS {
            unsafe {
                FUSION_RP2350_BENCH_OUTPUT.reserved = 0x4000 + iteration;
            }
            let handle = fibers
                .spawn(|| {
                    fiber_yield_now().expect("yielding fiber should yield");
                    black_box(1_u32)
                })
                .expect("yielding fiber should spawn");
            unsafe {
                FUSION_RP2350_BENCH_OUTPUT.reserved = 0x5000 + iteration;
            }
            let _ = handle.join().expect("yielding fiber should join");
            unsafe {
                FUSION_RP2350_BENCH_OUTPUT.reserved = 0x6000 + iteration;
            }
        }
    });
    write_record(2, BENCH_CURRENT_FIBER_SPAWN_JOIN_YIELD_ONCE, total);
    unsafe {
        FUSION_RP2350_BENCH_OUTPUT.reserved = 5;
    }
    fibers
        .shutdown()
        .expect("benchmark fiber pool should shut down cleanly");
    let runtime = runtime
        .build_explicit()
        .expect("benchmark async runtime should build from one owning slab");
    unsafe {
        FUSION_RP2350_BENCH_OUTPUT.reserved = 6;
    }
    let total = measure_nanos(|| {
        for _ in 0..ITERATIONS {
            let handle = runtime
                .spawn(async { black_box(1_u32) })
                .expect("async noop should spawn");
            let _ = runtime
                .block_on(handle)
                .expect("runtime should drive noop async task")
                .expect("noop async task should complete");
        }
    });
    write_record(3, BENCH_CURRENT_ASYNC_SPAWN_JOIN_NOOP, total);
    unsafe {
        FUSION_RP2350_BENCH_OUTPUT.reserved = 7;
    }

    let total = measure_nanos(|| {
        for _ in 0..ITERATIONS {
            let handle = runtime
                .spawn(async {
                    async_yield_now().await;
                    black_box(1_u32)
                })
                .expect("yielding async task should spawn");
            let _ = runtime
                .block_on(handle)
                .expect("runtime should drive yielding async task")
                .expect("yielding async task should complete");
        }
    });
    write_record(4, BENCH_CURRENT_ASYNC_SPAWN_JOIN_YIELD_ONCE, total);

    unsafe {
        FUSION_RP2350_BENCH_OUTPUT.reserved = 8;
        FUSION_RP2350_BENCH_OUTPUT.state = OUTPUT_STATE_DONE;
    }
}

#[entry]
fn main() -> ! {
    run_benchmarks();
    loop {
        cortex_m::asm::wfi();
    }
}

#[exception]
unsafe fn DefaultHandler(irqn: i16) {
    if irqn == 3 {
        fusion_pal::sys::cortex_m::hal::soc::board::service_event_timeout_irq()
            .expect("event-timeout irq should service");
        return;
    }
    loop {
        cortex_m::asm::wfi();
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        cortex_m::asm::wfi();
    }
}
