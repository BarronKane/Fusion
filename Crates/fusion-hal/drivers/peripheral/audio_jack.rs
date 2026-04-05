//! 3.5mm audio jack peripheral backed by owned GPIO lines.
//!
//! Models a TRRS (tip-ring-ring-sleeve) audio jack module. Each conductor is an independently
//! owned GPIO line. The actual analog audio output requires DAC, PWM, or PIO-driven I2S;
//! this peripheral owns the pins and exposes the wiring truth for each conductor.

use crate::drivers::peripheral::interface::gpio::{
    GpioPeripheral,
    GpioPeripheralError as GpioError,
    GpioPeripheralOutputPin as GpioOutputPinContract,
};

/// Conductor role in a TRRS 3.5mm audio jack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioJackConductor {
    /// Tip — conventionally left audio channel.
    Tip,
    /// Ring 1 — conventionally right audio channel.
    Ring1,
    /// Ring 2 — conventionally microphone or ground (depending on standard).
    Ring2,
    /// Sleeve — conventionally ground.
    Sleeve,
}

/// TRRS 3.5mm audio jack peripheral backed by four owned GPIO outputs.
///
/// Each conductor (tip, ring1, ring2, sleeve) is independently owned. The actual audio signal
/// generation is left to a future DAC, PWM, or PIO/I2S contract; this peripheral establishes
/// pin ownership for the physical connector.
#[derive(Debug)]
pub struct AudioJack<Tip, Ring1, Ring2, Sleeve> {
    tip: Tip,
    ring1: Ring1,
    ring2: Ring2,
    sleeve: Sleeve,
}

impl<Tip, Ring1, Ring2, Sleeve> AudioJack<Tip, Ring1, Ring2, Sleeve>
where
    Tip: GpioOutputPinContract,
    Ring1: GpioOutputPinContract,
    Ring2: GpioOutputPinContract,
    Sleeve: GpioOutputPinContract,
{
    /// Creates one TRRS audio jack peripheral by claiming all four conductor pins.
    ///
    /// All pins are configured as outputs with initial low level.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when any conductor pin cannot be configured for output.
    pub fn new(
        mut tip: Tip,
        mut ring1: Ring1,
        mut ring2: Ring2,
        mut sleeve: Sleeve,
    ) -> Result<Self, GpioError> {
        tip.configure_output(false)?;
        ring1.configure_output(false)?;
        ring2.configure_output(false)?;
        sleeve.configure_output(false)?;
        Ok(Self {
            tip,
            ring1,
            ring2,
            sleeve,
        })
    }

    /// Returns a reference to the tip (left channel) pin.
    #[must_use]
    pub const fn tip(&self) -> &Tip {
        &self.tip
    }

    /// Returns a reference to the ring 1 (right channel) pin.
    #[must_use]
    pub const fn ring1(&self) -> &Ring1 {
        &self.ring1
    }

    /// Returns a reference to the ring 2 (mic/ground) pin.
    #[must_use]
    pub const fn ring2(&self) -> &Ring2 {
        &self.ring2
    }

    /// Returns a reference to the sleeve (ground) pin.
    #[must_use]
    pub const fn sleeve(&self) -> &Sleeve {
        &self.sleeve
    }

    /// Returns a mutable reference to the tip pin for direct driver access.
    pub fn tip_mut(&mut self) -> &mut Tip {
        &mut self.tip
    }

    /// Returns a mutable reference to the ring 1 pin for direct driver access.
    pub fn ring1_mut(&mut self) -> &mut Ring1 {
        &mut self.ring1
    }

    /// Returns a mutable reference to the ring 2 pin for direct driver access.
    pub fn ring2_mut(&mut self) -> &mut Ring2 {
        &mut self.ring2
    }

    /// Returns a mutable reference to the sleeve pin for direct driver access.
    pub fn sleeve_mut(&mut self) -> &mut Sleeve {
        &mut self.sleeve
    }

    /// Releases all owned GPIO pins back to the caller.
    #[must_use]
    pub fn into_pins(self) -> (Tip, Ring1, Ring2, Sleeve) {
        (self.tip, self.ring1, self.ring2, self.sleeve)
    }
}

