//! 74HC595-style serial-in, parallel-out shift register peripherals.
//!
//! This peripheral owns the data, shift-clock, and latch-clock pins needed to drive one or more
//! daisy-chained 74HC595 devices. Optional output-enable wiring is supported when the board wants
//! software blanking or PWM control; otherwise `CE` can stay tied low in hardware.

use crate::drivers::peripheral::interface::gpio::{
    GpioPeripheral,
    GpioPeripheralError as GpioError,
    GpioPeripheralOutputPin as GpioOutputPinContract,
};

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
    P: GpioOutputPinContract,
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

/// One logical package in a composed 74HC595 chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShiftRegister74hc595PackageId(pub u8);

impl ShiftRegister74hc595PackageId {
    /// Returns the zero-based array index for this package ID.
    #[must_use]
    pub const fn array_index(self) -> Option<usize> {
        if self.0 == 0 {
            None
        } else {
            Some((self.0 - 1) as usize)
        }
    }
}

/// One output slot on a composed 74HC595 package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShiftRegister74hc595OutputSlot {
    Q0,
    Q1,
    Q2,
    Q3,
    Q4,
    Q5,
    Q6,
    Q7,
}

impl ShiftRegister74hc595OutputSlot {
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

/// One fully qualified output line in a composed 74HC595 chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShiftRegister74hc595OutputId {
    package: ShiftRegister74hc595PackageId,
    slot: ShiftRegister74hc595OutputSlot,
}

impl ShiftRegister74hc595OutputId {
    /// Creates one output ID.
    #[must_use]
    pub const fn new(
        package: ShiftRegister74hc595PackageId,
        slot: ShiftRegister74hc595OutputSlot,
    ) -> Self {
        Self { package, slot }
    }

    /// Returns the package ID.
    #[must_use]
    pub const fn package(self) -> ShiftRegister74hc595PackageId {
        self.package
    }

    /// Returns the output slot.
    #[must_use]
    pub const fn slot(self) -> ShiftRegister74hc595OutputSlot {
        self.slot
    }
}

/// One shared logical output spanning one or more concrete outputs in a composed chain.
#[derive(Debug, Clone, Copy)]
pub struct ShiftRegister74hc595OutputGroup<'a> {
    outputs: &'a [ShiftRegister74hc595OutputId],
}

impl<'a> ShiftRegister74hc595OutputGroup<'a> {
    /// Creates one shared output group.
    #[must_use]
    pub const fn new(outputs: &'a [ShiftRegister74hc595OutputId]) -> Self {
        Self { outputs }
    }

