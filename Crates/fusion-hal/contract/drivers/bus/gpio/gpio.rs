//! DriverContract-facing GPIO contract vocabulary.

mod caps;
mod error;
mod types;
mod unsupported;

pub use caps::*;
pub use error::*;
pub use types::*;
pub use unsupported::*;

/// Capability trait for generic GPIO backends.
pub trait GpioBaseContract {
    /// Returns the stable controller/provider identity for this GPIO surface.
    fn controller(&self) -> &'static GpioControllerDescriptor;

    /// Reports the truthful GPIO surface for this backend.
    fn support(&self) -> GpioSupport;

    /// Returns the statically or dynamically surfaced GPIO pin descriptors.
    #[must_use]
    fn pins(&self) -> &'static [GpioPinDescriptor];

    /// Returns the truthful capability snapshot for one pin number.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the pin does not exist or the backend cannot characterize it.
    fn capabilities(&self, pin: u8) -> Result<GpioCapabilities, GpioError> {
        self.pins()
            .iter()
            .find(|descriptor| descriptor.pin == pin)
            .map(|descriptor| descriptor.capabilities)
            .ok_or_else(GpioError::invalid)
    }
}

/// Control contract for generic GPIO backends.
pub trait GpioControlContract: GpioBaseContract {
    /// Concrete owned-pin handle returned by this backend.
    type Pin: GpioPinControlContract;

    /// Takes one pin exclusively.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the pin is invalid, unsupported, or already claimed.
    fn take_pin(&self, pin: u8) -> Result<Self::Pin, GpioError>;
}

/// Shared contract for one owned GPIO handle.
pub trait GpioOwnedPinContract {
    /// Returns the stable controller/provider identity for this owned pin.
    fn controller(&self) -> &'static GpioControllerDescriptor;

    /// Returns the concrete backend pin number.
    fn pin(&self) -> u8;

    /// Returns one truthful capability snapshot for this pin.
    fn capabilities(&self) -> GpioCapabilities;
}

/// Alternate-function control for one owned GPIO handle.
pub trait GpioFunctionPinContract: GpioOwnedPinContract {
    /// Selects one alternate-function mux setting for this pin.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the function cannot be selected.
    fn set_function(&mut self, function: GpioFunction) -> Result<(), GpioError>;
}

/// Pull-resistor control for one owned GPIO handle.
pub trait GpioPullPinContract: GpioOwnedPinContract {
    /// Selects the pull-resistor mode for this pin.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when pull control is unsupported or invalid.
    fn set_pull(&mut self, pull: GpioPull) -> Result<(), GpioError>;
}

/// Drive-strength control for one owned GPIO handle.
pub trait GpioDriveStrengthPinContract: GpioOwnedPinContract {
    /// Selects one drive-strength mode for this pin.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when drive-strength control is unsupported or invalid.
    fn set_drive_strength(&mut self, strength: GpioDriveStrength) -> Result<(), GpioError>;
}

/// Output-capable GPIO contract consumed by simple components such as LEDs.
pub trait GpioOutputPinContract: GpioOwnedPinContract {
    /// Configures this pin for software-controlled output.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when output configuration cannot be realized.
    fn configure_output(&mut self, initial_high: bool) -> Result<(), GpioError>;

    /// Sets the logical output level.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the pin cannot be driven.
    fn set_level(&mut self, high: bool) -> Result<(), GpioError>;
}

/// Input-capable GPIO contract consumed by simple components such as buttons.
pub trait GpioInputPinContract: GpioOwnedPinContract {
    /// Configures this pin for software-controlled input sampling.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when input configuration cannot be realized.
    fn configure_input(&mut self) -> Result<(), GpioError>;

    /// Reads the current sampled input level.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the pin cannot be read.
    fn read_level(&self) -> Result<bool, GpioError>;
}

/// Full configuration/control surface for one owned GPIO pin.
pub trait GpioPinControlContract:
    GpioOwnedPinContract
    + GpioFunctionPinContract
    + GpioPullPinContract
    + GpioDriveStrengthPinContract
    + GpioOutputPinContract
    + GpioInputPinContract
{
}

impl<T> GpioPinControlContract for T where
    T: GpioOwnedPinContract
        + GpioFunctionPinContract
        + GpioPullPinContract
        + GpioDriveStrengthPinContract
        + GpioOutputPinContract
        + GpioInputPinContract
{
}
