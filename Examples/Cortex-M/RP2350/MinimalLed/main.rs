#![no_std]
#![no_main]

use core::mem::MaybeUninit;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicBool, Ordering};
use core::time::Duration;

use cortex_m_rt::{entry, exception};
use fusion_sys::hardware::peripheral::LedPair;
use fusion_std::gpio::{Gpio, GpioDriveStrength, GpioPin};
use fusion_std::thread::async_sleep_for;
use fusion_sys::thread::system_monotonic_time;

mod backend {
    include!(concat!(env!("OUT_DIR"), "/rp2350_backing.rs"));
}
use backend::block_on;

const BLUE_LED_PIN: u8 = 28;
const RED_LED_PIN: u8 = 27;
const FIZZBUZZ_PERIOD: Duration = Duration::from_millis(300);
const STARTUP_PHASE_PERIOD: Duration = Duration::from_millis(500);
const PANIC_PHASE_PERIOD: Duration = Duration::from_millis(500);

type PicoLeds = LedPair<GpioPin, GpioPin>;

static mut LED_STORAGE: MaybeUninit<PicoLeds> = MaybeUninit::uninit();
static LEDS_READY: AtomicBool = AtomicBool::new(false);

fn claim_led_pin(pin: u8) -> GpioPin {
    let mut gpio = Gpio::take(pin).expect("gpio pin should be claimable");
    gpio.set_drive_strength(GpioDriveStrength::MilliAmps4)
        .expect("drive strength should be configurable");
    gpio
}

fn init_leds() -> &'static mut PicoLeds {
    unsafe {
        let leds = core::ptr::addr_of_mut!(LED_STORAGE).cast::<PicoLeds>();
        leds.write(
            LedPair::new(claim_led_pin(BLUE_LED_PIN), claim_led_pin(RED_LED_PIN))
                .expect("led pair should configure"),
        );
        LEDS_READY.store(true, Ordering::Release);
        &mut *leds
    }
}

fn panic_leds() -> Option<&'static mut PicoLeds> {
    if !LEDS_READY.load(Ordering::Acquire) {
        return None;
    }
    unsafe {
        let leds = core::ptr::addr_of_mut!(LED_STORAGE).cast::<PicoLeds>();
        Some(&mut *leds)
    }
}

fn blocking_pause(duration: Duration) {
    system_monotonic_time()
        .sleep_for(duration)
        .expect("blocking LED pause should complete");
}

fn panic_pause() {
    if system_monotonic_time()
        .sleep_for(PANIC_PHASE_PERIOD)
        .is_ok()
    {
        return;
    }
    for _ in 0..8_000_000 {
        core::hint::spin_loop();
    }
}

fn startup_dance(leds: &mut PicoLeds) {
    leds.first().expect("first LED should drive");
    blocking_pause(STARTUP_PHASE_PERIOD);
    leds.second().expect("second LED should drive");
    blocking_pause(STARTUP_PHASE_PERIOD);
    leds.both().expect("both LEDs should drive");
    blocking_pause(STARTUP_PHASE_PERIOD);
    leds.off().expect("LEDs should turn off");
    blocking_pause(STARTUP_PHASE_PERIOD);
}

fn fizzbuzz_command(step: u32) -> (bool, bool) {
    (step.is_multiple_of(3), step.is_multiple_of(5))
}

async fn fizzbuzz_loop(leds: &mut PicoLeds) -> ! {
    let mut step = 3_u32;
    loop {
        let (blue_on, red_on) = fizzbuzz_command(step);
        leds.set(blue_on, red_on).expect("LED pair should update");
        async_sleep_for(FIZZBUZZ_PERIOD)
            .await
            .expect("monotonic timer wait should complete");
        step = step.wrapping_add(1);
    }
}

#[entry]
fn main() -> ! {
    let leds = init_leds();
    leds.off().expect("LEDs should start off");
    startup_dance(leds);

    block_on(fizzbuzz_loop(leds))
        .expect("current-thread async runtime should drive fizzbuzz loop");
}

#[exception]
unsafe fn DefaultHandler(irqn: i16) {
    if irqn == 3 {
        fusion_pal::sys::soc::cortex_m::rp2350::service_event_timeout_irq()
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
        if let Some(leds) = panic_leds() {
            let _ = leds.second();
            panic_pause();
            let _ = leds.off();
            panic_pause();
            continue;
        }
        cortex_m::asm::wfi();
    }
}
