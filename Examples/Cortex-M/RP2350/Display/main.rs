#![no_std]
#![no_main]

//! Pico 2 W four-digit seven-segment display wiring.
//!
//! Board layout:
//! - two chained `74HC595` shift registers
//! - `U1` is the segment bank: `Q0..Q7 -> A, B, C, D, E, F, G, DP`
//! - `U2` is the digit-common bank: `Q0..Q3 -> DIG1, DIG2, DIG3, DIG4`
//! - `U1` is first in the serial chain: `Pico -> U1.DS`, then `U1.Q7' -> U2.DS`
//! - `OE`, `STcp`, and `SHcp` are shared across both chips
//!
//! Pico GPIO map:
//! - `GP11` -> panic/fault LED (standalone red LED)
//! - `GP12` -> serial data
//! - `GP13` -> output enable
//! - `GP14` -> latch
//! - `GP15` -> shift clock
//!
//! Shift protocol:
//! 1. shift digit byte first
//! 2. shift segment byte second
//! 3. pulse latch to update both banks together
//!
//! Electrical contract:
//! - common cathode
//! - segment lines are active high
//! - digit commons are active low

use core::mem::MaybeUninit;
use core::panic::PanicInfo;
use core::sync::atomic::{
    AtomicU8,
    Ordering,
};
use core::time::Duration;

use cortex_m_rt::{
    ExceptionFrame,
    exception,
};
use fusion_example_rp2350_on_device::seven_segment_timer::Rp2350TimerFourDigitSevenSegmentDisplay;
use fusion_firmware::sys::hal::drivers::bus::gpio::{
    SystemGpioPin,
    system_gpio,
};
use fusion_hal::contract::drivers::bus::gpio::{
    GpioDriveStrength,
};
use fusion_sys::thread::system_monotonic_time;

fusion_example_rp2350_on_device::fusion_rp2350_export_build_id!();

const DISPLAY_DATA_PIN: u8 = 12;
const DISPLAY_ENABLE_PIN: u8 = 13;
const DISPLAY_LATCH_PIN: u8 = 14;
const DISPLAY_SHIFT_CLOCK_PIN: u8 = 15;
const PANIC_LED_PIN: u8 = 11;

const DISPLAY_VALUE: u16 = 0x1234;
const PANIC_LED_UNINITIALIZED: u8 = 0;
const PANIC_LED_READY: u8 = 1;
const PANIC_LED_FAILED: u8 = 2;

static mut PANIC_LED_STORAGE: MaybeUninit<SystemGpioPin> = MaybeUninit::uninit();
static PANIC_LED_STATE: AtomicU8 = AtomicU8::new(PANIC_LED_UNINITIALIZED);

fn panic_led_on() -> ! {
    let _ = set_panic_led(true);
    loop {
        core::hint::spin_loop();
    }
}

fn panic_led_pin() -> Result<&'static mut SystemGpioPin, ()> {
    match PANIC_LED_STATE.load(Ordering::Acquire) {
        PANIC_LED_READY => unsafe {
            Ok((&mut *core::ptr::addr_of_mut!(PANIC_LED_STORAGE)).assume_init_mut())
        },
        PANIC_LED_FAILED => Err(()),
        _ => {
            let gpio = system_gpio().map_err(|_| ())?;
            let mut pin = gpio.take_pin(PANIC_LED_PIN).map_err(|_| ())?;
            pin.set_drive_strength(GpioDriveStrength::MilliAmps4)
                .map_err(|_| ())?;
            pin.configure_output(false).map_err(|_| ())?;
            unsafe {
                core::ptr::addr_of_mut!(PANIC_LED_STORAGE).write(MaybeUninit::new(pin));
                PANIC_LED_STATE.store(PANIC_LED_READY, Ordering::Release);
                Ok((&mut *core::ptr::addr_of_mut!(PANIC_LED_STORAGE)).assume_init_mut())
            }
        }
    }
}

fn set_panic_led(high: bool) -> Result<(), ()> {
    match panic_led_pin() {
        Ok(pin) => pin.set_level(high).map_err(|_| ()),
        Err(()) => {
            PANIC_LED_STATE.store(PANIC_LED_FAILED, Ordering::Release);
            Err(())
        }
    }
}

#[fusion_firmware::fusion_firmware_main]
fn main() -> ! {
    #[cfg(not(debug_assertions))]
    let _ = set_panic_led(false);
    let display = Rp2350TimerFourDigitSevenSegmentDisplay::common_cathode(
        DISPLAY_DATA_PIN,
        DISPLAY_ENABLE_PIN,
        DISPLAY_LATCH_PIN,
        DISPLAY_SHIFT_CLOCK_PIN,
    )
    .expect("display timer path should initialize");
    display
        .set_hex(DISPLAY_VALUE)
        .expect("display boot value should write");

    loop {
        let _ = system_monotonic_time().sleep_for(Duration::from_millis(250));
    }
}

#[exception]
unsafe fn HardFault(_frame: &ExceptionFrame) -> ! {
    panic_led_on()
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    panic_led_on()
}
