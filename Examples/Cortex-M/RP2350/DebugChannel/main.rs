#![no_std]
#![no_main]

//! Pico 2 W USB device-controller smoke test with seven-segment status output.
//!
//! Board layout:
//! - `GP11` -> panic/fault LED (standalone red LED)
//! - `GP12` -> display serial data
//! - `GP13` -> display output enable
//! - `GP14` -> display latch
//! - `GP15` -> display shift clock
//!
//! Status codes:
//! - `C100` startup
//! - `C110` display path alive
//! - `D100` USB driver bound
//! - `D110` USB device controller ready
//! - `D120` USB host observed
//! - `D12F` USB configured
//! - `D130` bulk loopback armed
//! - `D13F` bulk loopback observed
//! - `E1xx` display failure
//! - `E12x` USB failure

use core::mem::MaybeUninit;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU8, Ordering};

use cortex_m_rt::{
    ExceptionFrame,
    exception,
};
use fusion_example_rp2350_on_device::runtime::wait_for_runtime_progress;
use fusion_example_rp2350_on_device::seven_segment_timer::Rp2350TimerFourDigitSevenSegmentDisplay;
use fusion_firmware::sys::hal::drivers::bus::gpio::{
    SystemGpioPin,
    system_gpio,
};
use fusion_firmware::sys::hal::drivers::bus::usb::{
    system_usb_device_controller,
};
use fusion_hal::contract::drivers::bus::gpio::{
    GpioDriveStrength,
    GpioError,
};
use fusion_hal::contract::drivers::bus::usb::{
    UsbDeviceControllerContract,
    UsbEndpointAddress,
    UsbEndpointNumber,
    UsbErrorKind,
    UsbDeviceState,
    UsbDirection,
};

fusion_example_rp2350_on_device::fusion_rp2350_export_build_id!();

const DISPLAY_DATA_PIN: u8 = 12;
const DISPLAY_ENABLE_PIN: u8 = 13;
const DISPLAY_LATCH_PIN: u8 = 14;
const DISPLAY_SHIFT_CLOCK_PIN: u8 = 15;
const PANIC_LED_PIN: u8 = 11;

const STATUS_STARTUP: u16 = 0xC100;
const STATUS_DISPLAY_READY: u16 = 0xC110;
const STATUS_USB_BOUND: u16 = 0xD100;
const STATUS_USB_DEVICE_READY: u16 = 0xD110;
const STATUS_USB_HOST_OBSERVED: u16 = 0xD120;
const STATUS_USB_CONFIGURED: u16 = 0xD12F;
const STATUS_USB_BULK_READY: u16 = 0xD130;
const STATUS_USB_BULK_ACTIVITY: u16 = 0xD13F;
const STATUS_ERROR_USB_BIND: u16 = 0xE120;
const STATUS_ERROR_USB_TRANSFER: u16 = 0xE121;

const PANIC_LED_UNINITIALIZED: u8 = 0;
const PANIC_LED_READY: u8 = 1;
const PANIC_LED_FAILED: u8 = 2;

static mut PANIC_LED_STORAGE: MaybeUninit<SystemGpioPin> = MaybeUninit::uninit();
static PANIC_LED_STATE: AtomicU8 = AtomicU8::new(PANIC_LED_UNINITIALIZED);

const DEBUG_BULK_OUT_ENDPOINT: UsbEndpointAddress = UsbEndpointAddress {
    number: UsbEndpointNumber(1),
    direction: UsbDirection::Out,
};

const DEBUG_BULK_IN_ENDPOINT: UsbEndpointAddress = UsbEndpointAddress {
    number: UsbEndpointNumber(1),
    direction: UsbDirection::In,
};

fn panic_led_on() -> ! {
    #[cfg(all(target_arch = "arm", target_os = "none"))]
    unsafe {
        core::arch::asm!("cpsid i", options(nomem, nostack, preserves_flags));
    }
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
                (&mut *core::ptr::addr_of_mut!(PANIC_LED_STORAGE)).write(pin);
            }
            PANIC_LED_STATE.store(PANIC_LED_READY, Ordering::Release);
            unsafe { Ok((&mut *core::ptr::addr_of_mut!(PANIC_LED_STORAGE)).assume_init_mut()) }
        }
    }
}

fn set_panic_led(on: bool) -> Result<(), ()> {
    let pin = panic_led_pin()?;
    pin.set_level(on).map_err(|_| ())
}

fn build_display() -> Result<Rp2350TimerFourDigitSevenSegmentDisplay, GpioError> {
    Rp2350TimerFourDigitSevenSegmentDisplay::common_cathode(
        DISPLAY_DATA_PIN,
        DISPLAY_ENABLE_PIN,
        DISPLAY_LATCH_PIN,
        DISPLAY_SHIFT_CLOCK_PIN,
    )
}

