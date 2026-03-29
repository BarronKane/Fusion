//! Typed runtime-parameter vocabulary shared across PCU IR profiles.
//!
//! Parameters are neither memory bindings nor ports:
//! - bindings describe attached storage/resources
//! - ports describe dataflow edges
//! - parameters describe small submit-time values that specialize execution without changing the
//!   registered kernel structure itself
//!
//! That split matters directly for Fusion's PCU model. A runtime parameter should not force a new
//! kernel registration just because some poor backend wants to shift by `5` instead of `3`.

use super::PcuValueType;

/// Stable slot naming one runtime parameter in one kernel signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuParameterSlot(pub u8);

/// One declared runtime parameter in one kernel signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuParameter<'a> {
    pub slot: PcuParameterSlot,
    pub name: Option<&'a str>,
    pub value_type: PcuValueType,
}

impl<'a> PcuParameter<'a> {
    /// Creates one named runtime parameter declaration.
    #[must_use]
    pub const fn named(slot: PcuParameterSlot, name: &'a str, value_type: PcuValueType) -> Self {
        Self {
            slot,
            name: Some(name),
            value_type,
        }
    }

    /// Creates one anonymous runtime parameter declaration.
    #[must_use]
    pub const fn anonymous(slot: PcuParameterSlot, value_type: PcuValueType) -> Self {
        Self {
            slot,
            name: None,
            value_type,
        }
    }
}

/// One scalar runtime parameter value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuParameterValue {
    Bool(bool),
    I8(i8),
    U8(u8),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    F16(u16),
    F32(u32),
}

impl PcuParameterValue {
    /// Creates one half-precision floating-point runtime value from its raw IEEE-754 bit pattern.
    #[must_use]
    pub const fn from_f16_bits(bits: u16) -> Self {
        Self::F16(bits)
    }

    /// Creates one single-precision floating-point runtime value from one native `f32`.
    #[must_use]
    pub fn from_f32(value: f32) -> Self {
        Self::F32(value.to_bits())
    }

    /// Creates one single-precision floating-point runtime value from its raw IEEE-754 bit
    /// pattern.
    #[must_use]
    pub const fn from_f32_bits(bits: u32) -> Self {
        Self::F32(bits)
    }

    /// Returns the truthful PCU value type carried by this runtime parameter value.
    #[must_use]
    pub const fn value_type(self) -> PcuValueType {
        match self {
            Self::Bool(_) => PcuValueType::bool(),
            Self::I8(_) => PcuValueType::i8(),
            Self::U8(_) => PcuValueType::u8(),
            Self::I16(_) => PcuValueType::i16(),
            Self::U16(_) => PcuValueType::u16(),
            Self::I32(_) => PcuValueType::i32(),
            Self::U32(_) => PcuValueType::u32(),
            Self::F16(_) => PcuValueType::f16(),
            Self::F32(_) => PcuValueType::f32(),
        }
    }

    /// Returns whether this runtime value satisfies one declared PCU value type exactly.
    #[must_use]
    pub fn matches_type(self, value_type: PcuValueType) -> bool {
        self.value_type() == value_type
    }

    /// Returns this runtime value as one `u8`, when that is the truthful carrier type.
    #[must_use]
    pub const fn as_u8(self) -> Option<u8> {
        match self {
            Self::U8(value) => Some(value),
            _ => None,
        }
    }

    /// Returns this runtime value as one `u16`, when that is the truthful carrier type.
    #[must_use]
    pub const fn as_u16(self) -> Option<u16> {
        match self {
            Self::U16(value) => Some(value),
            _ => None,
        }
    }

    /// Returns this runtime value as one `u32`, when that is the truthful carrier type.
    #[must_use]
    pub const fn as_u32(self) -> Option<u32> {
        match self {
            Self::U32(value) => Some(value),
            _ => None,
        }
    }

    /// Returns this runtime value as one raw `f16` bit pattern, when that is the truthful
    /// carrier type.
    #[must_use]
    pub const fn as_f16_bits(self) -> Option<u16> {
        match self {
            Self::F16(bits) => Some(bits),
            _ => None,
        }
    }

    /// Returns this runtime value as one native `f32`, when that is the truthful carrier type.
    #[must_use]
    pub fn as_f32(self) -> Option<f32> {
        match self {
            Self::F32(bits) => Some(f32::from_bits(bits)),
            _ => None,
        }
    }

    /// Returns this runtime value as one raw `f32` bit pattern, when that is the truthful
    /// carrier type.
    #[must_use]
    pub const fn as_f32_bits(self) -> Option<u32> {
        match self {
            Self::F32(bits) => Some(bits),
            _ => None,
        }
    }
}

/// One submit-time binding from a declared parameter slot to one runtime value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuParameterBinding {
    pub slot: PcuParameterSlot,
    pub value: PcuParameterValue,
}

impl PcuParameterBinding {
    /// Creates one submit-time runtime-parameter binding.
    #[must_use]
    pub const fn new(slot: PcuParameterSlot, value: PcuParameterValue) -> Self {
        Self { slot, value }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::drivers::pcu::PcuValueType;

    #[test]
    fn parameter_values_report_truthful_types() {
        assert_eq!(PcuParameterValue::U32(7).value_type(), PcuValueType::u32());
        assert!(PcuParameterValue::U8(3).matches_type(PcuValueType::u8()));
        assert!(!PcuParameterValue::U8(3).matches_type(PcuValueType::u16()));
    }

    #[test]
    fn floating_parameter_helpers_preserve_raw_bits() {
        let f16_bits = 0b0_01111_1000000000_u16;
        let f32_bits = 0x7fc0_0001_u32;

        assert_eq!(
            PcuParameterValue::from_f16_bits(f16_bits).as_f16_bits(),
            Some(f16_bits)
        );
        assert_eq!(
            PcuParameterValue::from_f32_bits(f32_bits).as_f32_bits(),
            Some(f32_bits)
        );
        assert_eq!(
            PcuParameterValue::from_f32(1.5)
                .as_f32()
                .expect("f32 runtime value should decode"),
            1.5
        );
    }
}
