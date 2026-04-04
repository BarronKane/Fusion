//! Error vocabulary for ACPI table parsing and validation.
//!
//! At this layer ACPI failures are mostly structural, not semantic. We are
//! validating firmware-authored binary tables against the requirements laid out
//! in the UEFI Forum ACPI Specification 6.6, especially the common
//! `DESCRIPTION_HEADER` rules in Section 5.2.6 and the table-specific layout
//! rules for `XSDT` and `MADT`.
//!
//! That means the useful failures are narrow:
//!
//! - the table is truncated,
//! - the signature does not match the requested table type,
//! - the checksum does not satisfy ACPI's "entire table sums to zero" rule,
//! - the payload length or record structure is internally inconsistent.
//!
//! Anything more expressive belongs above the raw parser layer, once Fusion is
//! interpreting topology and policy instead of merely proving the bytes are not
//! lying quite yet.

use core::fmt;

/// Kind of ACPI table failure surfaced by the dynamic HAL lane.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AcpiErrorKind {
    /// The supplied bytes are too short for the requested structure.
    Truncated,
    /// The table signature did not match the requested structure.
    InvalidSignature,
    /// The ACPI checksum failed validation.
    InvalidChecksum,
    /// The table's declared length or internal shape is malformed.
    InvalidLayout,
}

/// ACPI table parsing or validation failure.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AcpiError {
    kind: AcpiErrorKind,
}

impl AcpiError {
    /// Creates one truncated-table error.
    #[must_use]
    pub const fn truncated() -> Self {
        Self {
            kind: AcpiErrorKind::Truncated,
        }
    }

    /// Creates one invalid-signature error.
    #[must_use]
    pub const fn invalid_signature() -> Self {
        Self {
            kind: AcpiErrorKind::InvalidSignature,
        }
    }

    /// Creates one invalid-checksum error.
    #[must_use]
    pub const fn invalid_checksum() -> Self {
        Self {
            kind: AcpiErrorKind::InvalidChecksum,
        }
    }

    /// Creates one invalid-layout error.
    #[must_use]
    pub const fn invalid_layout() -> Self {
        Self {
            kind: AcpiErrorKind::InvalidLayout,
        }
    }

    /// Returns the concrete error kind.
    #[must_use]
    pub const fn kind(self) -> AcpiErrorKind {
        self.kind
    }
}

impl fmt::Display for AcpiErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Truncated => f.write_str("acpi table truncated"),
            Self::InvalidSignature => f.write_str("acpi table signature mismatch"),
            Self::InvalidChecksum => f.write_str("acpi table checksum mismatch"),
            Self::InvalidLayout => f.write_str("acpi table layout invalid"),
        }
    }
}

impl fmt::Display for AcpiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}
