#![no_std]
#![no_main]

use core::num::NonZeroUsize;
use core::panic::PanicInfo;
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicU32, Ordering};
use core::time::Duration;

use cortex_m_rt::{entry, exception};
use fusion_pal::sys::mem::{Address, CachePolicy, MemAdviceCaps, Protect, Region};
use fusion_std::component::LedPair;
use fusion_std::gpio::{Gpio, GpioDriveStrength, GpioPin};
use fusion_std::thread::{
    CurrentAsyncRuntime,
    CurrentAsyncRuntimeBacking,
    CurrentFiberPool,
    CurrentFiberPoolBacking,
    ExecutorConfig,
    FiberPoolConfig,
    async_sleep_for,
};
use fusion_sys::mem::resource::{
    BoundMemoryResource,
    BoundResourceSpec,
    MemoryDomain,
    MemoryGeometry,
    MemoryResourceHandle,
    OvercommitPolicy,
    ResourceAttrs,
    ResourceBackingKind,
    ResourceContract,
    ResourceOpSet,
    ResourceResidencySupport,
    ResourceState,
    ResourceSupport,
    SharingPolicy,
    StateValue,
};

const BLUE_LED_PIN: u8 = 28;
const RED_LED_PIN: u8 = 27;
const FIZZBUZZ_PERIOD: Duration = Duration::from_millis(300);
const FIBER_STACK_BYTES: usize = 128 * 1024;
const STARTUP_STEP_DELAY_CYCLES: u32 = 3_000_000;
const CURRENT_RUNTIME_CAPACITY: usize = 4;

const FIBER_CONTROL_BYTES: usize = 4 * 1024;
const FIBER_RUNTIME_METADATA_BYTES: usize = 4 * 1024;
const FIBER_STACK_METADATA_BYTES: usize = 4 * 1024;
const FIBER_STACKS_BYTES: usize = FIBER_STACK_BYTES;

const ASYNC_CONTROL_BYTES: usize = 16 * 1024;
const ASYNC_REACTOR_BYTES: usize = 4 * 1024;
const ASYNC_REGISTRY_BYTES: usize = 8 * 1024;

#[repr(align(4096))]
struct AlignedBacking<const N: usize>([u8; N]);

static mut FIBER_CONTROL_BACKING: AlignedBacking<FIBER_CONTROL_BYTES> =
    AlignedBacking([0; FIBER_CONTROL_BYTES]);
static mut FIBER_RUNTIME_METADATA_BACKING: AlignedBacking<FIBER_RUNTIME_METADATA_BYTES> =
    AlignedBacking([0; FIBER_RUNTIME_METADATA_BYTES]);
static mut FIBER_STACK_METADATA_BACKING: AlignedBacking<FIBER_STACK_METADATA_BYTES> =
    AlignedBacking([0; FIBER_STACK_METADATA_BYTES]);
static mut FIBER_STACKS_BACKING: AlignedBacking<FIBER_STACKS_BYTES> =
    AlignedBacking([0; FIBER_STACKS_BYTES]);

static mut ASYNC_CONTROL_BACKING: AlignedBacking<ASYNC_CONTROL_BYTES> =
    AlignedBacking([0; ASYNC_CONTROL_BYTES]);
static mut ASYNC_REACTOR_BACKING: AlignedBacking<ASYNC_REACTOR_BYTES> =
    AlignedBacking([0; ASYNC_REACTOR_BYTES]);
static mut ASYNC_REGISTRY_BACKING: AlignedBacking<ASYNC_REGISTRY_BYTES> =
    AlignedBacking([0; ASYNC_REGISTRY_BYTES]);

#[unsafe(no_mangle)]
static FUSION_PICO_DEBUG_PHASE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
static FUSION_PICO_DEBUG_STEP: AtomicU32 = AtomicU32::new(0);

const fn static_geometry() -> MemoryGeometry {
    MemoryGeometry {
        base_granule: NonZeroUsize::new(1).expect("non-zero"),
        alloc_granule: NonZeroUsize::new(1).expect("non-zero"),
        protect_granule: None,
        commit_granule: None,
        lock_granule: None,
        large_granule: None,
    }
}

fn static_contract() -> ResourceContract {
    ResourceContract {
        allowed_protect: Protect::READ | Protect::WRITE,
        write_xor_execute: true,
        sharing: SharingPolicy::Private,
        overcommit: OvercommitPolicy::Disallow,
        cache_policy: CachePolicy::Default,
        integrity: None,
    }
}

fn static_support() -> ResourceSupport {
    ResourceSupport {
        protect: Protect::READ | Protect::WRITE,
        ops: ResourceOpSet::QUERY,
        advice: MemAdviceCaps::empty(),
        residency: ResourceResidencySupport::BEST_EFFORT,
    }
}

