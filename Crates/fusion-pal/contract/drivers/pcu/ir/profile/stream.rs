//! Continuous-I/O stream profile layered over the PCU IR core.

use super::super::{
    PcuBinding,
    PcuInvocationModel,
    PcuIrKind,
    PcuKernelId,
    PcuKernelIr,
    PcuKernelSignature,
    PcuParameter,
    PcuParameterSlot,
    PcuPort,
    PcuPortDirection,
    PcuPortRate,
    PcuScalarType,
    PcuValueType,
};

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

impl PcuStreamValueType {
    /// Returns the corresponding PCU core value type.
    #[must_use]
    pub const fn as_value_type(self) -> PcuValueType {
        match self {
            Self::U8 => PcuValueType::Scalar(PcuScalarType::U8),
            Self::U16 => PcuValueType::Scalar(PcuScalarType::U16),
            Self::U32 => PcuValueType::Scalar(PcuScalarType::U32),
        }
    }

    /// Attempts to recover one stream value type from one PCU core value type.
    #[must_use]
    pub const fn from_value_type(value_type: PcuValueType) -> Option<Self> {
        match value_type {
            PcuValueType::Scalar(PcuScalarType::U8) => Some(Self::U8),
            PcuValueType::Scalar(PcuScalarType::U16) => Some(Self::U16),
            PcuValueType::Scalar(PcuScalarType::U32) => Some(Self::U32),
            _ => None,
        }
    }
}

/// One semantic parameterized stream function/pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuStreamPattern {
    BitReverse,
    BitInvert,
    Increment,
    AddParameter { parameter: PcuParameterSlot },
    XorParameter { parameter: PcuParameterSlot },
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
            Self::BitReverse
            | Self::BitInvert
            | Self::Increment
            | Self::AddParameter { .. }
            | Self::XorParameter { .. } => true,
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
        const ADD_PARAMETER       = 1 << 5;
        const XOR_PARAMETER       = 1 << 6;
        const SHIFT_LEFT          = 1 << 7;
        const SHIFT_RIGHT         = 1 << 8;
        const EXTRACT_BITS        = 1 << 9;
        const MASK_LOWER          = 1 << 10;
        const BYTE_SWAP32         = 1 << 11;
        const PRECISE_DELAY       = 1 << 12;
        const PIN_PARALLEL_OUTPUT = 1 << 13;
    }
}

/// Minimal semantic stream-kernel IR payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuStreamKernelIr<'a> {
    pub id: PcuKernelId,
    pub entry_point: &'a str,
    pub bindings: &'a [PcuBinding<'a>],
    pub ports: &'a [PcuPort<'a>],
    pub parameters: &'a [PcuParameter<'a>],
    pub patterns: &'a [PcuStreamPattern],
    pub capabilities: PcuStreamCapabilities,
}

