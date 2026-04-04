//! Universal GPIO bus-driver implementation layered over a hardware-facing GPIO substrate.

use core::marker::PhantomData;

pub use crate::contract::drivers::bus::gpio::*;

pub mod contract;
mod unsupported;

pub use contract::{
    GpioHardware,
    GpioHardwarePin,
};

/// Universal GPIO provider composed over one selected hardware-facing GPIO substrate.
#[derive(Debug, Clone, Copy, Default)]
pub struct Gpio<H: GpioHardware = unsupported::UnsupportedGpioHardware> {
    _hardware: PhantomData<H>,
}

/// Universal owned GPIO pin composed over one selected hardware-facing GPIO substrate.
#[derive(Debug)]
pub struct GpioPin<P: GpioHardwarePin = unsupported::UnsupportedGpioPinHardware> {
    inner: P,
}

impl<H> Gpio<H>
where
    H: GpioHardware,
{
    /// Creates a new universal GPIO provider handle over one selected hardware substrate.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            _hardware: PhantomData,
        }
    }

    /// Takes one pin exclusively.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the pin is invalid, unsupported, or already claimed.
    pub fn take(pin: u8) -> Result<GpioPin<H::Pin>, GpioError> {
        Ok(GpioPin {
            inner: H::claim_pin(pin)?,
        })
    }

    /// Returns one truthful capability snapshot for one pin number.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the pin does not exist.
    pub fn capabilities(pin: u8) -> Result<GpioCapabilities, GpioError> {
        GpioBase::capabilities(&Self::new(), pin)
    }

    /// Returns the statically or dynamically surfaced GPIO pin catalog.
    #[must_use]
    pub fn pins() -> &'static [GpioPinDescriptor] {
        H::pins()
    }
}

impl<P> GpioPin<P>
where
    P: GpioHardwarePin,
{
    /// Returns the concrete pin number.
    #[must_use]
    pub fn pin(&self) -> u8 {
        self.inner.pin()
    }

    /// Returns one truthful capability snapshot for this owned pin.
    #[must_use]
    pub fn capabilities(&self) -> GpioCapabilities {
        self.inner.capabilities()
    }

    /// Selects one alternate-function mux setting for this pin.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the function cannot be selected.
    pub fn set_function(&mut self, function: GpioFunction) -> Result<(), GpioError> {
        self.inner.set_function(function)
    }

    /// Configures this pin for software-controlled input sampling.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when input mode cannot be realized.
    pub fn configure_input(&mut self) -> Result<(), GpioError> {
        self.inner.configure_input()
    }

    /// Reads the current sampled input level.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the pin cannot be read.
    pub fn read(&self) -> Result<bool, GpioError> {
        self.inner.read_level()
    }

    /// Configures this pin for software-controlled output.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when output mode cannot be realized.
    pub fn configure_output(&mut self, initial_high: bool) -> Result<(), GpioError> {
        self.inner.configure_output(initial_high)
    }

    /// Sets the logical output level.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the pin cannot be driven.
    pub fn set_level(&mut self, high: bool) -> Result<(), GpioError> {
        self.inner.set_level(high)
    }

    /// Selects the pull-resistor mode for this pin.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when pull control is unsupported or invalid.
    pub fn set_pull(&mut self, pull: GpioPull) -> Result<(), GpioError> {
        self.inner.set_pull(pull)
    }

    /// Selects one drive-strength mode for this pin.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when drive-strength control is unsupported or invalid.
    pub fn set_drive_strength(&mut self, strength: GpioDriveStrength) -> Result<(), GpioError> {
        self.inner.set_drive_strength(strength)
    }

    /// Releases the hardware-facing pin handle back to the caller.
    #[must_use]
    pub fn into_inner(self) -> P {
        self.inner
    }
}

impl<H> GpioBase for Gpio<H>
where
    H: GpioHardware,
{
    fn support(&self) -> GpioSupport {
        H::support()
    }

    fn pins(&self) -> &'static [GpioPinDescriptor] {
        H::pins()
    }
}

impl<H> GpioControl for Gpio<H>
where
    H: GpioHardware,
{
    type Pin = GpioPin<H::Pin>;

    fn take_pin(&self, pin: u8) -> Result<Self::Pin, GpioError> {
        Self::take(pin)
    }
}

impl<P> GpioOwnedPin for GpioPin<P>
where
    P: GpioHardwarePin,
{
    fn pin(&self) -> u8 {
        self.pin()
    }

    fn capabilities(&self) -> GpioCapabilities {
        self.capabilities()
    }
}

impl<P> GpioFunctionPin for GpioPin<P>
where
    P: GpioHardwarePin,
{
    fn set_function(&mut self, function: GpioFunction) -> Result<(), GpioError> {
        self.set_function(function)
    }
}

impl<P> GpioPullPin for GpioPin<P>
where
    P: GpioHardwarePin,
{
    fn set_pull(&mut self, pull: GpioPull) -> Result<(), GpioError> {
        self.set_pull(pull)
    }
}

impl<P> GpioDriveStrengthPin for GpioPin<P>
where
    P: GpioHardwarePin,
{
    fn set_drive_strength(&mut self, strength: GpioDriveStrength) -> Result<(), GpioError> {
        self.set_drive_strength(strength)
    }
}

impl<P> GpioOutputPin for GpioPin<P>
where
    P: GpioHardwarePin,
{
    fn configure_output(&mut self, initial_high: bool) -> Result<(), GpioError> {
        self.configure_output(initial_high)
    }

    fn set_level(&mut self, high: bool) -> Result<(), GpioError> {
        self.set_level(high)
    }
}

impl<P> GpioInputPin for GpioPin<P>
where
    P: GpioHardwarePin,
{
    fn configure_input(&mut self) -> Result<(), GpioError> {
        self.configure_input()
    }

    fn read_level(&self) -> Result<bool, GpioError> {
        self.read()
    }
}