fn static_state() -> ResourceState {
    ResourceState::static_state(
        StateValue::Uniform(Protect::READ | Protect::WRITE),
        StateValue::Uniform(false),
        StateValue::Uniform(true),
    )
}

unsafe fn bind_static_resource(ptr: *mut u8, len: usize) -> MemoryResourceHandle {
    MemoryResourceHandle::from(
        BoundMemoryResource::new(BoundResourceSpec::new(
            Region {
                base: Address::new(ptr as usize),
                len,
            },
            MemoryDomain::StaticRegion,
            ResourceBackingKind::StaticRegion,
            ResourceAttrs::ALLOCATABLE
                | ResourceAttrs::STATIC_REGION
                | ResourceAttrs::CACHEABLE
                | ResourceAttrs::COHERENT,
            static_geometry(),
            static_contract(),
            static_support(),
            static_state(),
        ))
        .expect("static backing resource should bind"),
    )
}

fn explicit_fiber_pool() -> CurrentFiberPool {
    FUSION_PICO_DEBUG_PHASE.store(3, Ordering::Release);
    let config = FiberPoolConfig::fixed(
        NonZeroUsize::new(FIBER_STACK_BYTES).expect("fiber stack must be non-zero"),
        1,
    )
    .with_guard_pages(0);
    let plan = CurrentFiberPool::backing_plan(&config).expect("fiber backing plan should build");
    assert!(plan.control.bytes <= FIBER_CONTROL_BYTES);
    assert!(plan.runtime_metadata.bytes <= FIBER_RUNTIME_METADATA_BYTES);
    assert!(plan.stack_metadata.bytes <= FIBER_STACK_METADATA_BYTES);
    assert!(plan.stacks.bytes <= FIBER_STACKS_BYTES);

    let backing = unsafe {
        CurrentFiberPoolBacking {
            control: bind_static_resource(
                addr_of_mut!(FIBER_CONTROL_BACKING.0).cast::<u8>(),
                FIBER_CONTROL_BYTES,
            ),
            runtime_metadata: bind_static_resource(
                addr_of_mut!(FIBER_RUNTIME_METADATA_BACKING.0).cast::<u8>(),
                FIBER_RUNTIME_METADATA_BYTES,
            ),
            stack_metadata: bind_static_resource(
                addr_of_mut!(FIBER_STACK_METADATA_BACKING.0).cast::<u8>(),
                FIBER_STACK_METADATA_BYTES,
            ),
            stacks: bind_static_resource(
                addr_of_mut!(FIBER_STACKS_BACKING.0).cast::<u8>(),
                FIBER_STACKS_BYTES,
            ),
        }
    };

    let fibers = CurrentFiberPool::from_backing(&config, backing)
        .expect("current-thread fiber pool should build from explicit backing");
    FUSION_PICO_DEBUG_PHASE.store(4, Ordering::Release);
    fibers
}

fn explicit_async_runtime() -> CurrentAsyncRuntime {
    FUSION_PICO_DEBUG_PHASE.store(50, Ordering::Release);
    let config = ExecutorConfig::new().with_capacity(CURRENT_RUNTIME_CAPACITY);
    FUSION_PICO_DEBUG_PHASE.store(51, Ordering::Release);
    let plan = CurrentAsyncRuntime::backing_plan(config).expect("async backing plan should build");
    FUSION_PICO_DEBUG_PHASE.store(52, Ordering::Release);
    assert!(plan.control.bytes <= ASYNC_CONTROL_BYTES);
    assert!(plan.reactor.bytes <= ASYNC_REACTOR_BYTES);
    assert!(plan.registry.bytes <= ASYNC_REGISTRY_BYTES);
    FUSION_PICO_DEBUG_PHASE.store(53, Ordering::Release);

    let backing = unsafe {
        CurrentAsyncRuntimeBacking {
            control: bind_static_resource(
                addr_of_mut!(ASYNC_CONTROL_BACKING.0).cast::<u8>(),
                ASYNC_CONTROL_BYTES,
            ),
            reactor: bind_static_resource(
                addr_of_mut!(ASYNC_REACTOR_BACKING.0).cast::<u8>(),
                ASYNC_REACTOR_BYTES,
            ),
            registry: bind_static_resource(
                addr_of_mut!(ASYNC_REGISTRY_BACKING.0).cast::<u8>(),
                ASYNC_REGISTRY_BYTES,
            ),
            future_medium: None,
            future_large: None,
            result_medium: None,
            result_large: None,
        }
    };
    FUSION_PICO_DEBUG_PHASE.store(54, Ordering::Release);

    let runtime = CurrentAsyncRuntime::from_backing(config, backing)
        .expect("current-thread async runtime should build from explicit backing");
    FUSION_PICO_DEBUG_PHASE.store(55, Ordering::Release);
    runtime
}

