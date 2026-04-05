//! Hardware-facing GPIO substrate contract consumed by the universal GPIO driver.

use fusion_hal::contract::drivers::bus::gpio::{
    GpioCapabilities,
    GpioDriveStrength,
    GpioError,
    GpioFunction,
    GpioPinDescriptor,
    GpioPull,
    GpioSupport,
};

/// Hardware-facing contract for one GPIO substrate implementation.
pub trait GpioHardware {
    /// Concrete hardware-owned pin handle surfaced by this substrate.
    type Pin: GpioHardwarePin;

    /// Reports the truthful GPIO surface for this substrate.
    fn support() -> GpioSupport;

    /// Returns the statically or dynamically surfaced GPIO pin descriptors.
    fn pins() -> &'static [GpioPinDescriptor];

    /// Claims one GPIO pin from the underlying substrate.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the pin is invalid, unsupported, or already claimed.
    fn claim_pin(pin: u8) -> Result<Self::Pin, GpioError>;
}

/// Hardware-facing contract for one owned GPIO pin.
pub trait GpioHardwarePin {
    /// Returns the concrete substrate pin number.
    fn pin(&self) -> u8;

    /// Returns the truthful capability snapshot for this pin.
    fn capabilities(&self) -> GpioCapabilities;

    /// Selects one alternate-function mux setting for this pin.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the function cannot be selected.
    fn set_function(&mut self, function: GpioFunction) -> Result<(), GpioError>;

    /// Configures this pin for software-controlled input sampling.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when input mode cannot be realized.
    fn configure_input(&mut self) -> Result<(), GpioError>;

    /// Reads the current sampled input level.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the pin cannot be read.
    fn read_level(&self) -> Result<bool, GpioError>;

    /// Configures this pin for software-controlled output.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when output mode cannot be realized.
    fn configure_output(&mut self, initial_high: bool) -> Result<(), GpioError>;

    /// Sets the logical output level.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the pin cannot be driven.
    fn set_level(&mut self, high: bool) -> Result<(), GpioError>;

    /// Selects the pull-resistor mode for this pin.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when pull control is unsupported or invalid.
    fn set_pull(&mut self, pull: GpioPull) -> Result<(), GpioError>;

    /// Selects one drive-strength mode for this pin.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when drive-strength control is unsupported or invalid.
    fn set_drive_strength(&mut self, strength: GpioDriveStrength) -> Result<(), GpioError>;
}
