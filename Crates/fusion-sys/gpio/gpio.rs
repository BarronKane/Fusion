//! fusion-sys GPIO ownership and capability surfaces built on the selected fusion-pal driver.

use fusion_pal::sys::gpio::{
    GpioBase,
    GpioControl,
    GpioDriveStrengthPin,
    GpioFunctionPin,
    GpioInputPin as PalGpioInputPin,
    GpioOutputPin as PalGpioOutputPin,
    GpioOwnedPin as PalGpioOwnedPin,
    GpioPullPin,
    PlatformGpioPin,
    system_gpio as pal_system_gpio,
};
pub use fusion_pal::sys::gpio::{
    GpioCapabilities,
    GpioDriveStrength,
    GpioError,
    GpioErrorKind,
    GpioFunction,
    GpioImplementationKind,
    GpioPinControl,
    GpioPinDescriptor,
    GpioProviderCaps,
    GpioPull,
    GpioSupport,
};

/// Shared contract for one owned GPIO handle.
pub trait GpioOwnedPin {
    /// Returns the concrete backend pin number.
    fn pin(&self) -> u8;

    /// Returns one truthful capability snapshot for this pin.
    fn capabilities(&self) -> GpioCapabilities;
}

/// Output-capable GPIO contract consumed by simple components such as LEDs.
pub trait GpioOutputPin: GpioOwnedPin {
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
pub trait GpioInputPin: GpioOwnedPin {
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

/// Namespace for taking owned GPIO handles from the selected backend.
#[derive(Debug, Clone, Copy)]
pub struct Gpio;

/// Owned GPIO handle for the selected backend.
#[derive(Debug)]
pub struct GpioPin {
    inner: PlatformGpioPin,
}

impl Gpio {
    /// Reports the truthful GPIO surface for the selected backend.
    #[must_use]
    pub fn support() -> GpioSupport {
        GpioBase::support(&pal_system_gpio())
    }

    /// Returns the statically or dynamically surfaced GPIO pin descriptors.
    #[must_use]
    pub fn pins() -> &'static [GpioPinDescriptor] {
        GpioBase::pins(&pal_system_gpio())
    }

    /// Takes exclusive ownership of one GPIO pin.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when GPIO is unsupported, the pin is invalid, or the pin
    /// is already owned.
    pub fn take(pin: u8) -> Result<GpioPin, GpioError> {
        Ok(GpioPin {
            inner: GpioControl::take_pin(&pal_system_gpio(), pin)?,
        })
    }

    /// Returns one truthful capability snapshot for one backend pin number.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the pin does not exist.
    pub fn capabilities(pin: u8) -> Result<GpioCapabilities, GpioError> {
        GpioBase::capabilities(&pal_system_gpio(), pin)
    }
}

impl GpioPin {
    /// Returns the concrete pin number.
    #[must_use]
    pub fn pin(&self) -> u8 {
        PalGpioOwnedPin::pin(&self.inner)
    }

    /// Returns one truthful capability snapshot for this owned pin.
    #[must_use]
    pub fn capabilities(&self) -> GpioCapabilities {
        PalGpioOwnedPin::capabilities(&self.inner)
    }

    /// Selects one alternate-function mux setting for this pin.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the function cannot be selected.
    pub fn set_function(&mut self, function: GpioFunction) -> Result<(), GpioError> {
        GpioFunctionPin::set_function(&mut self.inner, function)
    }

    /// Configures this pin for input sampling.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when input mode cannot be realized.
    pub fn configure_input(&mut self) -> Result<(), GpioError> {
        PalGpioInputPin::configure_input(&mut self.inner)
    }

    /// Reads the current sampled input level.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the pin cannot be read.
    pub fn read(&self) -> Result<bool, GpioError> {
        PalGpioInputPin::read_level(&self.inner)
    }

    /// Configures this pin for software-controlled output.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when output mode cannot be realized.
    pub fn configure_output(&mut self, initial_high: bool) -> Result<(), GpioError> {
        PalGpioOutputPin::configure_output(&mut self.inner, initial_high)
    }

    /// Sets the logical output level.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the pin cannot be driven.
    pub fn set_level(&mut self, high: bool) -> Result<(), GpioError> {
        PalGpioOutputPin::set_level(&mut self.inner, high)
    }

    /// Selects the pad pull-resistor mode.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when pull configuration is unsupported.
    pub fn set_pull(&mut self, pull: GpioPull) -> Result<(), GpioError> {
        GpioPullPin::set_pull(&mut self.inner, pull)
    }

    /// Selects the pad drive strength.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when drive-strength control is unsupported.
    pub fn set_drive_strength(&mut self, strength: GpioDriveStrength) -> Result<(), GpioError> {
        GpioDriveStrengthPin::set_drive_strength(&mut self.inner, strength)
    }

    /// Releases the selected PAL-owned pin wrapper back to the caller.
    #[must_use]
    pub fn into_inner(self) -> PlatformGpioPin {
        self.inner
    }
}

impl GpioOwnedPin for GpioPin {
    fn pin(&self) -> u8 {
        self.pin()
    }

    fn capabilities(&self) -> GpioCapabilities {
        self.capabilities()
    }
}

impl GpioOutputPin for GpioPin {
    fn configure_output(&mut self, initial_high: bool) -> Result<(), GpioError> {
        self.configure_output(initial_high)
    }

    fn set_level(&mut self, high: bool) -> Result<(), GpioError> {
        self.set_level(high)
    }
}

impl GpioInputPin for GpioPin {
    fn configure_input(&mut self) -> Result<(), GpioError> {
        self.configure_input()
    }

    fn read_level(&self) -> Result<bool, GpioError> {
        self.read()
    }
}

impl From<PlatformGpioPin> for GpioPin {
    fn from(inner: PlatformGpioPin) -> Self {
        Self { inner }
    }
}

impl From<GpioPin> for PlatformGpioPin {
    fn from(pin: GpioPin) -> Self {
        pin.inner
    }
}

impl Default for Gpio {
    fn default() -> Self {
        Self
    }
}
