//! Four-digit multiplexed seven-segment display peripherals backed by owned GPIO outputs.
//!
//! The display owns one GPIO per segment line (`a` through `g`, plus decimal point) and one
//! GPIO per digit-select line (`d1` through `d4`). The peripheral maintains one four-glyph
//! framebuffer and exposes one refresh step at a time so callers can drive multiplex timing
//! from a timer, fiber, or interrupt that matches the platform truth.

use crate::gpio::{GpioError, GpioOutputPin};
use fusion_pal::contract::drivers::peripheral::{
    SevenSegmentDisplayContract,
    SevenSegmentGlyph as PeripheralSevenSegmentGlyph,
    SevenSegmentPolarity as PeripheralSevenSegmentPolarity,
};

use super::shift_register_74hc595::OutputEnableControl;
use super::{NoOutputEnable, ShiftRegister74hc595};

const SEGMENT_A: u8 = 1 << 0;
const SEGMENT_B: u8 = 1 << 1;
const SEGMENT_C: u8 = 1 << 2;
const SEGMENT_D: u8 = 1 << 3;
const SEGMENT_E: u8 = 1 << 4;
const SEGMENT_F: u8 = 1 << 5;
const SEGMENT_G: u8 = 1 << 6;
const SEGMENT_DP: u8 = 1 << 7;

/// One seven-segment glyph encoded as `a..g,dp` in bits `0..7`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SevenSegmentGlyph(u8);

impl SevenSegmentGlyph {
    /// One blank glyph with all segments off.
    pub const BLANK: Self = Self(0x00);
    /// One dash glyph using only the center segment.
    pub const DASH: Self = Self(SEGMENT_G);

    /// Creates one glyph from one raw segment mask.
    #[must_use]
    pub const fn from_raw(raw: u8) -> Self {
        Self(raw)
    }

    /// Returns the raw `a..g,dp` bitmask.
    #[must_use]
    pub const fn raw(self) -> u8 {
        self.0
    }

    /// Returns one glyph with the decimal point bit forced on/off.
    #[must_use]
    pub const fn with_decimal_point(self, enabled: bool) -> Self {
        if enabled {
            Self(self.0 | SEGMENT_DP)
        } else {
            Self(self.0 & !SEGMENT_DP)
        }
    }

    /// Returns one hexadecimal glyph for one nibble.
    #[must_use]
    pub const fn from_hex(value: u8) -> Option<Self> {
        Some(match value {
            0x0 => Self(SEGMENT_A | SEGMENT_B | SEGMENT_C | SEGMENT_D | SEGMENT_E | SEGMENT_F),
            0x1 => Self(SEGMENT_B | SEGMENT_C),
            0x2 => Self(SEGMENT_A | SEGMENT_B | SEGMENT_D | SEGMENT_E | SEGMENT_G),
            0x3 => Self(SEGMENT_A | SEGMENT_B | SEGMENT_C | SEGMENT_D | SEGMENT_G),
            0x4 => Self(SEGMENT_B | SEGMENT_C | SEGMENT_F | SEGMENT_G),
            0x5 => Self(SEGMENT_A | SEGMENT_C | SEGMENT_D | SEGMENT_F | SEGMENT_G),
            0x6 => Self(SEGMENT_A | SEGMENT_C | SEGMENT_D | SEGMENT_E | SEGMENT_F | SEGMENT_G),
            0x7 => Self(SEGMENT_A | SEGMENT_B | SEGMENT_C),
            0x8 => Self(
                SEGMENT_A | SEGMENT_B | SEGMENT_C | SEGMENT_D | SEGMENT_E | SEGMENT_F | SEGMENT_G,
            ),
            0x9 => Self(SEGMENT_A | SEGMENT_B | SEGMENT_C | SEGMENT_D | SEGMENT_F | SEGMENT_G),
            0xA => Self(SEGMENT_A | SEGMENT_B | SEGMENT_C | SEGMENT_E | SEGMENT_F | SEGMENT_G),
            0xB => Self(SEGMENT_C | SEGMENT_D | SEGMENT_E | SEGMENT_F | SEGMENT_G),
            0xC => Self(SEGMENT_A | SEGMENT_D | SEGMENT_E | SEGMENT_F),
            0xD => Self(SEGMENT_B | SEGMENT_C | SEGMENT_D | SEGMENT_E | SEGMENT_G),
            0xE => Self(SEGMENT_A | SEGMENT_D | SEGMENT_E | SEGMENT_F | SEGMENT_G),
            0xF => Self(SEGMENT_A | SEGMENT_E | SEGMENT_F | SEGMENT_G),
            _ => return None,
        })
    }

