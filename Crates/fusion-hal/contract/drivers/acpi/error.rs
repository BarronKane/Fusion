//! Shared ACPI driver-family errors.

use core::fmt;

/// Kind of failure surfaced by ACPI-backed driver families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AcpiErrorKind {
    Unsupported,
    Invalid,
    Busy,
    ResourceExhausted,
    StateConflict,
    Platform(i32),
}

/// Shared ACPI driver-family error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiError {
    kind: AcpiErrorKind,
}

impl AcpiError {
    #[must_use]
    pub const fn new(kind: AcpiErrorKind) -> Self {
        Self { kind }
    }

    #[must_use]
    pub const fn kind(self) -> AcpiErrorKind {
        self.kind
    }

    #[must_use]
    pub const fn unsupported() -> Self {
        Self::new(AcpiErrorKind::Unsupported)
    }

    #[must_use]
    pub const fn invalid() -> Self {
        Self::new(AcpiErrorKind::Invalid)
    }

    #[must_use]
    pub const fn busy() -> Self {
        Self::new(AcpiErrorKind::Busy)
    }

    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self::new(AcpiErrorKind::ResourceExhausted)
    }

    #[must_use]
    pub const fn state_conflict() -> Self {
        Self::new(AcpiErrorKind::StateConflict)
    }

    #[must_use]
    pub const fn platform(code: i32) -> Self {
        Self::new(AcpiErrorKind::Platform(code))
    }
}

impl fmt::Display for AcpiErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Unsupported => f.write_str("acpi operation unsupported"),
            Self::Invalid => f.write_str("invalid acpi request"),
            Self::Busy => f.write_str("acpi resource busy"),
            Self::ResourceExhausted => f.write_str("acpi resources exhausted"),
            Self::StateConflict => f.write_str("acpi state conflict"),
            Self::Platform(code) => write!(f, "acpi platform error {code}"),
        }
    }
}

impl fmt::Display for AcpiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}
