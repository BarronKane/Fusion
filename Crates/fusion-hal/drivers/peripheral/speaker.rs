//! Passive speaker/woofer peripheral backed by owned GPIO output.
//!
//! Models a passive enclosed speaker driven through a 2.54mm Dupont interface (signal + ground).
//! The speaker requires an external amplifier or direct PWM/timer-driven signal to produce audio;
//! this peripheral owns the signal pin and exposes the wiring truth.

use crate::drivers::peripheral::interface::gpio::{
    GpioPeripheral,
    GpioPeripheralError as GpioError,
    GpioPeripheralOutputPin as GpioOutputPin,
};

/// Passive enclosed speaker peripheral backed by one owned GPIO signal output.
///
/// The physical device is driven by a frequency signal on the signal pin. Asserting the pin
/// alone does not produce continuous audio — the caller must arrange PWM, timer, or PIO-driven
/// frequency output. This peripheral owns the pin and tracks the commanded state.
#[derive(Debug)]
pub struct Speaker<P> {
    signal: P,
    impedance_ohms: u8,
    max_watts: u8,
    enabled: bool,
}

impl<P> Speaker<P>
where
    P: GpioOutputPin,
{
    /// Creates one passive speaker peripheral by claiming the signal pin.
    ///
    /// `impedance_ohms` and `max_watts` are electrical metadata describing the physical device
    /// (e.g. 8 ohm, 5 watt). They do not affect GPIO behavior but are carried as truthful
    /// metadata for downstream audio pipeline decisions.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the signal pin cannot be configured for output.
    pub fn new(mut signal: P, impedance_ohms: u8, max_watts: u8) -> Result<Self, GpioError> {
        signal.configure_output(false)?;
        Ok(Self {
            signal,
            impedance_ohms,
            max_watts,
            enabled: false,
        })
    }

    /// Returns the device impedance in ohms.
    #[must_use]
    pub const fn impedance_ohms(&self) -> u8 {
        self.impedance_ohms
    }

    /// Returns the device maximum power rating in watts.
    #[must_use]
    pub const fn max_watts(&self) -> u8 {
        self.max_watts
    }

    /// Returns whether this speaker is currently commanded on (signal pin asserted).
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Asserts the signal pin.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the pin cannot be driven.
    pub fn enable(&mut self) -> Result<(), GpioError> {
        self.signal.set_level(true)?;
        self.enabled = true;
        Ok(())
    }

    /// De-asserts the signal pin.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the pin cannot be driven.
    pub fn disable(&mut self) -> Result<(), GpioError> {
        self.signal.set_level(false)?;
        self.enabled = false;
        Ok(())
    }

    /// Toggles the signal pin level. Useful for software-driven frequency generation when
    /// called at a regular interval from a timer or fiber.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the pin cannot be driven.
    pub fn toggle(&mut self) -> Result<(), GpioError> {
        let next = !self.enabled;
        self.signal.set_level(next)?;
        self.enabled = next;
        Ok(())
    }

    /// Returns a mutable reference to the owned signal pin for direct driver access.
    pub fn signal_mut(&mut self) -> &mut P {
        &mut self.signal
    }

    /// Releases the owned GPIO output back to the caller.
    #[must_use]
    pub fn into_pin(self) -> P {
        self.signal
    }
}

impl<P> GpioPeripheral for Speaker<P>
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
    fn speaker_claims_signal_pin_and_carries_metadata() {
        let speaker = Speaker::new(
            FakeOutputPin {
                pin: 12,
                configured: false,
                level: false,
            },
            8,
            5,
        )
        .expect("signal pin should configure");

        assert_eq!(speaker.impedance_ohms(), 8);
        assert_eq!(speaker.max_watts(), 5);
        assert!(!speaker.is_enabled());
    }

    #[test]
    fn speaker_toggle_alternates_signal_level() {
        let mut speaker = Speaker::new(
            FakeOutputPin {
                pin: 13,
                configured: false,
                level: false,
            },
            8,
            5,
        )
        .expect("signal pin should configure");

        speaker.toggle().expect("first toggle should succeed");
        assert!(speaker.is_enabled());
        speaker.toggle().expect("second toggle should succeed");
        assert!(!speaker.is_enabled());
    }
}