    /// Returns one glyph for one limited ASCII-like display character.
    #[must_use]
    pub const fn from_ascii(ch: char) -> Option<Self> {
        match ch {
            '0' => Self::from_hex(0x0),
            '1' => Self::from_hex(0x1),
            '2' => Self::from_hex(0x2),
            '3' => Self::from_hex(0x3),
            '4' => Self::from_hex(0x4),
            '5' => Self::from_hex(0x5),
            '6' => Self::from_hex(0x6),
            '7' => Self::from_hex(0x7),
            '8' => Self::from_hex(0x8),
            '9' => Self::from_hex(0x9),
            'A' | 'a' => Self::from_hex(0xA),
            'B' | 'b' => Self::from_hex(0xB),
            'C' | 'c' => Self::from_hex(0xC),
            'D' | 'd' => Self::from_hex(0xD),
            'E' | 'e' => Self::from_hex(0xE),
            'F' | 'f' => Self::from_hex(0xF),
            '-' => Some(Self::DASH),
            ' ' => Some(Self::BLANK),
            _ => None,
        }
    }
}

/// Electrical polarity for one multiplexed seven-segment module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SevenSegmentPolarity {
    /// Whether one asserted segment line is driven high.
    pub segment_active_high: bool,
    /// Whether one asserted digit-select line is driven high.
    pub digit_active_high: bool,
}

impl SevenSegmentPolarity {
    /// Returns one common-cathode electrical contract.
    #[must_use]
    pub const fn common_cathode() -> Self {
        Self {
            segment_active_high: true,
            digit_active_high: false,
        }
    }

    /// Returns one common-anode electrical contract.
    #[must_use]
    pub const fn common_anode() -> Self {
        Self {
            segment_active_high: false,
            digit_active_high: true,
        }
    }
}

/// One explicit GPIO pin map for one four-digit seven-segment display.
#[derive(Debug)]
pub struct FourDigitSevenSegmentPins<Seg, Dig> {
    pub a: Seg,
    pub b: Seg,
    pub c: Seg,
    pub d: Seg,
    pub e: Seg,
    pub f: Seg,
    pub g: Seg,
    pub dp: Seg,
    pub d1: Dig,
    pub d2: Dig,
    pub d3: Dig,
    pub d4: Dig,
}

impl<Seg, Dig> FourDigitSevenSegmentPins<Seg, Dig> {
    fn into_arrays(self) -> ([Seg; 8], [Dig; 4]) {
        (
            [
                self.a, self.b, self.c, self.d, self.e, self.f, self.g, self.dp,
            ],
            [self.d1, self.d2, self.d3, self.d4],
        )
    }
}

/// One four-digit multiplexed seven-segment display backed by owned GPIO outputs.
#[derive(Debug)]
pub struct FourDigitSevenSegmentDisplay<Seg, Dig> {
    segments: [Seg; 8],
    digits: [Dig; 4],
    polarity: SevenSegmentPolarity,
    glyphs: [SevenSegmentGlyph; 4],
    next_digit: usize,
}

