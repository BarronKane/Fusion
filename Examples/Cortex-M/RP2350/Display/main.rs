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
use core::pin::Pin;
use core::sync::atomic::{
    AtomicU8,
    Ordering,
};

use cortex_m_rt::{
    ExceptionFrame,
    entry,
    exception,
};
use fusion_example_rp2350_on_device::gpio::Rp2350FiberGpioService;
use fusion_example_rp2350_on_device::runtime::wait_for_runtime_progress;
use fusion_example_rp2350_on_device::seven_segment::{
    Rp2350FiberFourDigitSevenSegmentDisplay,
    Rp2350FiberFourDigitSevenSegmentDisplayService,
};
use fusion_example_rp2350_on_device::shift_register_74hc595::Rp2350FiberShiftRegister74hc595Service;
use fusion_firmware::sys::hal::drivers::bus::gpio::{
    SystemGpioPin,
    system_gpio,
};
use fusion_hal::contract::drivers::bus::gpio::{
    GpioControlContract,
    GpioDriveStrength,
};
use fusion_hal::drivers::peripheral::SevenSegmentPolarity;

fusion_example_rp2350_on_device::fusion_rp2350_export_build_id!();

const DISPLAY_DATA_PIN: u8 = 12;
const DISPLAY_ENABLE_PIN: u8 = 13;
const DISPLAY_LATCH_PIN: u8 = 14;
const DISPLAY_SHIFT_CLOCK_PIN: u8 = 15;
const PANIC_LED_PIN: u8 = 11;

const GPIO_SERVICE_STACK_BYTES: usize = 4096;
const SHIFT_REGISTER_SERVICE_STACK_BYTES: usize = 4096;
const DISPLAY_SERVICE_STACK_BYTES: usize = 4096;
const DISPLAY_VALUE: u16 = 0x1234;
const PANIC_LED_UNINITIALIZED: u8 = 0;
const PANIC_LED_READY: u8 = 1;
const PANIC_LED_FAILED: u8 = 2;

type PicoGpioService = Rp2350FiberGpioService<4>;
type PicoShiftRegisterService = Rp2350FiberShiftRegister74hc595Service<2>;
type PicoDisplayService = Rp2350FiberFourDigitSevenSegmentDisplayService;

static mut GPIO_SERVICE_STORAGE: MaybeUninit<PicoGpioService> = MaybeUninit::uninit();
static mut SHIFT_REGISTER_SERVICE_STORAGE: MaybeUninit<PicoShiftRegisterService> = MaybeUninit::uninit();
static mut DISPLAY_SERVICE_STORAGE: MaybeUninit<PicoDisplayService> = MaybeUninit::uninit();
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

fn init_gpio_service() -> *mut PicoGpioService {
    unsafe {
        let service = core::ptr::addr_of_mut!(GPIO_SERVICE_STORAGE).cast::<PicoGpioService>();
        service.write(PicoGpioService::new().expect("gpio service should build"));
        service
    }
}

fn init_shift_register_service() -> fusion_example_rp2350_on_device::shift_register_74hc595::Rp2350FiberShiftRegister74hc595<2> {
    let gpio_service = init_gpio_service();
    let data = unsafe {
        Pin::new_unchecked(&mut *gpio_service)
            .claim_output_pin(DISPLAY_DATA_PIN, GpioDriveStrength::MilliAmps4)
            .expect("display data pin should be claimable")
    };
    let shift_clock = unsafe {
        Pin::new_unchecked(&mut *gpio_service)
            .claim_output_pin(DISPLAY_SHIFT_CLOCK_PIN, GpioDriveStrength::MilliAmps4)
            .expect("display shift clock pin should be claimable")
    };
    let latch_clock = unsafe {
        Pin::new_unchecked(&mut *gpio_service)
            .claim_output_pin(DISPLAY_LATCH_PIN, GpioDriveStrength::MilliAmps4)
            .expect("display latch pin should be claimable")
    };
    let output_enable = unsafe {
        Pin::new_unchecked(&mut *gpio_service)
            .claim_output_pin(DISPLAY_ENABLE_PIN, GpioDriveStrength::MilliAmps4)
            .expect("display output-enable pin should be claimable")
    };
    unsafe {
        Pin::new_unchecked(&mut *gpio_service)
            .spawn::<GPIO_SERVICE_STACK_BYTES>()
            .expect("display gpio service should spawn");
    }

    let shift_service_ptr = unsafe {
        let service = core::ptr::addr_of_mut!(SHIFT_REGISTER_SERVICE_STORAGE)
            .cast::<PicoShiftRegisterService>();
        let shift_service = PicoShiftRegisterService::new(data, shift_clock, latch_clock, output_enable)
            .expect("display shift-register service should build");
        service.write(shift_service);
        service
    };
    let shift_register = unsafe { (&*shift_service_ptr).client_handle() };
    unsafe {
        Pin::new_unchecked(&mut *shift_service_ptr)
            .spawn::<SHIFT_REGISTER_SERVICE_STACK_BYTES>()
            .expect("display shift-register service should spawn");
    }
    shift_register
}

fn init_display_service(
    shift_register: fusion_example_rp2350_on_device::shift_register_74hc595::Rp2350FiberShiftRegister74hc595<2>,
) -> Rp2350FiberFourDigitSevenSegmentDisplay {
    let display_service_ptr = unsafe {
        let service = core::ptr::addr_of_mut!(DISPLAY_SERVICE_STORAGE).cast::<PicoDisplayService>();
        match PicoDisplayService::new(shift_register, SevenSegmentPolarity::common_cathode()) {
            Ok(display_service) => service.write(display_service),
            Err(_) => panic!("display service should build"),
        }
        service
    };
    let display = unsafe { (&*display_service_ptr).client_handle() };
    unsafe {
        match Pin::new_unchecked(&mut *display_service_ptr).spawn::<DISPLAY_SERVICE_STACK_BYTES>() {
            Ok(()) => {}
            Err(_) => panic!("display service should spawn"),
        }
    }
    display
}

#[entry]
fn main() -> ! {
    let _ = set_panic_led(false);
    let shift_register = init_shift_register_service();
    let display = init_display_service(shift_register);
    display
        .set_hex(DISPLAY_VALUE)
        .expect("display boot value should write");

    loop {
        wait_for_runtime_progress();
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
