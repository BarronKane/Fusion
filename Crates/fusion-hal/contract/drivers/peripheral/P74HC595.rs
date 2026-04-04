//! 74HC595 peripheral contracts.

/// One output slot on a 74HC595 package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum P74HC595OutputSlot {
    Q0,
    Q1,
    Q2,
    Q3,
    Q4,
    Q5,
    Q6,
    Q7,
}

impl P74HC595OutputSlot {
    /// Returns the bitmask for this output slot.
    #[must_use]
    pub const fn bit_mask(self) -> u8 {
        match self {
            Self::Q0 => 1 << 0,
            Self::Q1 => 1 << 1,
            Self::Q2 => 1 << 2,
            Self::Q3 => 1 << 3,
            Self::Q4 => 1 << 4,
            Self::Q5 => 1 << 5,
            Self::Q6 => 1 << 6,
            Self::Q7 => 1 << 7,
        }
    }
}

/// One physical 74HC595 device contract.
///
/// This models one package only. Daisy chaining, shared clocks/latches, and mirrored outputs are
/// composition concerns and belong in the consumer-side implementation layer, not in the device
/// trait itself.
pub trait P74HC595Contract {
    /// Concrete backend or composition error.
    type Error;

    /// Returns the currently staged output byte.
    fn staged_byte(&self) -> u8;

    /// Returns the currently staged level of one output slot.
    ///
    /// # Errors
    ///
    /// Returns an honest backend error when the slot cannot be observed.
    fn output_level(&self, output: P74HC595OutputSlot) -> Result<bool, Self::Error>;

    /// Stages one output level without latching it yet.
    ///
    /// # Errors
    ///
    /// Returns an honest backend error when the slot cannot be updated.
    fn stage_output_level(
        &mut self,
        output: P74HC595OutputSlot,
        high: bool,
    ) -> Result<(), Self::Error>;

    /// Latches the current staged byte to the physical device.
    ///
    /// # Errors
    ///
    /// Returns an honest backend error when the device cannot be driven.
    fn latch_staged(&mut self) -> Result<(), Self::Error>;

    /// Clears the full device to zero and latches the frame.
    ///
    /// # Errors
    ///
    /// Returns an honest backend error when the device cannot be driven.
    fn clear(&mut self) -> Result<(), Self::Error>;

    /// Sets one output level and latches immediately.
    ///
    /// # Errors
    ///
    /// Returns an honest backend error when the slot cannot be updated or the device cannot be
    /// driven.
    fn set_output_level(
        &mut self,
        output: P74HC595OutputSlot,
        high: bool,
    ) -> Result<(), Self::Error> {
        self.stage_output_level(output, high)?;
        self.latch_staged()
    }
}

/// Optional output-enable control for one physical 74HC595 device.
pub trait P74HC595OutputEnableContract: P74HC595Contract {
    /// Enables or disables the output stage.
    ///
    /// # Errors
    ///
    /// Returns an honest backend error when the output-enable surface cannot be driven.
    fn set_outputs_enabled(&mut self, enabled: bool) -> Result<(), Self::Error>;
}
