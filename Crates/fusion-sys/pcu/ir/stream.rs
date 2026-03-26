//! Minimal semantic stream-kernel IR vocabulary for PCU dispatch.

use super::{PcuIrKind, PcuKernelId, PcuKernelIr};

/// Stream element types surfaced by the current stream dialect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuStreamValueType {
    U8,
    U16,
    U32,
}

impl PcuStreamValueType {
    /// Returns the lane width for this stream element type.
    #[must_use]
    pub const fn bit_width(self) -> u8 {
        match self {
            Self::U8 => 8,
            Self::U16 => 16,
            Self::U32 => 32,
        }
    }
}

/// Resource class for one stream-kernel binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuStreamBindingClass {
    Input,
    Output,
}

/// One explicit stream-kernel binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuStreamBinding<'a> {
    pub name: Option<&'a str>,
    pub class: PcuStreamBindingClass,
    pub value_type: PcuStreamValueType,
}

/// One semantic parameterized stream function/pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuStreamPattern {
    BitReverse,
    BitInvert,
    Increment,
    ShiftLeft { bits: u8 },
    ShiftRight { bits: u8 },
    ExtractBits { offset: u8, width: u8 },
    MaskLower { bits: u8 },
    ByteSwap32,
}

impl PcuStreamPattern {
    /// Returns whether this pattern carries prepare-time specialization parameters.
    #[must_use]
    pub const fn is_specialized(self) -> bool {
        matches!(
            self,
            Self::ShiftLeft { .. }
                | Self::ShiftRight { .. }
                | Self::ExtractBits { .. }
                | Self::MaskLower { .. }
        )
    }

    /// Returns whether this pattern is semantically valid for one stream element type.
    #[must_use]
    pub const fn supports_value_type(self, value_type: PcuStreamValueType) -> bool {
        let bit_width = value_type.bit_width();
        match self {
            Self::BitReverse | Self::BitInvert | Self::Increment => true,
            Self::ShiftLeft { bits } | Self::ShiftRight { bits } => bits >= 1 && bits <= bit_width,
            Self::ExtractBits { offset, width } => {
                width >= 1
                    && width <= bit_width
                    && offset < bit_width
                    && (offset as u16 + width as u16) <= bit_width as u16
            }
            Self::MaskLower { bits } => bits >= 1 && bits <= bit_width,
            Self::ByteSwap32 => matches!(value_type, PcuStreamValueType::U32),
        }
    }
}

/// Back-compat alias while the higher layers stop calling semantic patterns “ops”.
pub type PcuStreamOp = PcuStreamPattern;

bitflags::bitflags! {
    /// Coarse stream-dialect capabilities required by one kernel.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PcuStreamCapabilities: u32 {
        const FIFO_INPUT          = 1 << 0;
        const FIFO_OUTPUT         = 1 << 1;
        const BIT_REVERSE         = 1 << 2;
        const BIT_INVERT          = 1 << 3;
        const INCREMENT           = 1 << 4;
        const SHIFT_LEFT          = 1 << 5;
        const SHIFT_RIGHT         = 1 << 6;
        const EXTRACT_BITS        = 1 << 7;
        const MASK_LOWER          = 1 << 8;
        const BYTE_SWAP32         = 1 << 9;
        const PRECISE_DELAY       = 1 << 10;
        const PIN_PARALLEL_OUTPUT = 1 << 11;
    }
}

/// Minimal semantic stream-kernel IR payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuStreamKernelIr<'a> {
    pub id: PcuKernelId,
    pub entry_point: &'a str,
    pub bindings: &'a [PcuStreamBinding<'a>],
    pub patterns: &'a [PcuStreamPattern],
    pub capabilities: PcuStreamCapabilities,
}

impl PcuStreamKernelIr<'_> {
    /// Returns the simple input/output transform element type, when this kernel is one unary
    /// typed stream transform.
    #[must_use]
    pub fn simple_transform_type(&self) -> Option<PcuStreamValueType> {
        match self.bindings {
            [
                PcuStreamBinding {
                    class: PcuStreamBindingClass::Input,
                    value_type,
                    ..
                },
                PcuStreamBinding {
                    class: PcuStreamBindingClass::Output,
                    value_type: output_type,
                    ..
                },
            ] if value_type == output_type => Some(*value_type),
            _ => None,
        }
    }

    /// Returns whether the bound stream patterns are semantically valid for the simple transform
    /// shape carried by this kernel.
    #[must_use]
    pub fn simple_transform_patterns_are_valid(&self) -> bool {
        let Some(value_type) = self.simple_transform_type() else {
            return false;
        };
        self.patterns
            .iter()
            .copied()
            .all(|pattern| pattern.supports_value_type(value_type))
    }
}

impl PcuKernelIr for PcuStreamKernelIr<'_> {
    fn id(&self) -> PcuKernelId {
        self.id
    }

    fn kind(&self) -> PcuIrKind {
        PcuIrKind::Stream
    }

    fn entry_point(&self) -> &str {
        self.entry_point
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_ir_reports_simple_transform_shape() {
        let bindings = [
            PcuStreamBinding {
                name: Some("input"),
                class: PcuStreamBindingClass::Input,
                value_type: PcuStreamValueType::U32,
            },
            PcuStreamBinding {
                name: Some("output"),
                class: PcuStreamBindingClass::Output,
                value_type: PcuStreamValueType::U32,
            },
        ];
        let patterns = [PcuStreamPattern::BitReverse];
        let kernel = PcuStreamKernelIr {
            id: PcuKernelId(7),
            entry_point: "bit_reverse",
            bindings: &bindings,
            patterns: &patterns,
            capabilities: PcuStreamCapabilities::FIFO_INPUT
                | PcuStreamCapabilities::FIFO_OUTPUT
                | PcuStreamCapabilities::BIT_REVERSE,
        };

        assert_eq!(kernel.kind(), PcuIrKind::Stream);
        assert_eq!(kernel.entry_point(), "bit_reverse");
        assert_eq!(
            kernel.simple_transform_type(),
            Some(PcuStreamValueType::U32)
        );
        assert!(kernel.simple_transform_patterns_are_valid());
    }

    #[test]
    fn stream_pattern_validation_respects_specialization_bounds() {
        assert!(
            PcuStreamPattern::ShiftLeft { bits: 8 }.supports_value_type(PcuStreamValueType::U8)
        );
        assert!(
            !PcuStreamPattern::ExtractBits {
                offset: 7,
                width: 2
            }
            .supports_value_type(PcuStreamValueType::U8)
        );
        assert!(!PcuStreamPattern::ByteSwap32.supports_value_type(PcuStreamValueType::U16));
    }
}
