//! OLED display peripheral backed by owned I2C GPIO lines.
//!
//! This scaffolds the GPIO claim and pin ownership for a typical I2C OLED module wired as
//! GND + VCC + SCL + SDA. The actual I2C protocol driver and display command sequences belong
//! to a future I2C/display contract; this peripheral owns the pins and exposes the wiring truth.

use crate::drivers::peripheral::interface::gpio::{
    GpioPeripheral,
    GpioPeripheralError as GpioError,
    GpioPeripheralOutputPin as GpioOutputPinContract,
};

/// I2C wiring contract for one OLED display module.
///
/// Owns the SCL and SDA pins. The actual I2C clock/data driving is left to a future I2C
/// peripheral contract or bitbang driver; this struct establishes pin ownership and ensures
/// the GPIO lines are exclusively claimed.
#[derive(Debug)]
pub struct OledDisplay<Scl, Sda> {
    scl: Scl,
    sda: Sda,
}

impl<Scl, Sda> OledDisplay<Scl, Sda>
where
    Scl: GpioOutputPinContract,
    Sda: GpioOutputPinContract,
{
    /// Creates one OLED display peripheral by claiming the SCL and SDA pins.
    ///
    /// Both pins are configured as outputs with initial low level. The caller is responsible
    /// for selecting the correct I2C alternate function if the backend requires it.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when either pin cannot be configured for output.
    pub fn new(mut scl: Scl, mut sda: Sda) -> Result<Self, GpioError> {
        scl.configure_output(false)?;
        sda.configure_output(false)?;
        Ok(Self { scl, sda })
    }

    /// Returns a reference to the owned SCL pin.
    #[must_use]
    pub const fn scl(&self) -> &Scl {
        &self.scl
    }

    /// Returns a reference to the owned SDA pin.
    #[must_use]
    pub const fn sda(&self) -> &Sda {
        &self.sda
    }

    /// Returns a mutable reference to the owned SCL pin.
    pub fn scl_mut(&mut self) -> &mut Scl {
        &mut self.scl
    }

    /// Returns a mutable reference to the owned SDA pin.
    pub fn sda_mut(&mut self) -> &mut Sda {
        &mut self.sda
    }

    /// Releases the owned GPIO pins back to the caller.
    #[must_use]
    pub fn into_pins(self) -> (Scl, Sda) {
        (self.scl, self.sda)
    }
}

impl<Scl, Sda> GpioPeripheral for OledDisplay<Scl, Sda>
where
    Scl: GpioOutputPinContract,
    Sda: GpioOutputPinContract,
{
    type Error = GpioError;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::bus::gpio::{
        GpioCapabilities,
        GpioOwnedPinContract,
    };

    #[derive(Debug)]
    struct FakeOutputPin {
        pin: u8,
        configured: bool,
        level: bool,
    }

    impl GpioOwnedPinContract for FakeOutputPin {
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
    fn oled_display_claims_scl_and_sda_pins() {
        let display = OledDisplay::new(
            FakeOutputPin {
                pin: 9,
                configured: false,
                level: false,
            },
            FakeOutputPin {
                pin: 10,
                configured: false,
                level: false,
            },
        )
        .expect("i2c pins should configure");

        assert_eq!(display.scl().pin(), 9);
        assert_eq!(display.sda().pin(), 10);

        let (scl, sda) = display.into_pins();
        assert!(scl.configured);
        assert!(sda.configured);
    }
}