impl<Seg, Dig> FourDigitSevenSegmentDisplay<Seg, Dig>
where
    Seg: GpioOutputPin,
    Dig: GpioOutputPin,
{
    /// Creates one display with one explicit electrical polarity contract.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when any backing pin cannot be configured for output.
    pub fn new(
        pins: FourDigitSevenSegmentPins<Seg, Dig>,
        polarity: SevenSegmentPolarity,
    ) -> Result<Self, GpioError> {
        let (mut segments, mut digits) = pins.into_arrays();
        for segment in &mut segments {
            segment.configure_output(false)?;
        }
        for digit in &mut digits {
            digit.configure_output(false)?;
        }
        let mut display = Self {
            segments,
            digits,
            polarity,
            glyphs: [SevenSegmentGlyph::BLANK; 4],
            next_digit: 0,
        };
        display.disable()?;
        Ok(display)
    }

    /// Creates one common-cathode display.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when any backing pin cannot be configured for output.
    pub fn common_cathode(pins: FourDigitSevenSegmentPins<Seg, Dig>) -> Result<Self, GpioError> {
        Self::new(pins, SevenSegmentPolarity::common_cathode())
    }

    /// Creates one common-anode display.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when any backing pin cannot be configured for output.
    pub fn common_anode(pins: FourDigitSevenSegmentPins<Seg, Dig>) -> Result<Self, GpioError> {
        Self::new(pins, SevenSegmentPolarity::common_anode())
    }

    /// Returns the configured electrical polarity.
    #[must_use]
    pub const fn polarity(&self) -> SevenSegmentPolarity {
        self.polarity
    }

    /// Returns the buffered glyphs in left-to-right digit order.
    #[must_use]
    pub const fn glyphs(&self) -> [SevenSegmentGlyph; 4] {
        self.glyphs
    }

    /// Overwrites all four buffered glyphs.
    pub fn set_glyphs(&mut self, glyphs: [SevenSegmentGlyph; 4]) {
        self.glyphs = glyphs;
    }

    /// Sets one buffered digit by index.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the digit index is out of range.
    pub fn set_digit(&mut self, index: usize, glyph: SevenSegmentGlyph) -> Result<(), GpioError> {
        if index >= self.glyphs.len() {
            return Err(GpioError::invalid());
        }
        self.glyphs[index] = glyph;
        Ok(())
    }

    /// Clears the framebuffer to blanks.
    pub fn clear(&mut self) {
        self.glyphs = [SevenSegmentGlyph::BLANK; 4];
    }

    /// Writes one four-nibble hexadecimal value into the framebuffer.
    pub fn set_hex(&mut self, value: u16) {
        self.glyphs = [
            SevenSegmentGlyph::from_hex(((value >> 12) & 0x0f) as u8)
                .expect("upper nibble should be valid"),
            SevenSegmentGlyph::from_hex(((value >> 8) & 0x0f) as u8)
                .expect("second nibble should be valid"),
            SevenSegmentGlyph::from_hex(((value >> 4) & 0x0f) as u8)
                .expect("third nibble should be valid"),
            SevenSegmentGlyph::from_hex((value & 0x0f) as u8)
                .expect("lower nibble should be valid"),
        ];
    }

    /// Deactivates all digits and blanks all segment outputs.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when any backing pin cannot be driven.
    pub fn disable(&mut self) -> Result<(), GpioError> {
        self.drive_digit_lines(None)?;
        self.drive_segments(SevenSegmentGlyph::BLANK)
    }

    /// Refreshes one specific digit of the display.
    ///
    /// This deactivates every digit, updates the shared segment lines for the requested glyph,
    /// and then asserts only the requested digit-select line. Call this at one steady cadence
    /// from one timer, fiber, or interrupt to maintain a stable multiplexed display.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the digit index is out of range or any backing pin
    /// cannot be driven.
    pub fn refresh_digit(&mut self, index: usize) -> Result<(), GpioError> {
        if index >= self.glyphs.len() {
            return Err(GpioError::invalid());
        }
        self.drive_digit_lines(None)?;
        self.drive_segments(self.glyphs[index])?;
        self.drive_digit_lines(Some(index))
    }

    /// Refreshes the next digit in one round-robin scan.
    ///
    /// Returns the digit index that was just driven.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when any backing pin cannot be driven.
    pub fn refresh_next(&mut self) -> Result<usize, GpioError> {
        let index = self.next_digit;
        self.refresh_digit(index)?;
        self.next_digit = (index + 1) % self.glyphs.len();
        Ok(index)
    }

    /// Releases the owned GPIO pins back to the caller.
    #[must_use]
    pub fn into_pins(self) -> FourDigitSevenSegmentPins<Seg, Dig> {
        let [a, b, c, d, e, f, g, dp] = self.segments;
        let [d1, d2, d3, d4] = self.digits;
        FourDigitSevenSegmentPins {
            a,
            b,
            c,
            d,
            e,
            f,
            g,
            dp,
            d1,
            d2,
            d3,
            d4,
        }
    }

    fn drive_segments(&mut self, glyph: SevenSegmentGlyph) -> Result<(), GpioError> {
        let mask = glyph.raw();
        for (index, segment) in self.segments.iter_mut().enumerate() {
            let asserted = ((mask >> index) & 1) != 0;
            segment.set_level(output_level(asserted, self.polarity.segment_active_high))?;
        }
        Ok(())
    }

    fn drive_digit_lines(&mut self, active: Option<usize>) -> Result<(), GpioError> {
        for (index, digit) in self.digits.iter_mut().enumerate() {
            let asserted = active == Some(index);
            digit.set_level(output_level(asserted, self.polarity.digit_active_high))?;
        }
        Ok(())
    }
}

