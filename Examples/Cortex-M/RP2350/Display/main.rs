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
use core::ptr;
use core::sync::atomic::{
    AtomicU32,
    Ordering,
};

use cortex_m_rt::{
    ExceptionFrame,
    entry,
    exception,
};
use fusion_example_rp2350_on_device::gpio::Rp2350FiberGpioService;
use fusion_example_rp2350_on_device::runtime::drive_once;
use fusion_example_rp2350_on_device::seven_segment::{
    Rp2350FiberFourDigitSevenSegmentDisplay,
    Rp2350FiberFourDigitSevenSegmentDisplayService,
};
use fusion_example_rp2350_on_device::shift_register_74hc595::Rp2350FiberShiftRegister74hc595Service;
use fusion_hal::contract::drivers::bus::gpio::GpioDriveStrength;
use fusion_hal::drivers::peripheral::SevenSegmentPolarity;
use fusion_pal::sys::cpu::soc::board;
use fusion_sys::thread::system_monotonic_time;

fusion_example_rp2350_on_device::fusion_rp2350_export_build_id!();

const DISPLAY_DATA_PIN: u8 = 12;
const DISPLAY_ENABLE_PIN: u8 = 13;
const DISPLAY_LATCH_PIN: u8 = 14;
const DISPLAY_SHIFT_CLOCK_PIN: u8 = 15;
const PANIC_LED_PIN: u8 = 11;

const GPIO_SERVICE_STACK_BYTES: usize = 4096;
const SHIFT_REGISTER_SERVICE_STACK_BYTES: usize = 4096;
const DISPLAY_SERVICE_STACK_BYTES: usize = 4096;
const STARTUP_LED_MILLIS: u64 = 500;
const DISPLAY_DIAGNOSTIC_FALLBACK_SPINS: usize = 12_000_000;
const DISPLAY_BOOT_VALUE: u16 = 0x1234;

const RP2350_PAD_PUE_BIT: u32 = 1 << 3;
const RP2350_PAD_DRIVE_LSB: u32 = 4;
const RP2350_PAD_IE_BIT: u32 = 1 << 6;
const RP2350_PAD_OD_BIT: u32 = 1 << 7;
const RP2350_PAD_ISO_BIT: u32 = 1 << 8;
const RP2350_RESET_DONE_OFFSET: usize = 0x08;
const RP2350_REG_ALIAS_CLR_OFFSET: usize = 0x3000;
const RP2350_RESET_IO_BANK0: u32 = 1 << 6;
const RP2350_RESET_PADS_BANK0: u32 = 1 << 9;
const RP2350_SIO_GPIO_OUT_SET_OFFSET: usize = 0x18;
const RP2350_SIO_GPIO_OUT_CLR_OFFSET: usize = 0x20;
const RP2350_SIO_GPIO_OE_SET_OFFSET: usize = 0x38;
const RP2350_GPIO_CTRL_STRIDE: usize = 8;
const RP2350_GPIO_CTRL_FUNCSEL_OFFSET: usize = 4;
const RP2350_PAD_STRIDE: usize = 4;
const RP2350_PADS_BANK0_FIRST_PAD_OFFSET: usize = 0x04;
const RP2350_SIO_FUNCSEL: u32 = 5;

type PicoGpioService = Rp2350FiberGpioService<4>;
type PicoShiftRegisterService = Rp2350FiberShiftRegister74hc595Service<2>;
type PicoDisplayService = Rp2350FiberFourDigitSevenSegmentDisplayService;

static mut GPIO_SERVICE_STORAGE: MaybeUninit<PicoGpioService> = MaybeUninit::uninit();
static mut SHIFT_REGISTER_SERVICE_STORAGE: MaybeUninit<PicoShiftRegisterService> = MaybeUninit::uninit();
static mut DISPLAY_SERVICE_STORAGE: MaybeUninit<PicoDisplayService> = MaybeUninit::uninit();
#[unsafe(no_mangle)]
static DISPLAY_MAIN_PHASE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
static DISPLAY_MAIN_HEARTBEAT: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
static DISPLAY_SHIFT_INIT_STATUS: AtomicU32 = AtomicU32::new(0);

fn panic_led_on() -> ! {
    let _ = configure_panic_led_output(true);
    loop {
        core::hint::spin_loop();
    }
}

