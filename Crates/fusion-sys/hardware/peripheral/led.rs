//! Simple LED peripherals backed by owned GPIO outputs.

use fusion_pal::contract::drivers::peripheral::LedContract;

use crate::gpio::{
    GpioError,
    GpioOutputPin,
};

/// Simple binary LED peripheral backed by one owned GPIO output.
#[derive(Debug)]
pub struct Led<P> {
    pin: P,
    active_high: bool,
    lit: bool,
}

impl<P> Led<P>
where
    P: GpioOutputPin,
{
    /// Creates one active-high LED backed by one owned GPIO output.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the backing pin cannot be configured for output.
    pub fn new(mut pin: P) -> Result<Self, GpioError> {
        pin.configure_output(false)?;
        Ok(Self {
            pin,
            active_high: true,
            lit: false,
        })
    }

    /// Creates one LED with an explicit active-high/active-low electrical contract.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the backing pin cannot be configured for output.
    pub fn with_polarity(mut pin: P, active_high: bool) -> Result<Self, GpioError> {
        pin.configure_output(false)?;
        Ok(Self {
            pin,
            active_high,
            lit: false,
        })
    }

    /// Returns whether this LED is currently commanded on.
    #[must_use]
    pub const fn is_on(&self) -> bool {
        self.lit
    }

    /// Sets the LED on/off state.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the backing pin cannot be driven.
    pub fn set(&mut self, on: bool) -> Result<(), GpioError> {
        self.pin
            .set_level(if self.active_high { on } else { !on })?;
        self.lit = on;
        Ok(())
    }

    /// Turns the LED on.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the backing pin cannot be driven.
    pub fn on(&mut self) -> Result<(), GpioError> {
        self.set(true)
    }

    /// Turns the LED off.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the backing pin cannot be driven.
    pub fn off(&mut self) -> Result<(), GpioError> {
        self.set(false)
    }

    /// Toggles the LED state.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the backing pin cannot be driven.
    pub fn toggle(&mut self) -> Result<(), GpioError> {
        self.set(!self.lit)
    }

    /// Releases the owned GPIO output back to the caller.
    #[must_use]
    pub fn into_pin(self) -> P {
        self.pin
    }
}

impl<P> LedContract for Led<P>
where
    P: GpioOutputPin,
{
    type Error = GpioError;

    fn is_on(&self) -> bool {
        Self::is_on(self)
    }

    fn set(&mut self, on: bool) -> Result<(), Self::Error> {
        Self::set(self, on)
    }

    fn toggle(&mut self) -> Result<(), Self::Error> {
        Self::toggle(self)
    }
}