/// One four-digit multiplexed seven-segment display backed by two chained 74HC595 devices.
///
/// The first shifted byte lands in the furthest register in the chain, and the last shifted byte
/// lands in the nearest register. This matches the common wiring pattern where one register drives
/// digit selects and the other drives segment lines.
#[derive(Debug)]
pub struct ShiftedFourDigitSevenSegmentDisplay<
    Data,
    ShiftClock,
    LatchClock,
    OutputEnable = NoOutputEnable,
> {
    register: ShiftRegister74hc595<Data, ShiftClock, LatchClock, OutputEnable>,
    polarity: SevenSegmentPolarity,
    glyphs: [SevenSegmentGlyph; 4],
    next_digit: usize,
}

impl<Data, ShiftClock, LatchClock>
    ShiftedFourDigitSevenSegmentDisplay<Data, ShiftClock, LatchClock, NoOutputEnable>
where
    Data: GpioOutputPin,
    ShiftClock: GpioOutputPin,
    LatchClock: GpioOutputPin,
{
    /// Creates one shifted display with hardware-tied output enable.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when any backing pin cannot be configured for output.
    pub fn new(
        data: Data,
        shift_clock: ShiftClock,
        latch_clock: LatchClock,
        polarity: SevenSegmentPolarity,
    ) -> Result<Self, GpioError> {
        let register = ShiftRegister74hc595::new(data, shift_clock, latch_clock)?;
        Self::from_register(register, polarity)
    }

    /// Creates one shifted common-cathode display.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when any backing pin cannot be configured for output.
    pub fn common_cathode(
        data: Data,
        shift_clock: ShiftClock,
        latch_clock: LatchClock,
    ) -> Result<Self, GpioError> {
        Self::new(
            data,
            shift_clock,
            latch_clock,
            SevenSegmentPolarity::common_cathode(),
        )
    }

    /// Creates one shifted common-anode display.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when any backing pin cannot be configured for output.
    pub fn common_anode(
        data: Data,
        shift_clock: ShiftClock,
        latch_clock: LatchClock,
    ) -> Result<Self, GpioError> {
        Self::new(
            data,
            shift_clock,
            latch_clock,
            SevenSegmentPolarity::common_anode(),
        )
    }
}

impl<Data, ShiftClock, LatchClock, OutputEnable>
    ShiftedFourDigitSevenSegmentDisplay<Data, ShiftClock, LatchClock, OutputEnable>
where
    Data: GpioOutputPin,
    ShiftClock: GpioOutputPin,
    LatchClock: GpioOutputPin,
    OutputEnable: GpioOutputPin,
{
    /// Creates one shifted display with one software-controlled output-enable pin wired to CE/OE.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when any backing pin cannot be configured for output.
    pub fn with_output_enable(
        data: Data,
        shift_clock: ShiftClock,
        latch_clock: LatchClock,
        output_enable: OutputEnable,
        polarity: SevenSegmentPolarity,
    ) -> Result<Self, GpioError> {
        let register = ShiftRegister74hc595::with_output_enable(
            data,
            shift_clock,
            latch_clock,
            output_enable,
        )?;
        Self::from_register(register, polarity)
    }
}

impl<Data, ShiftClock, LatchClock, OutputEnable>
    ShiftedFourDigitSevenSegmentDisplay<Data, ShiftClock, LatchClock, OutputEnable>
