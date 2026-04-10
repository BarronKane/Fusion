//! Firmware-orchestrated RP2350 USB driver binding.

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{
    AtomicU8,
    Ordering,
};

use fd_bus_usb::{
    Usb as UniversalUsb,
    UsbBinding,
    UsbDriver,
    UsbDriverContext,
};
use fusion_hal::contract::drivers::bus::usb::{
    UsbError,
    UsbErrorKind,
};
use fusion_hal::contract::drivers::driver::{
    DriverActivationContext,
    DriverDiscoveryContext,
    DriverError,
    DriverErrorKind,
    DriverRegistry,
};
use fusion_pal::sys::soc::drivers::bus::usb::{
    USB_RUNTIME_IRQN,
    UsbDeviceController,
    UsbHardware,
    service_runtime_irq,
};
use fusion_std::thread::{
    RedInterrupt,
    RedInterruptConfig,
};

use crate::module::requested_driver_by_key;

const USB_DRIVER_KEY: &str = "bus.usb";
const USB_RUNTIME_IRQ_PRIORITY: u8 = 0x90;
const USB_RUNTIME_IRQ_UNBOUND: u8 = 0;
const USB_RUNTIME_IRQ_BINDING: u8 = 1;
const USB_RUNTIME_IRQ_BOUND: u8 = 2;
const USB_PROVIDER_UNINITIALIZED: u8 = 0;
const USB_PROVIDER_INITIALIZING: u8 = 1;
const USB_PROVIDER_READY: u8 = 2;

/// Canonical selected USB provider type for the current firmware image.
pub type SystemUsb = UniversalUsb<UsbHardware>;
/// Canonical selected USB device-controller surface for the current firmware image.
pub type SystemUsbDeviceController = UsbDeviceController;

static USB_RUNTIME_IRQ_STATE: AtomicU8 = AtomicU8::new(USB_RUNTIME_IRQ_UNBOUND);
static USB_PROVIDER_SLOT: UsbProviderSlot = UsbProviderSlot::new();

struct UsbProviderSlot {
    state: AtomicU8,
    value: UnsafeCell<MaybeUninit<SystemUsb>>,
}

