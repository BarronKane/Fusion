//! Seven-segment peripheral contracts.

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

/// Multiplexed seven-segment display contract.
pub trait SevenSegmentDisplayContract<const DIGITS: usize> {
    /// Concrete backend or composition error.
    type Error;

    /// Returns the configured electrical polarity.
    fn polarity(&self) -> SevenSegmentPolarity;

    /// Returns the buffered glyphs in left-to-right digit order.
    fn glyphs(&self) -> [SevenSegmentGlyph; DIGITS];

    /// Overwrites all buffered glyphs.
    fn set_glyphs(&mut self, glyphs: [SevenSegmentGlyph; DIGITS]);

    /// Sets one buffered digit by index.
    ///
    /// # Errors
    ///
    /// Returns an honest backend error when the digit index is out of range.
    fn set_digit(&mut self, index: usize, glyph: SevenSegmentGlyph) -> Result<(), Self::Error>;

    /// Clears the framebuffer to blanks.
    fn clear(&mut self);

    /// Deactivates all digits and blanks all outputs.
    ///
    /// # Errors
    ///
    /// Returns an honest backend error when the display cannot be driven.
    fn disable(&mut self) -> Result<(), Self::Error>;

    /// Refreshes one specific digit.
    ///
    /// # Errors
    ///
    /// Returns an honest backend error when the digit index is out of range or the display
    /// cannot be driven.
    fn refresh_digit(&mut self, index: usize) -> Result<(), Self::Error>;

    /// Refreshes the next digit in one round-robin scan.
    ///
    /// Returns the digit index that was just driven.
    ///
    /// # Errors
    ///
    /// Returns an honest backend error when the display cannot be driven.
    fn refresh_next(&mut self) -> Result<usize, Self::Error>;
}