where
    Data: GpioOutputPin,
    ShiftClock: GpioOutputPin,
    LatchClock: GpioOutputPin,
    OutputEnable: OutputEnableControl,
{
    fn from_register(
        register: ShiftRegister74hc595<Data, ShiftClock, LatchClock, OutputEnable>,
        polarity: SevenSegmentPolarity,
    ) -> Result<Self, GpioError> {
        let mut display = Self {
            register,
            polarity,
            glyphs: [SevenSegmentGlyph::BLANK; 4],
            next_digit: 0,
        };
        display.disable()?;
        Ok(display)
    }

    /// Returns the configured electrical polarity.
    #[must_use]
    pub const fn polarity(&self) -> SevenSegmentPolarity {
        self.polarity
    }

    /// Returns the buffered glyphs in left-to-right digit order.
    #[must_use]
    pub const fn glyphs(&self) -> [SevenSegmentGlyph; 4] {
        self.glyphs
    }

    /// Overwrites all four buffered glyphs.
    pub fn set_glyphs(&mut self, glyphs: [SevenSegmentGlyph; 4]) {
        self.glyphs = glyphs;
    }

    /// Sets one buffered digit by index.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the digit index is out of range.
    pub fn set_digit(&mut self, index: usize, glyph: SevenSegmentGlyph) -> Result<(), GpioError> {
        if index >= self.glyphs.len() {
            return Err(GpioError::invalid());
        }
        self.glyphs[index] = glyph;
        Ok(())
    }

    /// Clears the framebuffer to blanks.
    pub fn clear(&mut self) {
        self.glyphs = [SevenSegmentGlyph::BLANK; 4];
    }

    /// Writes one four-nibble hexadecimal value into the framebuffer.
    pub fn set_hex(&mut self, value: u16) {
        self.glyphs = [
            SevenSegmentGlyph::from_hex(((value >> 12) & 0x0f) as u8)
                .expect("upper nibble should be valid"),
            SevenSegmentGlyph::from_hex(((value >> 8) & 0x0f) as u8)
                .expect("second nibble should be valid"),
            SevenSegmentGlyph::from_hex(((value >> 4) & 0x0f) as u8)
                .expect("third nibble should be valid"),
            SevenSegmentGlyph::from_hex((value & 0x0f) as u8)
                .expect("lower nibble should be valid"),
        ];
    }

    /// Deactivates all digits and blanks all segment outputs.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the shift-register pins cannot be driven.
    pub fn disable(&mut self) -> Result<(), GpioError> {
        self.drive_state(None)
    }

    /// Refreshes one specific digit through the chained shift registers.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the digit index is out of range or the register chain
    /// cannot be driven.
    pub fn refresh_digit(&mut self, index: usize) -> Result<(), GpioError> {
        if index >= self.glyphs.len() {
            return Err(GpioError::invalid());
        }
        self.drive_state(Some(index))
    }

    /// Refreshes the next digit in one round-robin scan.
    ///
    /// Returns the digit index that was just driven.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the register chain cannot be driven.
    pub fn refresh_next(&mut self) -> Result<usize, GpioError> {
        let index = self.next_digit;
        self.refresh_digit(index)?;
        self.next_digit = (index + 1) % self.glyphs.len();
        Ok(index)
    }

    fn drive_state(&mut self, active_digit: Option<usize>) -> Result<(), GpioError> {
        let digit_index = active_digit.unwrap_or(0);
        let digit_mask = digit_output_byte(active_digit, self.polarity);
        let segment_mask = if active_digit.is_some() {
            segment_output_byte(self.glyphs[digit_index], self.polarity)
        } else {
            segment_output_byte(SevenSegmentGlyph::BLANK, self.polarity)
        };
        self.register
            .write_bytes_msb_first(&[digit_mask, segment_mask])
    }
}

const fn segment_output_byte(glyph: SevenSegmentGlyph, polarity: SevenSegmentPolarity) -> u8 {
    if polarity.segment_active_high {
        glyph.raw()
    } else {
        !glyph.raw()
    }
}

