//! Unsupported hardware-facing GPIO substrate used when no SoC or interface backend is selected.

use fusion_hal::contract::drivers::bus::gpio::{
    GpioCapabilities,
    GpioControllerDescriptor,
    GpioDriveStrength,
    GpioError,
    GpioFunction,
    GpioPinDescriptor,
    GpioPull,
    GpioSupport,
};
use crate::interface::contract::{
    GpioHardware,
    GpioHardwarePin,
};

/// Unsupported GPIO hardware substrate.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedGpioHardware;

/// Unsupported GPIO hardware-owned pin placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedGpioPinHardware {
    pin: u8,
}

impl GpioHardware for UnsupportedGpioHardware {
    type Pin = UnsupportedGpioPinHardware;

    fn provider_count() -> u8 {
        0
    }

    fn controller(_provider: u8) -> Option<&'static GpioControllerDescriptor> {
        None
    }

    fn support(_provider: u8) -> GpioSupport {
        GpioSupport::unsupported()
    }

    fn pins(_provider: u8) -> &'static [GpioPinDescriptor] {
        &[]
    }

    fn claim_pin(_provider: u8, _pin: u8) -> Result<Self::Pin, GpioError> {
        Err(GpioError::unsupported())
    }
}

impl GpioHardwarePin for UnsupportedGpioPinHardware {
    fn controller(&self) -> &'static GpioControllerDescriptor {
        const CONTROLLER: GpioControllerDescriptor = GpioControllerDescriptor {
            id: "unsupported-gpio",
            name: "Unsupported GPIO",
        };
        &CONTROLLER
    }

    fn pin(&self) -> u8 {
        self.pin
    }

    fn capabilities(&self) -> GpioCapabilities {
        GpioCapabilities::empty()
    }

    fn set_function(&mut self, _function: GpioFunction) -> Result<(), GpioError> {
        Err(GpioError::unsupported())
    }

    fn configure_input(&mut self) -> Result<(), GpioError> {
        Err(GpioError::unsupported())
    }

    fn read_level(&self) -> Result<bool, GpioError> {
        Err(GpioError::unsupported())
    }

    fn configure_output(&mut self, _initial_high: bool) -> Result<(), GpioError> {
        Err(GpioError::unsupported())
    }

    fn set_level(&mut self, _high: bool) -> Result<(), GpioError> {
        Err(GpioError::unsupported())
    }

    fn set_pull(&mut self, _pull: GpioPull) -> Result<(), GpioError> {
        Err(GpioError::unsupported())
    }

    fn set_drive_strength(&mut self, _strength: GpioDriveStrength) -> Result<(), GpioError> {
        Err(GpioError::unsupported())
    }
}
