//! Backend-neutral unsupported generic GPIO implementation.

use super::{
    GpioBaseContract,
    GpioCapabilities,
    GpioControlContract,
    GpioDriveStrength,
    GpioDriveStrengthPinContract,
    GpioError,
    GpioFunction,
    GpioFunctionPinContract,
    GpioInputPinContract,
    GpioOutputPinContract,
    GpioPinDescriptor,
    GpioPull,
    GpioPullPinContract,
    GpioSupport,
};

/// Unsupported generic GPIO provider placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedGpio;

/// Unsupported owned GPIO placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedGpioPin {
    pin: u8,
}

impl UnsupportedGpio {
    /// Creates a new unsupported generic GPIO provider placeholder.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Takes one pin from the unsupported backend.
    ///
    /// # Errors
    ///
    /// Always returns one unsupported error.
    pub const fn take(_pin: u8) -> Result<UnsupportedGpioPin, GpioError> {
        Err(GpioError::unsupported())
    }

    /// Returns the truthful capability snapshot for one unsupported pin number.
    ///
    /// # Errors
    ///
    /// Always returns one unsupported error.
    pub const fn capabilities(_pin: u8) -> Result<GpioCapabilities, GpioError> {
        Err(GpioError::unsupported())
    }

    /// Returns the statically surfaced unsupported pin catalog.
    #[must_use]
    pub const fn pins() -> &'static [GpioPinDescriptor] {
        &[]
    }
}

impl UnsupportedGpioPin {
    /// Creates one unsupported owned-pin placeholder for one pin number.
    #[must_use]
    pub const fn new(pin: u8) -> Self {
        Self { pin }
    }

    /// Returns the concrete pin number.
    #[must_use]
    pub const fn pin(&self) -> u8 {
        self.pin
    }

    /// Returns the capability snapshot for this unsupported pin.
    #[must_use]
    pub const fn capabilities(&self) -> GpioCapabilities {
        GpioCapabilities::empty()
    }

    /// Selects one alternate-function mux setting for this unsupported pin.
    ///
    /// # Errors
    ///
    /// Always returns one unsupported error.
    pub const fn set_function(&mut self, _function: GpioFunction) -> Result<(), GpioError> {
        Err(GpioError::unsupported())
    }

    /// Configures this unsupported pin for input sampling.
    ///
    /// # Errors
    ///
    /// Always returns one unsupported error.
    pub const fn configure_input(&mut self) -> Result<(), GpioError> {
        Err(GpioError::unsupported())
    }

    /// Reads the current sampled input level for this unsupported pin.
    ///
    /// # Errors
    ///
    /// Always returns one unsupported error.
    pub const fn read(&self) -> Result<bool, GpioError> {
        Err(GpioError::unsupported())
    }

    /// Configures this unsupported pin for output.
    ///
    /// # Errors
    ///
    /// Always returns one unsupported error.
    pub const fn configure_output(&mut self, _initial_high: bool) -> Result<(), GpioError> {
        Err(GpioError::unsupported())
    }

    /// Sets the logical output level for this unsupported pin.
    ///
    /// # Errors
    ///
    /// Always returns one unsupported error.
    pub const fn set_level(&mut self, _high: bool) -> Result<(), GpioError> {
        Err(GpioError::unsupported())
    }

    /// Selects the pull-resistor mode for this unsupported pin.
    ///
    /// # Errors
    ///
    /// Always returns one unsupported error.
    pub const fn set_pull(&mut self, _pull: GpioPull) -> Result<(), GpioError> {
        Err(GpioError::unsupported())
    }

    /// Selects one drive-strength mode for this unsupported pin.
    ///
    /// # Errors
    ///
    /// Always returns one unsupported error.
    pub const fn set_drive_strength(
        &mut self,
        _strength: GpioDriveStrength,
    ) -> Result<(), GpioError> {
        Err(GpioError::unsupported())
    }
}

impl GpioBaseContract for UnsupportedGpio {
    fn support(&self) -> GpioSupport {
        GpioSupport::unsupported()
    }

    fn pins(&self) -> &'static [GpioPinDescriptor] {
        &[]
    }
}

impl GpioControlContract for UnsupportedGpio {
    type Pin = UnsupportedGpioPin;

    fn take_pin(&self, _pin: u8) -> Result<Self::Pin, GpioError> {
        Err(GpioError::unsupported())
    }
}

impl super::GpioOwnedPinContract for UnsupportedGpioPin {
    fn pin(&self) -> u8 {
        self.pin()
    }

    fn capabilities(&self) -> GpioCapabilities {
        self.capabilities()
    }
}

impl GpioFunctionPinContract for UnsupportedGpioPin {
    fn set_function(&mut self, function: GpioFunction) -> Result<(), GpioError> {
        self.set_function(function)
    }
}

impl GpioPullPinContract for UnsupportedGpioPin {
    fn set_pull(&mut self, pull: GpioPull) -> Result<(), GpioError> {
        self.set_pull(pull)
    }
}

impl GpioDriveStrengthPinContract for UnsupportedGpioPin {
    fn set_drive_strength(&mut self, strength: GpioDriveStrength) -> Result<(), GpioError> {
        self.set_drive_strength(strength)
    }
}

impl GpioOutputPinContract for UnsupportedGpioPin {
    fn configure_output(&mut self, initial_high: bool) -> Result<(), GpioError> {
        self.configure_output(initial_high)
    }

    fn set_level(&mut self, high: bool) -> Result<(), GpioError> {
        self.set_level(high)
    }
}

impl GpioInputPinContract for UnsupportedGpioPin {
    fn configure_input(&mut self) -> Result<(), GpioError> {
        self.configure_input()
    }

    fn read_level(&self) -> Result<bool, GpioError> {
        self.read()
    }
}