const fn digit_output_byte(active: Option<usize>, polarity: SevenSegmentPolarity) -> u8 {
    let inactive = output_level(false, polarity.digit_active_high);
    let active_level = output_level(true, polarity.digit_active_high);
    let base = if inactive { 0xff } else { 0x00 };
    match active {
        Some(index) if index < 4 => {
            let bit = 1u8 << index;
            if active_level {
                (base & !bit) | bit
            } else {
                base & !bit
            }
        }
        _ => base,
    }
}

const fn output_level(asserted: bool, active_high: bool) -> bool {
    if active_high { asserted } else { !asserted }
}

const fn peripheral_glyph_to_native(glyph: PeripheralSevenSegmentGlyph) -> SevenSegmentGlyph {
    SevenSegmentGlyph::from_raw(glyph.raw())
}

const fn native_glyph_to_peripheral(glyph: SevenSegmentGlyph) -> PeripheralSevenSegmentGlyph {
    PeripheralSevenSegmentGlyph::from_raw(glyph.raw())
}

const fn native_polarity_to_peripheral(
    polarity: SevenSegmentPolarity,
) -> PeripheralSevenSegmentPolarity {
    PeripheralSevenSegmentPolarity {
        segment_active_high: polarity.segment_active_high,
        digit_active_high: polarity.digit_active_high,
    }
}

impl<Seg, Dig> SevenSegmentDisplayContract<4> for FourDigitSevenSegmentDisplay<Seg, Dig>
where
    Seg: GpioOutputPin,
    Dig: GpioOutputPin,
{
    type Error = GpioError;

    fn polarity(&self) -> PeripheralSevenSegmentPolarity {
        native_polarity_to_peripheral(self.polarity())
    }

    fn glyphs(&self) -> [PeripheralSevenSegmentGlyph; 4] {
        self.glyphs().map(native_glyph_to_peripheral)
    }

    fn set_glyphs(&mut self, glyphs: [PeripheralSevenSegmentGlyph; 4]) {
        Self::set_glyphs(self, glyphs.map(peripheral_glyph_to_native));
    }

    fn set_digit(
        &mut self,
        index: usize,
        glyph: PeripheralSevenSegmentGlyph,
    ) -> Result<(), Self::Error> {
        Self::set_digit(self, index, peripheral_glyph_to_native(glyph))
    }

    fn clear(&mut self) {
        Self::clear(self)
    }

    fn disable(&mut self) -> Result<(), Self::Error> {
        Self::disable(self)
    }

    fn refresh_digit(&mut self, index: usize) -> Result<(), Self::Error> {
        Self::refresh_digit(self, index)
    }

    fn refresh_next(&mut self) -> Result<usize, Self::Error> {
        Self::refresh_next(self)
    }
}

impl<Data, ShiftClock, LatchClock, OutputEnable> SevenSegmentDisplayContract<4>
    for ShiftedFourDigitSevenSegmentDisplay<Data, ShiftClock, LatchClock, OutputEnable>
