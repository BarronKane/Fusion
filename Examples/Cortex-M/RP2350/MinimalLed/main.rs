#![no_std]
#![no_main]

use core::mem::MaybeUninit;
use core::panic::PanicInfo;
use core::pin::Pin;
use core::sync::atomic::{
    AtomicBool,
    Ordering,
};
use core::time::Duration;

use cortex_m_rt::entry;

use fusion_example_rp2350_on_device::gpio::{
    Rp2350FiberGpioOutputPin,
    Rp2350FiberGpioService,
};
use fusion_example_rp2350_on_device::runtime::{
    drive_once,
    spawn_with_stack,
};
use fusion_hal::contract::drivers::bus::gpio::GpioDriveStrength;
use fusion_hal::drivers::peripheral::LedPair;
use fusion_std::thread::yield_now;
use fusion_sys::thread::system_monotonic_time;

const BLUE_LED_PIN: u8 = 28;
const RED_LED_PIN: u8 = 27;
const FIZZBUZZ_PERIOD: Duration = Duration::from_millis(300);
const STARTUP_PHASE_PERIOD: Duration = Duration::from_millis(500);
const PANIC_PHASE_PERIOD: Duration = Duration::from_millis(500);
const GPIO_SERVICE_STACK_BYTES: usize = 2048;
const FIZZBUZZ_STACK_BYTES: usize = 4096;

type PicoGpioService = Rp2350FiberGpioService<2>;
type PicoGpioPin = Rp2350FiberGpioOutputPin<16, 16>;
type PicoLeds = LedPair<PicoGpioPin, PicoGpioPin>;

static mut LED_STORAGE: MaybeUninit<PicoLeds> = MaybeUninit::uninit();
static mut GPIO_SERVICE_STORAGE: MaybeUninit<PicoGpioService> = MaybeUninit::uninit();
static LEDS_READY: AtomicBool = AtomicBool::new(false);

fn init_gpio_service() -> *mut PicoGpioService {
    unsafe {
        let service = core::ptr::addr_of_mut!(GPIO_SERVICE_STORAGE).cast::<PicoGpioService>();
        service.write(PicoGpioService::new().expect("gpio service should build"));
        service
    }
}

fn init_leds() -> &'static mut PicoLeds {
    let gpio_service = init_gpio_service();
    let blue = unsafe {
        Pin::new_unchecked(&mut *gpio_service)
            .claim_output_pin(BLUE_LED_PIN, GpioDriveStrength::MilliAmps4)
            .expect("blue led pin should be claimable")
    };
    let red = unsafe {
        Pin::new_unchecked(&mut *gpio_service)
            .claim_output_pin(RED_LED_PIN, GpioDriveStrength::MilliAmps4)
            .expect("red led pin should be claimable")
    };
    unsafe {
        Pin::new_unchecked(&mut *gpio_service)
            .spawn::<GPIO_SERVICE_STACK_BYTES>()
            .expect("gpio service fiber should spawn");
    }

    unsafe {
        let leds = core::ptr::addr_of_mut!(LED_STORAGE).cast::<PicoLeds>();
        leds.write(LedPair::new(blue, red).expect("led pair should configure"));
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

fn fizzbuzz_loop(leds: &mut PicoLeds) -> ! {
    let mut step = 3_u32;
    loop {
        let (blue_on, red_on) = fizzbuzz_command(step);
        leds.set(blue_on, red_on).expect("LED pair should update");
        system_monotonic_time()
            .sleep_for(FIZZBUZZ_PERIOD)
            .expect("monotonic timer wait should complete");
        if yield_now().is_err() {
            loop {
                core::hint::spin_loop();
            }
        }
        step = step.wrapping_add(1);
    }
}

#[entry]
fn main() -> ! {
    let leds = init_leds();
    leds.off().expect("LEDs should start off");
    startup_dance(leds);

    let _fizzbuzz = spawn_with_stack::<FIZZBUZZ_STACK_BYTES, _, _>(move || fizzbuzz_loop(leds))
        .expect("fizzbuzz fiber should spawn");

    loop {
        drive_once().expect("current-thread fiber runtime should progress");
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