impl PcuStreamKernelIr<'_> {
    fn parameter(&self, slot: PcuParameterSlot) -> Option<PcuParameter<'_>> {
        self.parameters
            .iter()
            .copied()
            .find(|parameter| parameter.slot == slot)
    }

    fn parameter_declarations_are_valid(&self) -> bool {
        for (index, parameter) in self.parameters.iter().enumerate() {
            if self.parameters[..index]
                .iter()
                .any(|existing| existing.slot == parameter.slot)
            {
                return false;
            }
        }
        true
    }

    fn pattern_parameter_declarations_are_valid(&self, value_type: PcuStreamValueType) -> bool {
        self.patterns.iter().copied().all(|pattern| match pattern {
            PcuStreamPattern::AddParameter { parameter }
            | PcuStreamPattern::XorParameter { parameter } => self
                .parameter(parameter)
                .is_some_and(|declared| declared.value_type == value_type.as_value_type()),
            _ => true,
        })
    }

    /// Returns the simple input/output transform element type, when this kernel is one unary
    /// typed stream transform.
    #[must_use]
    pub fn simple_transform_type(&self) -> Option<PcuStreamValueType> {
        let [input, output] = self.ports else {
            return None;
        };
        if input.direction != PcuPortDirection::Input
            || output.direction != PcuPortDirection::Output
            || input.rate != PcuPortRate::Stream
            || output.rate != PcuPortRate::Stream
        {
            return None;
        }
        let input_type = PcuStreamValueType::from_value_type(input.value_type)?;
        let output_type = PcuStreamValueType::from_value_type(output.value_type)?;
        (input_type == output_type).then_some(input_type)
    }

    /// Returns whether the bound stream patterns are semantically valid for the simple transform
    /// shape carried by this kernel.
    #[must_use]
    pub fn simple_transform_patterns_are_valid(&self) -> bool {
        let Some(value_type) = self.simple_transform_type() else {
            return false;
        };
        if !self.parameter_declarations_are_valid()
            || !self.pattern_parameter_declarations_are_valid(value_type)
        {
            return false;
        }
        self.patterns
            .iter()
            .copied()
            .all(|pattern| pattern.supports_value_type(value_type))
    }

    /// Returns whether the supplied runtime-parameter table satisfies this stream kernel's
    /// declared parameter contract.
    #[must_use]
    pub fn invocation_parameters_are_valid(
        &self,
        parameters: crate::contract::drivers::pcu::PcuInvocationParameters<'_>,
    ) -> bool {
        parameters.validate_against(self.parameters)
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

    fn signature(&self) -> PcuKernelSignature<'_> {
        PcuKernelSignature {
            bindings: self.bindings,
            ports: self.ports,
            parameters: self.parameters,
            invocation: PcuInvocationModel::continuous(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::drivers::pcu::{
        PcuParameterBinding,
        PcuParameterValue,
    };

    #[test]
    fn stream_ir_reports_simple_transform_shape() {
        let ports = [
            PcuPort::stream_input(Some("input"), PcuStreamValueType::U32.as_value_type()),
            PcuPort::stream_output(Some("output"), PcuStreamValueType::U32.as_value_type()),
        ];
        let patterns = [PcuStreamPattern::BitReverse];
        let kernel = PcuStreamKernelIr {
            id: PcuKernelId(7),
            entry_point: "bit_reverse",
            bindings: &[],
            ports: &ports,
            parameters: &[],
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
        assert_eq!(
            kernel.signature().invocation,
            PcuInvocationModel::continuous()
        );
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

    #[test]
    fn stream_ir_validates_runtime_parameter_declarations_and_bindings() {
        let ports = [
            PcuPort::stream_input(Some("input"), PcuStreamValueType::U32.as_value_type()),
            PcuPort::stream_output(Some("output"), PcuStreamValueType::U32.as_value_type()),
        ];
        let parameters = [PcuParameter::named(
            PcuParameterSlot(0),
            "value",
            PcuValueType::u32(),
        )];
        let patterns = [PcuStreamPattern::XorParameter {
            parameter: PcuParameterSlot(0),
        }];
        let kernel = PcuStreamKernelIr {
            id: PcuKernelId(8),
            entry_point: "xor_parameter",
            bindings: &[],
            ports: &ports,
            parameters: &parameters,
            patterns: &patterns,
            capabilities: PcuStreamCapabilities::FIFO_INPUT
                | PcuStreamCapabilities::FIFO_OUTPUT
                | PcuStreamCapabilities::XOR_PARAMETER,
        };

        assert!(kernel.simple_transform_patterns_are_valid());
        assert!(kernel.invocation_parameters_are_valid(
            crate::contract::drivers::pcu::PcuInvocationParameters {
                bindings: &[PcuParameterBinding::new(
                    PcuParameterSlot(0),
                    PcuParameterValue::U32(0x55aa_55aa),
                )],
            }
        ));
        assert!(!kernel.invocation_parameters_are_valid(
            crate::contract::drivers::pcu::PcuInvocationParameters {
                bindings: &[PcuParameterBinding::new(
                    PcuParameterSlot(0),
                    PcuParameterValue::U16(7),
                )],
            }
        ));
    }
}