fn spin_pause(mut spins: usize) {
    while spins != 0 {
        core::hint::spin_loop();
        spins -= 1;
    }
}

fn configure_panic_led_output(initial_high: bool) -> Result<(), ()> {
    ensure_bank0_ready().ok_or(())?;
    let ctrl = ctrl_register(PANIC_LED_PIN).ok_or(())?;
    let pad = pad_register(PANIC_LED_PIN).ok_or(())?;
    let sio_oe_set = sio_register_mut(RP2350_SIO_GPIO_OE_SET_OFFSET).ok_or(())?;
    unsafe {
        ptr::write_volatile(ctrl, RP2350_SIO_FUNCSEL);
        let mut pad_value = ptr::read_volatile(pad);
        pad_value |= RP2350_PAD_IE_BIT | RP2350_PAD_PUE_BIT;
        pad_value &= !(RP2350_PAD_OD_BIT | RP2350_PAD_ISO_BIT);
        pad_value &= !(0b11 << RP2350_PAD_DRIVE_LSB);
        pad_value |= 0b01 << RP2350_PAD_DRIVE_LSB;
        ptr::write_volatile(pad, pad_value);
        write_panic_led(initial_high)?;
        ptr::write_volatile(sio_oe_set, 1_u32 << PANIC_LED_PIN);
    }
    Ok(())
}

fn write_panic_led(high: bool) -> Result<(), ()> {
    let register = if high {
        sio_register_mut(RP2350_SIO_GPIO_OUT_SET_OFFSET)
    } else {
        sio_register_mut(RP2350_SIO_GPIO_OUT_CLR_OFFSET)
    }
    .ok_or(())?;
    unsafe {
        ptr::write_volatile(register, 1_u32 << PANIC_LED_PIN);
    }
    Ok(())
}

fn startup_led_sanity_pulse() {
    DISPLAY_MAIN_PHASE.store(0x10, Ordering::Release);
    let _ = configure_panic_led_output(true);
    blocking_pause_millis(STARTUP_LED_MILLIS);
    let _ = write_panic_led(false);
    DISPLAY_MAIN_PHASE.store(0x11, Ordering::Release);
}

fn blocking_pause_millis(millis: u64) {
    if system_monotonic_time()
        .sleep_for(core::time::Duration::from_millis(millis))
        .is_ok()
    {
        return;
    }
    spin_pause(DISPLAY_DIAGNOSTIC_FALLBACK_SPINS);
}

fn ensure_bank0_ready() -> Option<()> {
    let resets_base = peripheral_base("resets")?;
    let reset_clear = rebase_mut(resets_base, RP2350_REG_ALIAS_CLR_OFFSET) as *mut u32;
    let reset_done = rebase(resets_base, RP2350_RESET_DONE_OFFSET) as *const u32;
    unsafe {
        ptr::write_volatile(reset_clear, RP2350_RESET_IO_BANK0 | RP2350_RESET_PADS_BANK0);
        while ptr::read_volatile(reset_done) & (RP2350_RESET_IO_BANK0 | RP2350_RESET_PADS_BANK0)
            != (RP2350_RESET_IO_BANK0 | RP2350_RESET_PADS_BANK0)
        {}
    }
    Some(())
}

fn peripheral_base(name: &'static str) -> Option<usize> {
    board::peripherals()
        .iter()
        .find(|descriptor| descriptor.name == name)
        .map(|descriptor| descriptor.base)
}

fn ctrl_register(pin: u8) -> Option<*mut u32> {
    let base = peripheral_base("io_bank0")?;
    Some(
        rebase_mut(
            base,
            usize::from(pin) * RP2350_GPIO_CTRL_STRIDE + RP2350_GPIO_CTRL_FUNCSEL_OFFSET,
        ) as *mut u32,
    )
}

fn pad_register(pin: u8) -> Option<*mut u32> {
    let base = peripheral_base("pads_bank0")?;
    Some(
        rebase_mut(
            base,
            RP2350_PADS_BANK0_FIRST_PAD_OFFSET + usize::from(pin) * RP2350_PAD_STRIDE,
        ) as *mut u32,
    )
}

fn sio_register_mut(offset: usize) -> Option<*mut u32> {
    let base = peripheral_base("sio")?;
    Some(rebase_mut(base, offset) as *mut u32)
}

