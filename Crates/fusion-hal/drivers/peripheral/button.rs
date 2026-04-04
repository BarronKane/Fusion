//! Simple button peripherals backed by owned GPIO inputs.

use crate::contract::drivers::peripheral::ButtonContract;
use crate::drivers::peripheral::interface::gpio::{
    GpioPeripheral,
    GpioPeripheralError as GpioError,
    GpioPeripheralInputPin as GpioInputPin,
};

/// Simple binary button peripheral backed by one owned GPIO input.
#[derive(Debug)]
pub struct Button<P> {
    pin: P,
    active_high: bool,
}

impl<P> Button<P>
where
    P: GpioInputPin,
{
    /// Creates one active-high button backed by one owned GPIO input.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the backing pin cannot be configured for input.
    pub fn new(mut pin: P) -> Result<Self, GpioError> {
        pin.configure_input()?;
        Ok(Self {
            pin,
            active_high: true,
        })
    }

    /// Creates one button with an explicit active-high/active-low electrical contract.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the backing pin cannot be configured for input.
    pub fn with_polarity(mut pin: P, active_high: bool) -> Result<Self, GpioError> {
        pin.configure_input()?;
        Ok(Self { pin, active_high })
    }

    /// Returns whether the button is currently pressed.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the backing pin cannot be sampled.
    pub fn is_pressed(&self) -> Result<bool, GpioError> {
        let level = self.pin.read_level()?;
        Ok(if self.active_high { level } else { !level })
    }

    /// Releases the owned GPIO input back to the caller.
    #[must_use]
    pub fn into_pin(self) -> P {
        self.pin
    }
}

impl<P> ButtonContract for Button<P>
where
    P: GpioInputPin,
{
    type Error = GpioError;

    fn is_pressed(&self) -> Result<bool, Self::Error> {
        Self::is_pressed(self)
    }
}

impl<P> GpioPeripheral for Button<P>
where
    P: GpioInputPin,
{
    type Error = GpioError;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::bus::gpio::{
        GpioCapabilities,
        GpioOwnedPin,
    };

    #[derive(Debug)]
    struct FakeInputPin {
        pin: u8,
        level: bool,
        configured: bool,
    }

    impl GpioOwnedPin for FakeInputPin {
        fn pin(&self) -> u8 {
            self.pin
        }

        fn capabilities(&self) -> GpioCapabilities {
            GpioCapabilities::INPUT
        }
    }

    impl GpioInputPin for FakeInputPin {
        fn configure_input(&mut self) -> Result<(), GpioError> {
            self.configured = true;
            Ok(())
        }

        fn read_level(&self) -> Result<bool, GpioError> {
            Ok(self.level)
        }
    }

    #[test]
    fn button_reports_pressed_for_active_high_inputs() {
        let button = Button::new(FakeInputPin {
            pin: 3,
            level: true,
            configured: false,
        })
        .expect("input pin should configure");
        assert!(button.is_pressed().expect("button should read"));
    }

    #[test]
    fn button_respects_active_low_polarity() {
        let button = Button::with_polarity(
            FakeInputPin {
                pin: 4,
                level: false,
                configured: false,
            },
            false,
        )
        .expect("input pin should configure");
        assert!(button.is_pressed().expect("button should read"));
    }
}
