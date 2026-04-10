//! Simple paired LED indicators backed by two owned GPIO outputs.

use crate::drivers::peripheral::interface::gpio::{
    GpioPeripheral,
    GpioPeripheralError as GpioError,
    GpioPeripheralOutputPin as GpioOutputPinContract,
};
use super::Led;

/// One paired LED indicator backed by two owned GPIO outputs.
#[derive(Debug)]
pub struct LedPair<P1, P2> {
    first: Led<P1>,
    second: Led<P2>,
}

impl<P1, P2> LedPair<P1, P2>
where
    P1: GpioOutputPinContract,
    P2: GpioOutputPinContract,
{
    /// Creates one paired active-high LED indicator.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when either backing pin cannot be configured for output.
    pub fn new(first: P1, second: P2) -> Result<Self, GpioError> {
        Ok(Self {
            first: Led::new(first)?,
            second: Led::new(second)?,
        })
    }

    /// Creates one paired LED indicator with explicit electrical polarity for each leg.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when either backing pin cannot be configured for output.
    pub fn with_polarities(
        first: P1,
        first_active_high: bool,
        second: P2,
        second_active_high: bool,
    ) -> Result<Self, GpioError> {
        Ok(Self {
            first: Led::with_polarity(first, first_active_high)?,
            second: Led::with_polarity(second, second_active_high)?,
        })
    }

    /// Sets both LEDs in one call.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when either backing pin cannot be driven.
    pub fn set(&mut self, first_on: bool, second_on: bool) -> Result<(), GpioError> {
        self.first.set(first_on)?;
        self.second.set(second_on)?;
        Ok(())
    }

    /// Turns both LEDs off.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when either backing pin cannot be driven.
    pub fn off(&mut self) -> Result<(), GpioError> {
        self.set(false, false)
    }

    /// Turns only the first LED on.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when either backing pin cannot be driven.
    pub fn first(&mut self) -> Result<(), GpioError> {
        self.set(true, false)
    }

    /// Turns only the second LED on.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when either backing pin cannot be driven.
    pub fn second(&mut self) -> Result<(), GpioError> {
        self.set(false, true)
    }

    /// Turns both LEDs on.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when either backing pin cannot be driven.
    pub fn both(&mut self) -> Result<(), GpioError> {
        self.set(true, true)
    }

    /// Returns whether the first LED is currently commanded on.
    #[must_use]
    pub const fn first_is_on(&self) -> bool {
        self.first.is_on()
    }

    /// Returns whether the second LED is currently commanded on.
    #[must_use]
    pub const fn second_is_on(&self) -> bool {
        self.second.is_on()
    }

    /// Releases the owned LED peripherals back to the caller.
    #[must_use]
    pub fn into_leds(self) -> (Led<P1>, Led<P2>) {
        (self.first, self.second)
    }
}

impl<P1, P2> GpioPeripheral for LedPair<P1, P2>
where
    P1: GpioOutputPinContract,
    P2: GpioOutputPinContract,
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
    struct FakeOutputPin {
        pin: u8,
        configured: bool,
        level: bool,
    }

    impl GpioOwnedPinContract for FakeOutputPin {
        fn controller(&self) -> &'static GpioControllerDescriptor {
            &TEST_GPIO_CONTROLLER
        }

        fn pin(&self) -> u8 {
            self.pin
        }

        fn capabilities(&self) -> GpioCapabilities {
            GpioCapabilities::OUTPUT
        }
    }

    impl GpioOutputPinContract for FakeOutputPin {
        fn configure_output(&mut self, initial_high: bool) -> Result<(), GpioError> {
            self.configured = true;
            self.level = initial_high;
            Ok(())
        }

        fn set_level(&mut self, high: bool) -> Result<(), GpioError> {
            self.level = high;
            Ok(())
        }
    }

    #[test]
    fn led_pair_sets_each_leg_independently() {
        let mut pair = LedPair::new(
            FakeOutputPin {
                pin: 1,
                configured: false,
                level: false,
            },
            FakeOutputPin {
                pin: 2,
                configured: false,
                level: false,
            },
        )
        .expect("output pins should configure");

        pair.first().expect("first leg should drive");
        assert!(pair.first_is_on());
        assert!(!pair.second_is_on());

        pair.second().expect("second leg should drive");
        assert!(!pair.first_is_on());
        assert!(pair.second_is_on());

        pair.both().expect("both legs should drive");
        assert!(pair.first_is_on());
        assert!(pair.second_is_on());
    }
}