fn configure_led_pin(pin: u8) -> fusion_std::gpio::GpioPin {
    let mut gpio = Gpio::take(pin).expect("gpio pin should be claimable");
    gpio.set_drive_strength(GpioDriveStrength::MilliAmps4)
        .expect("drive strength should be configurable");
    gpio
}

fn fizzbuzz_command(step: u32) -> (bool, bool) {
    let fizz = step.is_multiple_of(3);
    let buzz = step.is_multiple_of(5);
    match (fizz, buzz) {
        (true, true) => (true, true),
        (true, false) => (true, false),
        (false, true) => (false, true),
        (false, false) => (false, false),
    }
}

fn startup_self_test(leds: &mut LedPair<GpioPin, GpioPin>) {
    FUSION_PICO_DEBUG_PHASE.store(11, Ordering::Release);
    leds.first().expect("first led should drive during startup");
    cortex_m::asm::delay(STARTUP_STEP_DELAY_CYCLES);
    FUSION_PICO_DEBUG_PHASE.store(12, Ordering::Release);
    leds.second()
        .expect("second led should drive during startup");
    cortex_m::asm::delay(STARTUP_STEP_DELAY_CYCLES);
    FUSION_PICO_DEBUG_PHASE.store(13, Ordering::Release);
    leds.both().expect("both leds should drive during startup");
    cortex_m::asm::delay(STARTUP_STEP_DELAY_CYCLES);
    FUSION_PICO_DEBUG_PHASE.store(14, Ordering::Release);
    leds.off().expect("startup self-test should finish dark");
    cortex_m::asm::delay(STARTUP_STEP_DELAY_CYCLES);
}

fn phase_signal(leds: &mut LedPair<GpioPin, GpioPin>, first: bool, second: bool) {
    leds.set(first, second)
        .expect("phase indicator should update");
    cortex_m::asm::delay(STARTUP_STEP_DELAY_CYCLES);
}

async fn fizzbuzz_loop(mut leds: LedPair<GpioPin, GpioPin>) -> ! {
    FUSION_PICO_DEBUG_PHASE.store(7, Ordering::Release);
    phase_signal(&mut leds, true, true);
    let mut step = 3_u32;
    loop {
        FUSION_PICO_DEBUG_PHASE.store(8, Ordering::Release);
        FUSION_PICO_DEBUG_STEP.store(step, Ordering::Release);
        let (blue_on, red_on) = fizzbuzz_command(step);
        leds.set(blue_on, red_on).expect("led pair should update");
        async_sleep_for(FIZZBUZZ_PERIOD)
            .await
            .expect("monotonic timer wait should complete");
        step = step.wrapping_add(1);
    }
}

#[entry]
fn main() -> ! {
    FUSION_PICO_DEBUG_PHASE.store(1, Ordering::Release);
    let mut leds = LedPair::new(
        configure_led_pin(BLUE_LED_PIN),
        configure_led_pin(RED_LED_PIN),
    )
    .expect("gpio pins should configure as one led pair");
    leds.off().expect("led pair should turn off");
    startup_self_test(&mut leds);
    FUSION_PICO_DEBUG_PHASE.store(2, Ordering::Release);

    let fibers = explicit_fiber_pool();

    let runner = fibers
        .spawn(move || {
            FUSION_PICO_DEBUG_PHASE.store(9, Ordering::Release);
            phase_signal(&mut leds, true, false);
            let runtime = explicit_async_runtime();
            FUSION_PICO_DEBUG_PHASE.store(10, Ordering::Release);
            phase_signal(&mut leds, false, true);
            runtime
                .block_on(fizzbuzz_loop(leds))
                .expect("fiber-owned async runtime should drive fizzbuzz loop");
        })
        .expect("fiber-owned async loop should spawn");

    let _: () = runner
        .join()
        .expect("current-thread fiber join should drive the async loop");

    loop {
        cortex_m::asm::wfi();
    }
}

#[exception]
unsafe fn DefaultHandler(irqn: i16) {
    if irqn == 3 {
        FUSION_PICO_DEBUG_PHASE.store(0x3000_0000 | irqn as u32, Ordering::Release);
        fusion_pal::sys::cortex_m::hal::soc::board::service_event_timeout_irq()
            .expect("event-timeout irq should service");
        return;
    }

    FUSION_PICO_DEBUG_PHASE.store(u32::MAX - 1, Ordering::Release);
    loop {
        cortex_m::asm::wfi();
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    FUSION_PICO_DEBUG_PHASE.store(u32::MAX, Ordering::Release);
    loop {
        cortex_m::asm::wfi();
    }
}