    /// Returns the concrete outputs in this group.
    #[must_use]
    pub const fn outputs(self) -> &'a [ShiftRegister74hc595OutputId] {
        self.outputs
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

impl<Data, ShiftClock, LatchClock, OutputEnable> GpioPeripheral
    for ShiftRegister74hc595<Data, ShiftClock, LatchClock, OutputEnable>
where
    Data: GpioOutputPinContract,
    ShiftClock: GpioOutputPinContract,
    LatchClock: GpioOutputPinContract,
    OutputEnable: OutputEnableControl,
{
    type Error = GpioError;
}

impl<Data, ShiftClock, LatchClock>
    ShiftRegister74hc595<Data, ShiftClock, LatchClock, NoOutputEnable>
where
    Data: GpioOutputPinContract,
    ShiftClock: GpioOutputPinContract,
    LatchClock: GpioOutputPinContract,
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
    Data: GpioOutputPinContract,
    ShiftClock: GpioOutputPinContract,
    LatchClock: GpioOutputPinContract,
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

/// One composed 74HC595 chain with one staged shadow frame.
///
/// This is the honest surface for addressing outputs such as `U1.Q3` or one shared logical line
/// spanning `U1.Q3` and `U2.Q3`, instead of hand-authoring raw byte masks every time.
#[derive(Debug)]
pub struct ComposedShiftRegister74hc595<
    const PACKAGES: usize,
    Data,
    ShiftClock,
    LatchClock,
    OutputEnable = NoOutputEnable,
> {
    register: ShiftRegister74hc595<Data, ShiftClock, LatchClock, OutputEnable>,
    staged: [u8; PACKAGES],
}

impl<Data, ShiftClock, LatchClock, OutputEnable, const PACKAGES: usize> GpioPeripheral
    for ComposedShiftRegister74hc595<PACKAGES, Data, ShiftClock, LatchClock, OutputEnable>
where
    Data: GpioOutputPinContract,
    ShiftClock: GpioOutputPinContract,
    LatchClock: GpioOutputPinContract,
    OutputEnable: OutputEnableControl,
{
    type Error = GpioError;
}

impl<const PACKAGES: usize, Data, ShiftClock, LatchClock>
    ComposedShiftRegister74hc595<PACKAGES, Data, ShiftClock, LatchClock, NoOutputEnable>
where
    Data: GpioOutputPinContract,
    ShiftClock: GpioOutputPinContract,
    LatchClock: GpioOutputPinContract,
{
    /// Creates one composed 74HC595 chain with hardware-tied output enable.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the backing pins cannot be configured or the chain
    /// cannot be cleared.
    pub fn new(
        data: Data,
        shift_clock: ShiftClock,
        latch_clock: LatchClock,
    ) -> Result<Self, GpioError> {
        Self::from_register(ShiftRegister74hc595::new(data, shift_clock, latch_clock)?)
    }
}

impl<const PACKAGES: usize, Data, ShiftClock, LatchClock, OutputEnable>
    ComposedShiftRegister74hc595<PACKAGES, Data, ShiftClock, LatchClock, OutputEnable>
where
    Data: GpioOutputPinContract,
    ShiftClock: GpioOutputPinContract,
    LatchClock: GpioOutputPinContract,
    OutputEnable: GpioOutputPinContract,
{
    /// Creates one composed 74HC595 chain with one explicit software-controlled output-enable pin.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the backing pins cannot be configured or the chain
    /// cannot be cleared.
    pub fn with_output_enable(
        data: Data,
        shift_clock: ShiftClock,
        latch_clock: LatchClock,
        output_enable: OutputEnable,
    ) -> Result<Self, GpioError> {
        Self::from_register(ShiftRegister74hc595::with_output_enable(
            data,
            shift_clock,
            latch_clock,
            output_enable,
        )?)
    }
}

impl<const PACKAGES: usize, Data, ShiftClock, LatchClock, OutputEnable>
    ComposedShiftRegister74hc595<PACKAGES, Data, ShiftClock, LatchClock, OutputEnable>
where
    Data: GpioOutputPinContract,
    ShiftClock: GpioOutputPinContract,
    LatchClock: GpioOutputPinContract,
    OutputEnable: OutputEnableControl,
{
    /// Creates one composed chain from one already-owned raw register chain.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the chain cannot be cleared.
    pub fn from_register(
        register: ShiftRegister74hc595<Data, ShiftClock, LatchClock, OutputEnable>,
    ) -> Result<Self, GpioError> {
        let mut composed = Self {
            register,
            staged: [0; PACKAGES],
        };
        composed.clear()?;
        Ok(composed)
    }

    /// Returns the number of logical packages in the chain.
    #[must_use]
    pub const fn package_count(&self) -> usize {
        PACKAGES
    }

    /// Returns the staged frame in shift order.
    #[must_use]
    pub const fn staged_frame(&self) -> &[u8; PACKAGES] {
        &self.staged
    }

    /// Returns the owned raw register chain.
    #[must_use]
    pub fn into_register(self) -> ShiftRegister74hc595<Data, ShiftClock, LatchClock, OutputEnable> {
        self.register
    }

    fn package_and_mask(output: ShiftRegister74hc595OutputId) -> Result<(usize, u8), GpioError> {
        let Some(package) = output.package().array_index() else {
            return Err(GpioError::invalid());
        };
        if package >= PACKAGES {
            return Err(GpioError::invalid());
        }
        Ok((package, output.slot().bit_mask()))
    }

    /// Returns the currently staged level of one composed output.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the output ID is invalid for this chain.
    pub fn output_level(&self, output: ShiftRegister74hc595OutputId) -> Result<bool, GpioError> {
        let (package, mask) = Self::package_and_mask(output)?;
        Ok((self.staged[package] & mask) != 0)
    }

    /// Stages one composed output without latching it yet.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the output ID is invalid for this chain.
    pub fn stage_output_level(
        &mut self,
        output: ShiftRegister74hc595OutputId,
        high: bool,
    ) -> Result<(), GpioError> {
        let (package, mask) = Self::package_and_mask(output)?;
        if high {
            self.staged[package] |= mask;
        } else {
            self.staged[package] &= !mask;
        }
        Ok(())
    }

    /// Stages one shared output group without latching it yet.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when any output ID is invalid for this chain.
    pub fn stage_output_group_level(
        &mut self,
        group: ShiftRegister74hc595OutputGroup<'_>,
        high: bool,
    ) -> Result<(), GpioError> {
        for output in group.outputs() {
            self.stage_output_level(*output, high)?;
        }
        Ok(())
    }

    /// Latches the current staged frame to the physical chain.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the chain cannot be driven.
    pub fn latch_staged(&mut self) -> Result<(), GpioError> {
        self.register.write_bytes_msb_first(&self.staged)
    }

    /// Clears the full chain to zero and latches the frame.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the chain cannot be driven.
    pub fn clear(&mut self) -> Result<(), GpioError> {
        self.staged = [0; PACKAGES];
        self.latch_staged()
    }

    /// Sets one output level and latches immediately.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the output ID is invalid or the chain cannot be driven.
    pub fn set_output_level(
        &mut self,
        output: ShiftRegister74hc595OutputId,
        high: bool,
    ) -> Result<(), GpioError> {
        self.stage_output_level(output, high)?;
        self.latch_staged()
    }

    /// Sets one shared output group and latches immediately.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when any output ID is invalid or the chain cannot be driven.
    pub fn set_output_group_level(
        &mut self,
        group: ShiftRegister74hc595OutputGroup<'_>,
        high: bool,
    ) -> Result<(), GpioError> {
        self.stage_output_group_level(group, high)?;
        self.latch_staged()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::drivers::bus::gpio::{
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

    fn fake_chain() -> ComposedShiftRegister74hc595<2, FakeOutputPin, FakeOutputPin, FakeOutputPin>
    {
        ComposedShiftRegister74hc595::new(
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
            FakeOutputPin {
                pin: 3,
                configured: false,
                level: false,
            },
        )
        .expect("shift register chain should configure")
    }

    #[test]
    fn composed_chain_tracks_u1_and_u2_outputs() {
        let mut chain = fake_chain();
        chain
            .stage_output_level(
                ShiftRegister74hc595OutputId::new(
                    ShiftRegister74hc595PackageId(1),
                    ShiftRegister74hc595OutputSlot::Q3,
                ),
                true,
            )
            .expect("u1 q3 should stage");
        chain
            .stage_output_level(
                ShiftRegister74hc595OutputId::new(
                    ShiftRegister74hc595PackageId(2),
                    ShiftRegister74hc595OutputSlot::Q3,
                ),
                true,
            )
            .expect("u2 q3 should stage");

        assert_eq!(chain.staged_frame(), &[0b0000_1000, 0b0000_1000]);
    }
}
