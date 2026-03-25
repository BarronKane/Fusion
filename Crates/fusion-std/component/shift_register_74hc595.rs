//! 74HC595-style serial-in, parallel-out shift register components.
//!
//! This component owns the data, shift-clock, and latch-clock pins needed to drive one or more
//! daisy-chained 74HC595 devices. Optional output-enable wiring is supported when the board wants
//! software blanking or PWM control; otherwise `CE` can stay tied low in hardware.

use fusion_sys::gpio::{GpioError, GpioOutputPin};

/// Marker for one 74HC595 chain without one software-controlled output-enable pin.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoOutputEnable;

#[doc(hidden)]
pub trait OutputEnableControl {
    fn configure_output_enable(&mut self) -> Result<(), GpioError>;
    fn set_outputs_enabled(&mut self, enabled: bool) -> Result<(), GpioError>;
}

impl OutputEnableControl for NoOutputEnable {
    fn configure_output_enable(&mut self) -> Result<(), GpioError> {
        Ok(())
    }

    fn set_outputs_enabled(&mut self, _enabled: bool) -> Result<(), GpioError> {
        Ok(())
    }
}

impl<P> OutputEnableControl for P
where
    P: GpioOutputPin,
{
    fn configure_output_enable(&mut self) -> Result<(), GpioError> {
        // 74HC595 CE/OE is active-low; start disabled so one fresh register does not briefly
        // expose junk outputs before the first latched frame is written.
        self.configure_output(true)
    }

    fn set_outputs_enabled(&mut self, enabled: bool) -> Result<(), GpioError> {
        self.set_level(!enabled)
    }
}

/// One owned 74HC595 shift-register chain.
#[derive(Debug)]
pub struct ShiftRegister74hc595<Data, ShiftClock, LatchClock, OutputEnable = NoOutputEnable> {
    data: Data,
    shift_clock: ShiftClock,
    latch_clock: LatchClock,
    output_enable: OutputEnable,
}

impl<Data, ShiftClock, LatchClock>
    ShiftRegister74hc595<Data, ShiftClock, LatchClock, NoOutputEnable>
where
    Data: GpioOutputPin,
    ShiftClock: GpioOutputPin,
    LatchClock: GpioOutputPin,
{
    /// Creates one 74HC595 chain with hardware-tied output enable.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when any backing pin cannot be configured for output.
    pub fn new(
        data: Data,
        shift_clock: ShiftClock,
        latch_clock: LatchClock,
    ) -> Result<Self, GpioError> {
        Self::with_output_enable(data, shift_clock, latch_clock, NoOutputEnable)
    }
}

impl<Data, ShiftClock, LatchClock, OutputEnable>
    ShiftRegister74hc595<Data, ShiftClock, LatchClock, OutputEnable>
where
    Data: GpioOutputPin,
    ShiftClock: GpioOutputPin,
    LatchClock: GpioOutputPin,
    OutputEnable: OutputEnableControl,
{
    /// Creates one 74HC595 chain with one explicit software-controlled output-enable pin.
    ///
    /// `output_enable` must be wired to `CE/OE` and is treated as active-low.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when any backing pin cannot be configured for output.
    pub fn with_output_enable(
        mut data: Data,
        mut shift_clock: ShiftClock,
        mut latch_clock: LatchClock,
        mut output_enable: OutputEnable,
    ) -> Result<Self, GpioError> {
        data.configure_output(false)?;
        shift_clock.configure_output(false)?;
        latch_clock.configure_output(false)?;
        output_enable.configure_output_enable()?;
        let mut register = Self {
            data,
            shift_clock,
            latch_clock,
            output_enable,
        };
        register.clear()?;
        register.set_outputs_enabled(true)?;
        Ok(register)
    }

    /// Enables or disables the output stage.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the output-enable pin cannot be driven.
    pub fn set_outputs_enabled(&mut self, enabled: bool) -> Result<(), GpioError> {
        self.output_enable.set_outputs_enabled(enabled)
    }

    /// Writes one latched frame to the register chain, shifting the most-significant bit first
    /// within each byte.
    ///
    /// For one chain of two 74HC595 devices, the first byte in the slice lands in the furthest
    /// register and the last byte lands in the nearest register, matching normal daisy-chain
    /// behavior.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when any backing pin cannot be driven.
    pub fn write_bytes_msb_first(&mut self, bytes: &[u8]) -> Result<(), GpioError> {
        for &byte in bytes {
            self.shift_byte_msb_first(byte)?;
        }
        self.latch()
    }

    /// Clears the full chain to zero and latches the frame.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when any backing pin cannot be driven.
    pub fn clear(&mut self) -> Result<(), GpioError> {
        self.write_bytes_msb_first(&[0])
    }

    fn shift_byte_msb_first(&mut self, byte: u8) -> Result<(), GpioError> {
        for bit in (0..8).rev() {
            self.data.set_level(((byte >> bit) & 1) != 0)?;
            self.pulse_shift_clock()?;
        }
        Ok(())
    }

    fn pulse_shift_clock(&mut self) -> Result<(), GpioError> {
        self.shift_clock.set_level(true)?;
        self.shift_clock.set_level(false)
    }

    fn latch(&mut self) -> Result<(), GpioError> {
        self.latch_clock.set_level(true)?;
        self.latch_clock.set_level(false)
    }
}
