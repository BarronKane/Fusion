//! Firmware-orchestrated RP2350 GPIO driver binding.

use fd_bus_gpio::{
    Gpio as UniversalGpio,
    GpioBinding,
    GpioDriver,
    GpioDriverContext,
    GpioPin as UniversalGpioPin,
};
use fusion_hal::contract::drivers::bus::gpio::{
    GpioError,
    GpioSignalSource,
};
use fusion_hal::contract::drivers::driver::{
    DriverActivationContext,
    DriverDiscoveryContext,
    DriverError,
    DriverErrorKind,
    DriverRegistry,
};
use fusion_pal::sys::soc::drivers::bus::gpio::{
    GpioHardware,
    GpioPinHardware,
    primary_gpio_controller_id,
};

use crate::module::requested_driver_by_key;

const GPIO_DRIVER_KEY: &str = "bus.gpio";

/// Canonical selected GPIO provider type for the current firmware image.
pub type SystemGpio = UniversalGpio<GpioHardware>;
/// Canonical selected GPIO pin type for the current firmware image.
pub type SystemGpioPin = UniversalGpioPin<GpioPinHardware>;

/// Activates the selected GPIO driver and returns the canonical GPIO provider surface.
///
/// # Errors
///
/// Returns one honest GPIO error when the selected firmware image did not request the GPIO
/// driver module or the driver cannot activate for the selected SoC substrate.
pub fn system_gpio() -> Result<SystemGpio, GpioError> {
    system_gpio_by_controller_id(primary_gpio_controller_id())
}

/// Activates one selected GPIO controller by stable controller identifier.
///
/// # Errors
///
/// Returns one honest GPIO error when the controller is unavailable or activation fails.
pub fn system_gpio_by_controller_id(controller_id: &str) -> Result<SystemGpio, GpioError> {
    let _ = requested_driver_by_key(GPIO_DRIVER_KEY).map_err(map_driver_gpio)?;

    let mut registry = DriverRegistry::<1>::new();
    let registered = registry
        .register::<GpioDriver<GpioHardware>>()
        .map_err(map_driver_gpio)?;
    let mut driver_context = GpioDriverContext::<GpioHardware>::new();
    let mut bindings = [GpioBinding {
        provider: 0,
        controller_id: "",
    }; 4];
    let selected_binding = {
        let mut discovery = DriverDiscoveryContext::new(&mut driver_context);
        let count = registered
            .enumerate_bindings(&mut discovery, &mut bindings)
            .map_err(map_driver_gpio)?;
        if count == 0 {
            return Err(GpioError::unsupported());
        }
        bindings[..count]
            .iter()
            .copied()
            .find(|binding| binding.controller_id == controller_id)
            .ok_or_else(GpioError::unsupported)?
    };

    let mut activation = DriverActivationContext::new(&mut driver_context);
    registered
        .activate(&mut activation, selected_binding)
        .map_err(map_driver_gpio)
        .map(|driver| driver.into_instance())
}

/// Activates the GPIO controller referenced by one signal source.
///
/// # Errors
///
/// Returns one honest GPIO error when the controller cannot be activated.
pub fn system_gpio_for_signal(source: GpioSignalSource) -> Result<SystemGpio, GpioError> {
    system_gpio_by_controller_id(source.controller_id)
}

fn map_driver_gpio(error: DriverError) -> GpioError {
    match error.kind() {
        DriverErrorKind::Unsupported => GpioError::unsupported(),
        DriverErrorKind::Invalid => GpioError::invalid(),
        DriverErrorKind::Busy => GpioError::busy(),
        DriverErrorKind::ResourceExhausted => GpioError::resource_exhausted(),
        DriverErrorKind::StateConflict => GpioError::state_conflict(),
        DriverErrorKind::MissingContext | DriverErrorKind::WrongContextType => {
            GpioError::state_conflict()
        }
        DriverErrorKind::AlreadyRegistered => GpioError::state_conflict(),
        DriverErrorKind::Platform(code) => GpioError::platform(code),
    }
}
