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
    AtomicU32,
    AtomicU8,
    Ordering,
};
use core::time::Duration;

use cortex_m_rt::{
    ExceptionFrame,
    exception,
};
use fusion_example_rp2350_on_device::seven_segment_timer::Rp2350TimerFourDigitSevenSegmentDisplay;
use fusion_firmware::sys::hal::drivers::pcu::system_pio_courier;
use fusion_firmware::sys::hal::drivers::bus::gpio::{
    SystemGpioPin,
    system_gpio,
};
use fusion_hal::contract::drivers::bus::gpio::{
    GpioDriveStrength,
};
use fusion_pal::sys::pcu::{
    PcuBaseContract,
    PcuDispatchContract,
    PcuInvocationBindings,
    PcuInvocationParameters,
    PcuPersistentHandle,
    PcuStreamInstallation,
    system_pcu,
};
use fusion_pcu::model::PcuStreamKernelBuilder;
use fusion_sys::thread::system_monotonic_time;

fusion_example_rp2350_on_device::fusion_rp2350_export_build_id!();

const DISPLAY_DATA_PIN: u8 = 12;
const DISPLAY_ENABLE_PIN: u8 = 13;
const DISPLAY_LATCH_PIN: u8 = 14;
const DISPLAY_SHIFT_CLOCK_PIN: u8 = 15;
const PANIC_LED_PIN: u8 = 11;

const DISPLAY_BOOT_VALUE: u16 = 0x1234;
const DISPLAY_STEP_PERIOD_MILLIS: u64 = 250;
const DISPLAY_DIRECTION_TOGGLE_STEPS: u8 = 16;
const PANIC_LED_UNINITIALIZED: u8 = 0;
const PANIC_LED_READY: u8 = 1;
const PANIC_LED_FAILED: u8 = 2;
const DEBUG_MAGIC_START: u32 = 0x4450_4330;
const DEBUG_MAGIC_END: u32 = 0x4450_4331;

static mut PANIC_LED_STORAGE: MaybeUninit<SystemGpioPin> = MaybeUninit::uninit();
static PANIC_LED_STATE: AtomicU8 = AtomicU8::new(PANIC_LED_UNINITIALIZED);

#[repr(C)]
pub struct Rp2350DisplayPcuDebugState {
    pub magic_start: u32,
    pub value: AtomicU32,
    pub direction: AtomicU32,
    pub heartbeat: AtomicU32,
    pub phase: AtomicU32,
    pub magic_end: u32,
}

