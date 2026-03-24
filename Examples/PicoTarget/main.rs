#![no_std]
#![no_main]

use core::panic::PanicInfo;
use core::time::Duration;

use cortex_m_rt::{entry, exception};
use fusion_sys::thread::system_monotonic_time;
use fusion_std::gpio::{Gpio, GpioDriveStrength, GpioPin};
use fusion_std::thread::async_sleep_for;

mod support;
use support::{AlignedBacking, MAIN_SLAB_BYTES, main_async_runtime, main_fiber_pool};

const BLUE_LED_PIN: u8 = 28;
const RED_LED_PIN: u8 = 27;
const FIZZBUZZ_PERIOD: Duration = Duration::from_millis(300);
const STARTUP_PHASE_PERIOD: Duration = Duration::from_millis(500);

static mut MAIN_SLAB_BACKING: AlignedBacking<MAIN_SLAB_BYTES> =
    AlignedBacking([0; MAIN_SLAB_BYTES]);

fn configure_led_pin(pin: u8) -> fusion_std::gpio::GpioPin {
    let mut gpio = Gpio::take(pin).expect("gpio pin should be claimable");
    gpio.set_drive_strength(GpioDriveStrength::MilliAmps4)
        .expect("drive strength should be configurable");
    gpio.configure_output(false)
        .expect("gpio pin should configure for output");
    gpio
}

struct PicoLeds {
    blue: GpioPin,
    red: GpioPin,
}

impl PicoLeds {
    fn new(blue: u8, red: u8) -> Self {
        Self {
            blue: configure_led_pin(blue),
            red: configure_led_pin(red),
        }
    }

    fn set(&mut self, blue_on: bool, red_on: bool) {
        self.blue
            .set_level(blue_on)
            .expect("blue led should update");
        self.red.set_level(red_on).expect("red led should update");
    }

    fn off(&mut self) {
        self.set(false, false);
    }
}

fn phase_pause() {
    system_monotonic_time()
        .sleep_for(STARTUP_PHASE_PERIOD)
        .expect("startup phase pause should complete");
}

fn startup_dance(leds: &mut PicoLeds) {
    leds.set(true, false);
    phase_pause();
    leds.set(false, true);
    phase_pause();
    leds.set(true, true);
    phase_pause();
    leds.off();
    phase_pause();
}

fn runtime_error_loop(leds: &mut PicoLeds) -> ! {
    loop {
        leds.set(false, true);
        phase_pause();
        leds.set(true, false);
        phase_pause();
    }
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

async fn fizzbuzz_loop(mut leds: PicoLeds) -> ! {
    let mut step = 3_u32;
    loop {
        let (blue_on, red_on) = fizzbuzz_command(step);
        leds.set(blue_on, red_on);
        async_sleep_for(FIZZBUZZ_PERIOD)
            .await
            .expect("monotonic timer wait should complete");
        step = step.wrapping_add(1);
    }
}

#[entry]
fn main() -> ! {
    let mut leds = PicoLeds::new(BLUE_LED_PIN, RED_LED_PIN);
    leds.off();
    startup_dance(&mut leds);

    let fibers = unsafe { main_fiber_pool(&raw mut MAIN_SLAB_BACKING) };
    leds.set(true, false);
    phase_pause();

    let runner = fibers
        .spawn(move || {
            let mut leds = leds;
            leds.set(false, true);
            phase_pause();
            let runtime = match unsafe { main_async_runtime(&raw mut MAIN_SLAB_BACKING) } {
                Ok(runtime) => runtime,
                Err(_) => runtime_error_loop(&mut leds),
            };
            leds.set(true, true);
            phase_pause();
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
