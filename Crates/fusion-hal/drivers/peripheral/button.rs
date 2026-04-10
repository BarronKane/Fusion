//! Simple button peripherals backed by owned GPIO inputs.

use crate::contract::drivers::peripheral::ButtonContract;
use crate::drivers::peripheral::interface::gpio::{
    GpioPeripheral,
    GpioPeripheralError as GpioError,
    GpioPeripheralInputPin as GpioInputPinContract,
};

/// Simple binary button peripheral backed by one owned GPIO input.
#[derive(Debug)]
pub struct Button<P> {
    pin: P,
    active_high: bool,
}

impl<P> Button<P>
where
    P: GpioInputPinContract,
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
    P: GpioInputPinContract,
{
    type Error = GpioError;

    fn is_pressed(&self) -> Result<bool, Self::Error> {
        Self::is_pressed(self)
    }
}

impl<P> GpioPeripheral for Button<P>
where
    P: GpioInputPinContract,
{
    type Error = GpioError;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::drivers::bus::gpio::{
        GpioCapabilities,
        GpioControllerDescriptor,
        GpioOwnedPinContract,
    };

    const TEST_GPIO_CONTROLLER: GpioControllerDescriptor = GpioControllerDescriptor {
        id: "test-gpio",
        name: "Test GPIO",
    };

    #[derive(Debug)]
    struct FakeInputPin {
        pin: u8,
        level: bool,
        configured: bool,
    }

    impl GpioOwnedPinContract for FakeInputPin {
        fn controller(&self) -> &'static GpioControllerDescriptor {
            &TEST_GPIO_CONTROLLER
        }

        fn pin(&self) -> u8 {
            self.pin
        }

        fn capabilities(&self) -> GpioCapabilities {
            GpioCapabilities::INPUT
        }
    }

    impl GpioInputPinContract for FakeInputPin {
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