fn set_status(display: &Rp2350TimerFourDigitSevenSegmentDisplay, code: u16) {
    let _ = display.set_hex(code);
}

fn fatal_status(display: &Rp2350TimerFourDigitSevenSegmentDisplay, code: u16) -> ! {
    set_status(display, code);
    panic_led_on()
}

fn status_for_usb_state(state: UsbDeviceState) -> u16 {
    match state {
        UsbDeviceState::Configured => STATUS_USB_CONFIGURED,
        UsbDeviceState::Default | UsbDeviceState::Addressed | UsbDeviceState::Suspended => {
            STATUS_USB_HOST_OBSERVED
        }
        UsbDeviceState::Detached | UsbDeviceState::Attached | UsbDeviceState::Powered => {
            STATUS_USB_DEVICE_READY
        }
    }
}

fn update_usb_state_status(
    usb: &mut impl UsbDeviceControllerContract,
    display: &Rp2350TimerFourDigitSevenSegmentDisplay,
) -> UsbDeviceState {
    let state = usb.device_state();
    set_status(display, status_for_usb_state(state));
    state
}

fn wait_for_usb_configuration(
    usb: &mut impl UsbDeviceControllerContract,
    display: &Rp2350TimerFourDigitSevenSegmentDisplay,
) {
    set_status(display, STATUS_USB_DEVICE_READY);
    let mut saw_host = false;

    loop {
        let state = usb.device_state();

        match state {
            UsbDeviceState::Configured => {
                set_status(display, STATUS_USB_CONFIGURED);
                return;
            }
            UsbDeviceState::Default | UsbDeviceState::Addressed | UsbDeviceState::Suspended => {
                saw_host = true;
                set_status(display, STATUS_USB_HOST_OBSERVED);
            }
            UsbDeviceState::Attached | UsbDeviceState::Powered if saw_host => {
                set_status(display, STATUS_USB_HOST_OBSERVED);
            }
            _ => {
                set_status(display, STATUS_USB_DEVICE_READY);
            }
        }

        wait_for_runtime_progress();
    }
}

fn pump_usb_loopback(
    usb: &mut impl UsbDeviceControllerContract,
    display: &Rp2350TimerFourDigitSevenSegmentDisplay,
    pending: &mut [u8; 64],
    pending_len: &mut usize,
    saw_bulk_activity: &mut bool,
) -> Result<(), ()> {
    if *pending_len != 0 {
        match usb.queue_in(DEBUG_BULK_IN_ENDPOINT, &pending[..*pending_len]) {
            Ok(()) => {
                *pending_len = 0;
                *saw_bulk_activity = true;
            }
            Err(error) if matches!(error.kind(), UsbErrorKind::Busy) => {}
            Err(_) => return Err(()),
        }
    }

    if *pending_len == 0 {
        match usb.dequeue_out(DEBUG_BULK_OUT_ENDPOINT, pending) {
            Ok(payload) => {
                if !payload.is_empty() {
                    *pending_len = payload.len();
                }
            }
            Err(error) if matches!(error.kind(), UsbErrorKind::Busy) => {}
            Err(_) => return Err(()),
        }
    }

    if *saw_bulk_activity || *pending_len != 0 {
        set_status(display, STATUS_USB_BULK_ACTIVITY);
    } else {
        set_status(display, STATUS_USB_BULK_READY);
    }

    Ok(())
}

#[fusion_firmware::fusion_firmware_main]
fn main() -> ! {
    let display = match build_display() {
        Ok(display) => display,
        Err(_) => panic_led_on(),
    };

    set_status(&display, STATUS_STARTUP);
    set_status(&display, STATUS_DISPLAY_READY);

    let mut usb = match system_usb_device_controller() {
        Ok(usb) => usb,
        Err(_) => fatal_status(&display, STATUS_ERROR_USB_BIND),
    };

    set_status(&display, STATUS_USB_BOUND);
    wait_for_usb_configuration(&mut usb, &display);

    let mut pending_loopback = [0_u8; 64];
    let mut pending_loopback_len = 0;
    let mut saw_bulk_activity = false;

    loop {
        let state = update_usb_state_status(&mut usb, &display);
        if matches!(state, UsbDeviceState::Configured)
            && pump_usb_loopback(
                &mut usb,
                &display,
                &mut pending_loopback,
                &mut pending_loopback_len,
                &mut saw_bulk_activity,
            )
            .is_err()
        {
            fatal_status(&display, STATUS_ERROR_USB_TRANSFER);
        } else if matches!(state, UsbDeviceState::Configured) {
            set_status(
                &display,
                if saw_bulk_activity || pending_loopback_len != 0 {
                    STATUS_USB_BULK_ACTIVITY
                } else {
                    STATUS_USB_BULK_READY
                },
            );
        } else {
            pending_loopback_len = 0;
            saw_bulk_activity = false;
        }
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
