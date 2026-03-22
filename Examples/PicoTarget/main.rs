#![no_std]
#![no_main]

use core::cell::UnsafeCell;
use core::panic::PanicInfo;

use cortex_m_rt::entry;
use fusion_std::component::LedPair;
use fusion_std::gpio::{Gpio, GpioDriveStrength, GpioPin};
use fusion_std::thread::{CurrentAsyncRuntime, async_yield_now};

const BLUE_LED_PIN: u8 = 28;
const RED_LED_PIN: u8 = 27;
const STEP_DELAY_CYCLES: u32 = 1_000_000;

#[derive(Clone, Copy)]
struct LedCommand {
    blue_on: bool,
    red_on: bool,
}

struct SharedState {
    current: u64,
    next: u64,
    command: LedCommand,
    command_ready: bool,
}

impl SharedState {
    const fn new() -> Self {
        Self {
            current: 0,
            next: 1,
            command: LedCommand {
                blue_on: false,
                red_on: false,
            },
            command_ready: false,
        }
    }
}

struct SharedStateCell(UnsafeCell<SharedState>);

impl SharedStateCell {
    const fn new() -> Self {
        Self(UnsafeCell::new(SharedState::new()))
    }

    fn with<R>(&self, f: impl FnOnce(&mut SharedState) -> R) -> R {
        // SAFETY: this example uses one current-thread carrier, so shared-state access is
        // serialized by cooperative scheduling rather than hardware concurrency.
        let shared = unsafe { &mut *self.0.get() };
        f(shared)
    }

    fn take_command(&self) -> Option<LedCommand> {
        self.with(|shared| {
            if !shared.command_ready {
                return None;
            }
            shared.command_ready = false;
            Some(shared.command)
        })
    }
}

// SAFETY: the example is single-core and only touches this state through the current-thread pool.
unsafe impl Sync for SharedStateCell {}

static SHARED: SharedStateCell = SharedStateCell::new();

fn fibonacci_step() {
    SHARED.with(|shared| {
        let value = shared.current;
        let is_even = value & 1 == 0;
        shared.command = LedCommand {
            blue_on: is_even,
            red_on: !is_even,
        };
        shared.command_ready = true;

        let next = shared.current.wrapping_add(shared.next);
        shared.current = shared.next;
        shared.next = next;
    });
}

fn configure_led_pin(pin: u8) -> fusion_std::gpio::GpioPin {
    let mut gpio = Gpio::take(pin).expect("gpio pin should be claimable");
    gpio.set_drive_strength(GpioDriveStrength::MilliAmps4)
        .expect("drive strength should be configurable");
    gpio
}

async fn fibonacci_task() {
    loop {
        fibonacci_step();
        async_yield_now().await;
    }
}

async fn dispatch_task(mut leds: LedPair<GpioPin, GpioPin>) {
    loop {
        if let Some(command) = SHARED.take_command() {
            leds.set(command.blue_on, command.red_on)
                .expect("led pair should update");
        }
        async_yield_now().await;
    }
}

#[entry]
fn main() -> ! {
    let mut leds = LedPair::new(
        configure_led_pin(BLUE_LED_PIN),
        configure_led_pin(RED_LED_PIN),
    )
    .expect("gpio pins should configure as one led pair");
    leds.off().expect("led pair should turn off");

    let runtime = CurrentAsyncRuntime::new();

    let _fibonacci = runtime
        .spawn(fibonacci_task())
        .expect("fibonacci task should spawn");

    let _dispatch = runtime
        .spawn(dispatch_task(leds))
        .expect("dispatch task should spawn");

    loop {
        runtime
            .drive_once()
            .expect("current-thread executor should drive one ready task");
        cortex_m::asm::delay(STEP_DELAY_CYCLES);
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        cortex_m::asm::wfi();
    }
}