where
    Data: GpioOutputPin,
    ShiftClock: GpioOutputPin,
    LatchClock: GpioOutputPin,
    OutputEnable: OutputEnableControl,
{
    type Error = GpioError;

    fn polarity(&self) -> PeripheralSevenSegmentPolarity {
        native_polarity_to_peripheral(self.polarity())
    }

    fn glyphs(&self) -> [PeripheralSevenSegmentGlyph; 4] {
        self.glyphs().map(native_glyph_to_peripheral)
    }

    fn set_glyphs(&mut self, glyphs: [PeripheralSevenSegmentGlyph; 4]) {
        Self::set_glyphs(self, glyphs.map(peripheral_glyph_to_native));
    }

    fn set_digit(
        &mut self,
        index: usize,
        glyph: PeripheralSevenSegmentGlyph,
    ) -> Result<(), Self::Error> {
        Self::set_digit(self, index, peripheral_glyph_to_native(glyph))
    }

    fn clear(&mut self) {
        Self::clear(self)
    }

    fn disable(&mut self) -> Result<(), Self::Error> {
        Self::disable(self)
    }

    fn refresh_digit(&mut self, index: usize) -> Result<(), Self::Error> {
        Self::refresh_digit(self, index)
    }

    fn refresh_next(&mut self) -> Result<usize, Self::Error> {
        Self::refresh_next(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpio::{GpioCapabilities, GpioOwnedPin};

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

    fn fake_pins() -> FourDigitSevenSegmentPins<FakeOutputPin, FakeOutputPin> {
        FourDigitSevenSegmentPins {
            a: FakeOutputPin {
                pin: 11,
                configured: false,
                level: false,
            },
            b: FakeOutputPin {
                pin: 7,
                configured: false,
                level: false,
            },
            c: FakeOutputPin {
                pin: 4,
                configured: false,
                level: false,
            },
            d: FakeOutputPin {
                pin: 2,
                configured: false,
                level: false,
            },
            e: FakeOutputPin {
                pin: 1,
                configured: false,
                level: false,
            },
            f: FakeOutputPin {
                pin: 10,
                configured: false,
                level: false,
            },
            g: FakeOutputPin {
                pin: 5,
                configured: false,
                level: false,
            },
            dp: FakeOutputPin {
                pin: 3,
                configured: false,
                level: false,
            },
            d1: FakeOutputPin {
                pin: 12,
                configured: false,
                level: false,
            },
            d2: FakeOutputPin {
                pin: 9,
                configured: false,
                level: false,
            },
            d3: FakeOutputPin {
                pin: 8,
                configured: false,
                level: false,
            },
            d4: FakeOutputPin {
                pin: 6,
                configured: false,
                level: false,
            },
        }
    }

    #[test]
    fn hex_glyph_table_matches_expected_segment_codes() {
        assert_eq!(
            SevenSegmentGlyph::from_hex(0x0).expect("0"),
            SevenSegmentGlyph::from_raw(0x3f)
        );
        assert_eq!(
            SevenSegmentGlyph::from_hex(0x1).expect("1"),
            SevenSegmentGlyph::from_raw(0x06)
        );
        assert_eq!(
            SevenSegmentGlyph::from_hex(0xA).expect("A"),
            SevenSegmentGlyph::from_raw(0x77)
        );
        assert_eq!(
            SevenSegmentGlyph::from_hex(0xF).expect("F"),
            SevenSegmentGlyph::from_raw(0x71)
        );
    }

    #[test]
    fn common_cathode_refresh_drives_requested_digit_and_segments() {
        let mut display = FourDigitSevenSegmentDisplay::common_cathode(fake_pins())
            .expect("pins should configure");
        display
            .set_digit(0, SevenSegmentGlyph::from_hex(2).expect("hex glyph"))
            .expect("digit index should be valid");
        display.refresh_digit(0).expect("digit should refresh");

        let pins = display.into_pins();
        assert!(pins.a.level);
        assert!(pins.b.level);
        assert!(!pins.c.level);
        assert!(pins.d.level);
        assert!(pins.e.level);
        assert!(!pins.f.level);
        assert!(pins.g.level);
        assert!(!pins.dp.level);

        assert!(!pins.d1.level);
        assert!(pins.d2.level);
        assert!(pins.d3.level);
        assert!(pins.d4.level);
    }

    #[test]
    fn common_anode_refresh_inverts_segment_and_digit_polarity() {
        let mut display =
            FourDigitSevenSegmentDisplay::common_anode(fake_pins()).expect("pins should configure");
        display.set_hex(0x0001);
        display.refresh_digit(3).expect("digit should refresh");

        let pins = display.into_pins();
        assert!(pins.a.level);
        assert!(!pins.b.level);
        assert!(!pins.c.level);
        assert!(pins.d.level);
        assert!(pins.e.level);
        assert!(pins.f.level);
        assert!(pins.g.level);

        assert!(pins.d4.level);
        assert!(!pins.d1.level);
        assert!(!pins.d2.level);
        assert!(!pins.d3.level);
    }

    #[test]
    fn refresh_next_rotates_through_digits() {
        let mut display = FourDigitSevenSegmentDisplay::common_cathode(fake_pins())
            .expect("pins should configure");
        assert_eq!(display.refresh_next().expect("first scan"), 0);
        assert_eq!(display.refresh_next().expect("second scan"), 1);
        assert_eq!(display.refresh_next().expect("third scan"), 2);
        assert_eq!(display.refresh_next().expect("fourth scan"), 3);
        assert_eq!(display.refresh_next().expect("wrap"), 0);
    }
}
