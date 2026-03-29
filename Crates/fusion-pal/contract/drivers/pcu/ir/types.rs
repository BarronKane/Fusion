//! Core typed value vocabulary shared across PCU IR profiles.

/// Scalar element types surfaced by the current PCU IR core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuScalarType {
    Bool,
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F16,
    F32,
}

impl PcuScalarType {
    /// Returns the honest bit width for this scalar type.
    #[must_use]
    pub const fn bit_width(self) -> u8 {
        match self {
            Self::Bool => 1,
            Self::I8 | Self::U8 => 8,
            Self::I16 | Self::U16 | Self::F16 => 16,
            Self::I32 | Self::U32 | Self::F32 => 32,
        }
    }
}

/// Value shapes surfaced by the current PCU IR core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuValueType {
    Scalar(PcuScalarType),
    Vector { scalar: PcuScalarType, lanes: u8 },
}

impl PcuValueType {
    /// Returns the scalar carrier type for this value.
    #[must_use]
    pub const fn scalar_type(self) -> PcuScalarType {
        match self {
            Self::Scalar(scalar) | Self::Vector { scalar, .. } => scalar,
        }
    }

    /// Returns the lane count for this value.
    #[must_use]
    pub const fn lanes(self) -> u8 {
        match self {
            Self::Scalar(_) => 1,
            Self::Vector { lanes, .. } => lanes,
        }
    }
}

/// Back-compat alias while the compute profile keeps using the older vocabulary.
pub type PcuComputeScalarType = PcuScalarType;

/// Back-compat alias while the compute profile keeps using the older vocabulary.
pub type PcuComputeValueType = PcuValueType;