const fn rebase(base: usize, offset: usize) -> usize {
    base.wrapping_add(offset)
}

const fn rebase_mut(base: usize, offset: usize) -> usize {
    base.wrapping_add(offset)
}

fn init_gpio_service() -> *mut PicoGpioService {
    unsafe {
        let service = core::ptr::addr_of_mut!(GPIO_SERVICE_STORAGE).cast::<PicoGpioService>();
        service.write(PicoGpioService::new().expect("gpio service should build"));
        service
    }
}

fn init_shift_register_service() -> fusion_example_rp2350_on_device::shift_register_74hc595::Rp2350FiberShiftRegister74hc595<2> {
    DISPLAY_MAIN_PHASE.store(1, Ordering::Release);
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
    DISPLAY_MAIN_PHASE.store(2, Ordering::Release);

    let shift_service_ptr = unsafe {
        let service = core::ptr::addr_of_mut!(SHIFT_REGISTER_SERVICE_STORAGE)
            .cast::<PicoShiftRegisterService>();
        match PicoShiftRegisterService::new(data, shift_clock, latch_clock, output_enable) {
            Ok(shift_service) => {
                DISPLAY_SHIFT_INIT_STATUS.store(1, Ordering::Release);
                service.write(shift_service);
            }
            Err(error) => {
                DISPLAY_SHIFT_INIT_STATUS.store(
                    match error.kind() {
                        fusion_hal::contract::drivers::bus::gpio::GpioErrorKind::Unsupported => 0x10,
                        fusion_hal::contract::drivers::bus::gpio::GpioErrorKind::Invalid => 0x11,
                        fusion_hal::contract::drivers::bus::gpio::GpioErrorKind::Busy => 0x12,
                        fusion_hal::contract::drivers::bus::gpio::GpioErrorKind::ResourceExhausted => 0x13,
                        fusion_hal::contract::drivers::bus::gpio::GpioErrorKind::StateConflict => 0x14,
                        fusion_hal::contract::drivers::bus::gpio::GpioErrorKind::Platform(code) => {
                            0x8000_0000 | (code as u32)
                        }
                    },
                    Ordering::Release,
                );
                panic!("display shift-register service should build");
            }
        }
        service
    };
    let shift_register = unsafe { (&*shift_service_ptr).client_handle() };
    unsafe {
        match Pin::new_unchecked(&mut *shift_service_ptr).spawn::<SHIFT_REGISTER_SERVICE_STACK_BYTES>() {
            Ok(()) => {
                DISPLAY_SHIFT_INIT_STATUS.store(2, Ordering::Release);
            }
            Err(error) => {
                DISPLAY_SHIFT_INIT_STATUS.store(
                    match error.kind() {
                        fusion_hal::contract::drivers::bus::gpio::GpioErrorKind::Unsupported => 0x20,
                        fusion_hal::contract::drivers::bus::gpio::GpioErrorKind::Invalid => 0x21,
                        fusion_hal::contract::drivers::bus::gpio::GpioErrorKind::Busy => 0x22,
                        fusion_hal::contract::drivers::bus::gpio::GpioErrorKind::ResourceExhausted => 0x23,
                        fusion_hal::contract::drivers::bus::gpio::GpioErrorKind::StateConflict => 0x24,
                        fusion_hal::contract::drivers::bus::gpio::GpioErrorKind::Platform(code) => {
                            0x9000_0000 | (code as u32)
                        }
                    },
                    Ordering::Release,
                );
                panic!("display shift-register service should spawn");
            }
        }
    }
    DISPLAY_MAIN_PHASE.store(3, Ordering::Release);
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
    startup_led_sanity_pulse();
    let shift_register = init_shift_register_service();
    let display = init_display_service(shift_register);
    DISPLAY_MAIN_PHASE.store(4, Ordering::Release);
    display
        .set_hex(DISPLAY_BOOT_VALUE)
        .expect("display boot value should write");

    loop {
        DISPLAY_MAIN_PHASE.store(5, Ordering::Release);
        match drive_once() {
            Ok(_) => {}
            Err(_) => panic!("display runtime should keep advancing"),
        }
        DISPLAY_MAIN_HEARTBEAT.fetch_add(1, Ordering::AcqRel);
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