impl UsbProviderSlot {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(USB_PROVIDER_UNINITIALIZED),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    fn get_or_try_init(&self) -> Result<SystemUsb, UsbError> {
        loop {
            match self.state.load(Ordering::Acquire) {
                USB_PROVIDER_READY => {
                    return Ok(unsafe { *(*self.value.get()).as_ptr() });
                }
                USB_PROVIDER_UNINITIALIZED => {
                    if self
                        .state
                        .compare_exchange(
                            USB_PROVIDER_UNINITIALIZED,
                            USB_PROVIDER_INITIALIZING,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_err()
                    {
                        continue;
                    }

                    match activate_system_usb_provider() {
                        Ok(provider) => {
                            unsafe { (*self.value.get()).write(provider) };
                            self.state.store(USB_PROVIDER_READY, Ordering::Release);
                            return Ok(provider);
                        }
                        Err(error) => {
                            self.state
                                .store(USB_PROVIDER_UNINITIALIZED, Ordering::Release);
                            return Err(error);
                        }
                    }
                }
                USB_PROVIDER_INITIALIZING => core::hint::spin_loop(),
                _ => return Err(UsbError::state_conflict()),
            }
        }
    }
}

unsafe impl Sync for UsbProviderSlot {}

/// Activates the selected USB driver and returns the canonical USB provider surface.
///
/// # Errors
///
/// Returns one honest USB error when the selected firmware image did not request the USB driver
/// module or the RP2350 SoC cannot surface the USB controller honestly.
pub fn system_usb() -> Result<SystemUsb, UsbError> {
    USB_PROVIDER_SLOT.get_or_try_init()
}

fn activate_system_usb_provider() -> Result<SystemUsb, UsbError> {
    let _ = requested_driver_by_key(USB_DRIVER_KEY).map_err(map_driver_usb)?;

    let mut registry = DriverRegistry::<1>::new();
    let registered = registry
        .register::<UsbDriver<UsbHardware>>()
        .map_err(map_driver_usb)?;
    let mut driver_context = UsbDriverContext::<UsbHardware>::new();
    let mut bindings = [UsbBinding { provider: 0 }];

    {
        let mut discovery = DriverDiscoveryContext::new(&mut driver_context);
        let count = registered
            .enumerate_bindings(&mut discovery, &mut bindings)
            .map_err(map_driver_usb)?;
        if count == 0 {
            return Err(UsbError::unsupported());
        }
    }

    let mut activation = DriverActivationContext::new(&mut driver_context);
    let driver = registered
        .activate(&mut activation, bindings[0])
        .map_err(map_driver_usb)?
        .into_instance();

    let _ = SystemUsb::device_controller()?.ok_or_else(UsbError::unsupported)?;
    Ok(driver)
}

/// Returns the selected USB device-controller surface for the current firmware image.
///
/// # Errors
///
/// Returns one honest USB error when the firmware image did not select the USB driver or the
/// RP2350 USB controller cannot realize device mode honestly.
pub fn system_usb_device_controller() -> Result<SystemUsbDeviceController, UsbError> {
    let _ = system_usb()?;
    ensure_usb_runtime_irq_bound()?;
    SystemUsb::device_controller()?.ok_or_else(UsbError::unsupported)
}

unsafe extern "C" fn system_usb_runtime_irq_handler() {
    // Runtime-owned USB IRQ service is best-effort in-handler; bind-time policy already
    // guarantees that this line belongs to the selected USB controller path.
    let _ = service_runtime_irq(usb_runtime_irqn_i16());
}

fn usb_runtime_irqn_i16() -> i16 {
    i16::try_from(USB_RUNTIME_IRQN).expect("USB runtime IRQ line should fit in i16")
}

fn ensure_usb_runtime_irq_bound() -> Result<(), UsbError> {
    loop {
        match USB_RUNTIME_IRQ_STATE.load(Ordering::Acquire) {
            USB_RUNTIME_IRQ_BOUND => return Ok(()),
            USB_RUNTIME_IRQ_BINDING => core::hint::spin_loop(),
            USB_RUNTIME_IRQ_UNBOUND => {
                if USB_RUNTIME_IRQ_STATE
                    .compare_exchange(
                        USB_RUNTIME_IRQ_UNBOUND,
                        USB_RUNTIME_IRQ_BINDING,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_err()
                {
                    continue;
                }

                match RedInterrupt::bind_runtime_owned(
                    &RedInterruptConfig::new(USB_RUNTIME_IRQN)
                        .with_priority(USB_RUNTIME_IRQ_PRIORITY)
                        .with_enable_on_bind(true),
                    system_usb_runtime_irq_handler,
                ) {
                    Ok(_) => {
                        USB_RUNTIME_IRQ_STATE.store(USB_RUNTIME_IRQ_BOUND, Ordering::Release);
                        return Ok(());
                    }
                    Err(error) => {
                        USB_RUNTIME_IRQ_STATE.store(USB_RUNTIME_IRQ_UNBOUND, Ordering::Release);
                        return Err(match error.kind() {
                            fusion_sys::thread::ThreadErrorKind::Unsupported => {
                                UsbError::unsupported()
                            }
                            fusion_sys::thread::ThreadErrorKind::Invalid
                            | fusion_sys::thread::ThreadErrorKind::PermissionDenied
                            | fusion_sys::thread::ThreadErrorKind::PlacementDenied
                            | fusion_sys::thread::ThreadErrorKind::SchedulerDenied
                            | fusion_sys::thread::ThreadErrorKind::StackDenied
                            | fusion_sys::thread::ThreadErrorKind::Platform(_) => {
                                UsbError::invalid()
                            }
                            fusion_sys::thread::ThreadErrorKind::Busy
                            | fusion_sys::thread::ThreadErrorKind::Timeout => UsbError::busy(),
                            fusion_sys::thread::ThreadErrorKind::ResourceExhausted => {
                                UsbError::resource_exhausted()
                            }
                            fusion_sys::thread::ThreadErrorKind::StateConflict => {
                                UsbError::state_conflict()
                            }
                        });
                    }
                }
            }
            _ => return Err(UsbError::state_conflict()),
        }
    }
}

fn map_driver_usb(error: DriverError) -> UsbError {
    match error.kind() {
        DriverErrorKind::Unsupported => UsbError::unsupported(),
        DriverErrorKind::Invalid => UsbError::invalid(),
        DriverErrorKind::Busy => UsbError::busy(),
        DriverErrorKind::ResourceExhausted => UsbError::resource_exhausted(),
        DriverErrorKind::StateConflict => UsbError::state_conflict(),
        DriverErrorKind::MissingContext | DriverErrorKind::WrongContextType => {
            UsbError::state_conflict()
        }
        DriverErrorKind::AlreadyRegistered => UsbError::state_conflict(),
        DriverErrorKind::Platform(code) => UsbError::platform(code),
    }
}

/// Encodes one USB error into a stable debug breadcrumb word.
#[must_use]
pub fn encode_usb_error(error: UsbError) -> u32 {
    match error.kind() {
        UsbErrorKind::Unsupported => 1,
        UsbErrorKind::Invalid => 2,
        UsbErrorKind::Busy => 3,
        UsbErrorKind::Disconnected => 4,
        UsbErrorKind::Timeout => 5,
        UsbErrorKind::Stall => 6,
        UsbErrorKind::Protocol => 7,
        UsbErrorKind::Overcurrent => 8,
        UsbErrorKind::StateConflict => 9,
        UsbErrorKind::ResourceExhausted => 10,
        UsbErrorKind::Platform(code) => 0x8000_0000 | code as u32,
    }
}