#[unsafe(no_mangle)]
#[used]
pub static RP2350_DISPLAY_PCU_DEBUG_STATE: Rp2350DisplayPcuDebugState =
    Rp2350DisplayPcuDebugState {
        magic_start: DEBUG_MAGIC_START,
        value: AtomicU32::new(DISPLAY_BOOT_VALUE as u32),
        direction: AtomicU32::new(1),
        heartbeat: AtomicU32::new(0),
        phase: AtomicU32::new(0),
        magic_end: DEBUG_MAGIC_END,
    };

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
    RP2350_DISPLAY_PCU_DEBUG_STATE
        .phase
        .store(1, Ordering::Release);
    #[cfg(not(debug_assertions))]
    let _ = set_panic_led(false);

    RP2350_DISPLAY_PCU_DEBUG_STATE
        .phase
        .store(2, Ordering::Release);
    let increment_kernel = PcuStreamKernelBuilder::<1>::words(0x1301, "display.increment")
        .increment()
        .expect("increment kernel should fit");
    RP2350_DISPLAY_PCU_DEBUG_STATE
        .phase
        .store(3, Ordering::Release);
    let decrement_kernel = PcuStreamKernelBuilder::<1>::words(0x1302, "display.decrement")
        .decrement()
        .expect("decrement kernel should fit");
    RP2350_DISPLAY_PCU_DEBUG_STATE
        .phase
        .store(4, Ordering::Release);
    let pcu_support = system_pcu().support();
    let increment_program = increment_kernel.kernel();
    let decrement_program = decrement_kernel.kernel();
    assert!(
        pcu_support.supports_kernel_direct(increment_program),
        "increment kernel should lower directly to PIO"
    );
    assert!(
        pcu_support.supports_kernel_direct(decrement_program),
        "decrement kernel should lower directly to PIO"
    );
    assert!(
        !pcu_support.supports_kernel_cpu_fallback(increment_program),
        "display PCU proving path must not admit cpu fallback"
    );
    assert!(
        !pcu_support.supports_kernel_cpu_fallback(decrement_program),
        "display PCU proving path must not admit cpu fallback"
    );
    RP2350_DISPLAY_PCU_DEBUG_STATE
        .phase
        .store(5, Ordering::Release);
    let courier = system_pio_courier().expect("pio courier should initialize");
    RP2350_DISPLAY_PCU_DEBUG_STATE
        .phase
        .store(6, Ordering::Release);
    let mut increment = courier
        .install_stream(
            PcuStreamInstallation {
                kernel: &increment_kernel.ir(),
            },
            PcuInvocationBindings::empty(),
            PcuInvocationParameters::empty(),
        )
        .expect("increment stream should install");
    RP2350_DISPLAY_PCU_DEBUG_STATE
        .phase
        .store(7, Ordering::Release);
    let mut decrement = courier
        .install_stream(
            PcuStreamInstallation {
                kernel: &decrement_kernel.ir(),
            },
            PcuInvocationBindings::empty(),
            PcuInvocationParameters::empty(),
        )
        .expect("decrement stream should install");
    RP2350_DISPLAY_PCU_DEBUG_STATE
        .phase
        .store(8, Ordering::Release);
    increment.start().expect("increment stream should start");
    RP2350_DISPLAY_PCU_DEBUG_STATE
        .phase
        .store(9, Ordering::Release);
    decrement.start().expect("decrement stream should start");
    RP2350_DISPLAY_PCU_DEBUG_STATE
        .phase
        .store(10, Ordering::Release);

    let display = Rp2350TimerFourDigitSevenSegmentDisplay::common_cathode(
        DISPLAY_DATA_PIN,
        DISPLAY_ENABLE_PIN,
        DISPLAY_LATCH_PIN,
        DISPLAY_SHIFT_CLOCK_PIN,
    )
    .expect("display timer path should initialize");
    RP2350_DISPLAY_PCU_DEBUG_STATE
        .phase
        .store(11, Ordering::Release);
    display
        .set_hex(DISPLAY_BOOT_VALUE)
        .expect("display boot value should write");
    RP2350_DISPLAY_PCU_DEBUG_STATE
        .phase
        .store(12, Ordering::Release);
    RP2350_DISPLAY_PCU_DEBUG_STATE
        .value
        .store(u32::from(DISPLAY_BOOT_VALUE), Ordering::Release);
    RP2350_DISPLAY_PCU_DEBUG_STATE
        .direction
        .store(1, Ordering::Release);

    let mut value = u32::from(DISPLAY_BOOT_VALUE);
    let mut incrementing = true;
    let mut phase_steps = 0_u8;

    loop {
        RP2350_DISPLAY_PCU_DEBUG_STATE
            .phase
            .store(100, Ordering::Release);
        let _ = system_monotonic_time().sleep_for(Duration::from_millis(
            DISPLAY_STEP_PERIOD_MILLIS,
        ));
        RP2350_DISPLAY_PCU_DEBUG_STATE
            .phase
            .store(101, Ordering::Release);
        value = if incrementing {
            increment
                .process_word(value)
                .expect("increment stream should process one word")
        } else {
            decrement
                .process_word(value)
                .expect("decrement stream should process one word")
        };
        RP2350_DISPLAY_PCU_DEBUG_STATE
            .phase
            .store(102, Ordering::Release);
        display
            .set_hex(value as u16)
            .expect("display value should write");
        RP2350_DISPLAY_PCU_DEBUG_STATE
            .phase
            .store(103, Ordering::Release);
        RP2350_DISPLAY_PCU_DEBUG_STATE
            .value
            .store(value, Ordering::Release);
        RP2350_DISPLAY_PCU_DEBUG_STATE
            .direction
            .store(u32::from(incrementing), Ordering::Release);
        RP2350_DISPLAY_PCU_DEBUG_STATE
            .heartbeat
            .fetch_add(1, Ordering::AcqRel);
        phase_steps = phase_steps.wrapping_add(1);
        if phase_steps >= DISPLAY_DIRECTION_TOGGLE_STEPS {
            phase_steps = 0;
            incrementing = !incrementing;
            RP2350_DISPLAY_PCU_DEBUG_STATE
                .direction
                .store(u32::from(incrementing), Ordering::Release);
        }
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
