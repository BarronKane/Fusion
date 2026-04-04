//! Simple buzzer peripherals backed by owned GPIO outputs.
//!
//! Supports both active buzzers (self-oscillating, driven by a single GPIO level) and passive
//! buzzers (externally driven, requiring a frequency signal). The frequency-generation path is
//! left to the caller or a future PCU/timer contract; these peripherals own the GPIO pin and
//! expose an honest on/off surface.

use crate::drivers::peripheral::interface::gpio::{
    GpioPeripheral,
    GpioPeripheralError as GpioError,
    GpioPeripheralOutputPin as GpioOutputPin,
};

/// Buzzer variant describing the electrical behavior of the connected device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuzzerKind {
    /// Active buzzer: self-oscillating, produces sound when its drive pin is asserted.
    Active,
    /// Passive buzzer: requires an external frequency signal (typically 2–5 kHz) to produce sound.
    Passive,
}

/// Simple buzzer peripheral backed by one owned GPIO output.
///
/// For active buzzers, asserting the pin produces sound at the device's fixed frequency.
/// For passive buzzers, asserting the pin alone may produce a click or silence; the caller is
/// responsible for driving a frequency signal through a timer, PCU program, or toggling loop.
#[derive(Debug)]
pub struct Buzzer<P> {
    pin: P,
    kind: BuzzerKind,
    active_high: bool,
    sounding: bool,
}

impl<P> Buzzer<P>
where
    P: GpioOutputPin,
{
    /// Creates one active-high buzzer backed by one owned GPIO output.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the backing pin cannot be configured for output.
    pub fn new(mut pin: P, kind: BuzzerKind) -> Result<Self, GpioError> {
        pin.configure_output(false)?;
        Ok(Self {
            pin,
            kind,
            active_high: true,
            sounding: false,
        })
    }

    /// Creates one buzzer with an explicit active-high/active-low electrical contract.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the backing pin cannot be configured for output.
    pub fn with_polarity(
        mut pin: P,
        kind: BuzzerKind,
        active_high: bool,
    ) -> Result<Self, GpioError> {
        pin.configure_output(false)?;
        Ok(Self {
            pin,
            kind,
            active_high,
            sounding: false,
        })
    }

    /// Returns the buzzer variant.
    #[must_use]
    pub const fn kind(&self) -> BuzzerKind {
        self.kind
    }

    /// Returns whether this buzzer is currently commanded on.
    #[must_use]
    pub const fn is_sounding(&self) -> bool {
        self.sounding
    }

    /// Asserts the drive pin to start the buzzer.
    ///
    /// For active buzzers this immediately produces sound. For passive buzzers this asserts the
    /// GPIO level but does not generate a frequency; the caller must arrange frequency driving
    /// separately.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the backing pin cannot be driven.
    pub fn on(&mut self) -> Result<(), GpioError> {
        self.pin
            .set_level(if self.active_high { true } else { false })?;
        self.sounding = true;
        Ok(())
    }

    /// De-asserts the drive pin to silence the buzzer.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the backing pin cannot be driven.
    pub fn off(&mut self) -> Result<(), GpioError> {
        self.pin
            .set_level(if self.active_high { false } else { true })?;
        self.sounding = false;
        Ok(())
    }

    /// Toggles the drive pin level. Useful for software-driven frequency generation on passive
    /// buzzers when called at a regular interval.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the backing pin cannot be driven.
    pub fn toggle(&mut self) -> Result<(), GpioError> {
        let next = !self.sounding;
        self.pin
            .set_level(if self.active_high { next } else { !next })?;
        self.sounding = next;
        Ok(())
    }

    /// Releases the owned GPIO output back to the caller.
    #[must_use]
    pub fn into_pin(self) -> P {
        self.pin
    }
}

impl<P> GpioPeripheral for Buzzer<P>
where
    P: GpioOutputPin,
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
    struct FakeOutputPin {
        pin: u8,
        configured: bool,
        level: bool,
    }

    impl GpioOwnedPin for FakeOutputPin {
        fn pin(&self) -> u8 {
            self.pin
        }

        fn capabilities(&self) -> GpioCapabilities {
            GpioCapabilities::OUTPUT
        }
    }

    impl GpioOutputPin for FakeOutputPin {
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
    fn active_buzzer_sounds_when_driven_on() {
        let mut buzzer = Buzzer::new(
            FakeOutputPin {
                pin: 5,
                configured: false,
                level: false,
            },
            BuzzerKind::Active,
        )
        .expect("output pin should configure");

        assert!(!buzzer.is_sounding());
        buzzer.on().expect("buzzer should drive on");
        assert!(buzzer.is_sounding());
        buzzer.off().expect("buzzer should drive off");
        assert!(!buzzer.is_sounding());
    }

    #[test]
    fn passive_buzzer_toggle_alternates_level() {
        let mut buzzer = Buzzer::new(
            FakeOutputPin {
                pin: 6,
                configured: false,
                level: false,
            },
            BuzzerKind::Passive,
        )
        .expect("output pin should configure");

        buzzer.toggle().expect("first toggle should succeed");
        assert!(buzzer.is_sounding());
        buzzer.toggle().expect("second toggle should succeed");
        assert!(!buzzer.is_sounding());
    }
}