impl<Tip, Ring1, Ring2, Sleeve> GpioPeripheral for AudioJack<Tip, Ring1, Ring2, Sleeve>
where
    Tip: GpioOutputPinContract,
    Ring1: GpioOutputPinContract,
    Ring2: GpioOutputPinContract,
    Sleeve: GpioOutputPinContract,
{
    type Error = GpioError;
}

/// TRS 3.5mm audio jack peripheral backed by three owned GPIO outputs.
///
/// Simplified variant for standard stereo jacks without a microphone conductor.
#[derive(Debug)]
pub struct AudioJackStereo<Tip, Ring, Sleeve> {
    tip: Tip,
    ring: Ring,
    sleeve: Sleeve,
}

impl<Tip, Ring, Sleeve> AudioJackStereo<Tip, Ring, Sleeve>
where
    Tip: GpioOutputPinContract,
    Ring: GpioOutputPinContract,
    Sleeve: GpioOutputPinContract,
{
    /// Creates one TRS audio jack peripheral by claiming the tip, ring, and sleeve pins.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when any conductor pin cannot be configured for output.
    pub fn new(mut tip: Tip, mut ring: Ring, mut sleeve: Sleeve) -> Result<Self, GpioError> {
        tip.configure_output(false)?;
        ring.configure_output(false)?;
        sleeve.configure_output(false)?;
        Ok(Self { tip, ring, sleeve })
    }

    /// Returns a reference to the tip (left channel) pin.
    #[must_use]
    pub const fn tip(&self) -> &Tip {
        &self.tip
    }

    /// Returns a reference to the ring (right channel) pin.
    #[must_use]
    pub const fn ring(&self) -> &Ring {
        &self.ring
    }

    /// Returns a reference to the sleeve (ground) pin.
    #[must_use]
    pub const fn sleeve(&self) -> &Sleeve {
        &self.sleeve
    }

    /// Returns a mutable reference to the tip pin for direct driver access.
    pub fn tip_mut(&mut self) -> &mut Tip {
        &mut self.tip
    }

    /// Returns a mutable reference to the ring pin for direct driver access.
    pub fn ring_mut(&mut self) -> &mut Ring {
        &mut self.ring
    }

    /// Returns a mutable reference to the sleeve pin for direct driver access.
    pub fn sleeve_mut(&mut self) -> &mut Sleeve {
        &mut self.sleeve
    }

    /// Releases all owned GPIO pins back to the caller.
    #[must_use]
    pub fn into_pins(self) -> (Tip, Ring, Sleeve) {
        (self.tip, self.ring, self.sleeve)
    }
}

impl<Tip, Ring, Sleeve> GpioPeripheral for AudioJackStereo<Tip, Ring, Sleeve>
where
    Tip: GpioOutputPinContract,
    Ring: GpioOutputPinContract,
    Sleeve: GpioOutputPinContract,
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

    fn fake_pin(pin: u8) -> FakeOutputPin {
        FakeOutputPin {
            pin,
            configured: false,
            level: false,
        }
    }

    #[test]
    fn trrs_audio_jack_claims_all_four_conductors() {
        let jack = AudioJack::new(fake_pin(16), fake_pin(17), fake_pin(18), fake_pin(19))
            .expect("all conductor pins should configure");

        assert_eq!(jack.tip().pin(), 16);
        assert_eq!(jack.ring1().pin(), 17);
        assert_eq!(jack.ring2().pin(), 18);
        assert_eq!(jack.sleeve().pin(), 19);
    }

    #[test]
    fn trs_stereo_jack_claims_three_conductors() {
        let jack = AudioJackStereo::new(fake_pin(20), fake_pin(21), fake_pin(22))
            .expect("all conductor pins should configure");

        assert_eq!(jack.tip().pin(), 20);
        assert_eq!(jack.ring().pin(), 21);
        assert_eq!(jack.sleeve().pin(), 22);
    }

    #[test]
    fn trrs_audio_jack_releases_pins() {
        let jack = AudioJack::new(fake_pin(16), fake_pin(17), fake_pin(18), fake_pin(19))
            .expect("all conductor pins should configure");

        let (tip, ring1, ring2, sleeve) = jack.into_pins();
        assert!(tip.configured);
        assert!(ring1.configured);
        assert!(ring2.configured);
        assert!(sleeve.configured);
    }
}
