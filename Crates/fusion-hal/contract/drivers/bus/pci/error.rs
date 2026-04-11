//! Canonical PCI contract error and error-reporting vocabulary.

use core::fmt;

/// Kind of failure returned by PCI-family contract surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PciErrorKind {
    Unsupported,
    Invalid,
    Busy,
    NotPresent,
    Timeout,
    Fault,
    StateConflict,
    ResourceExhausted,
    Platform(i32),
}

/// Error returned by PCI-family contract surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciError {
    kind: PciErrorKind,
}

impl PciError {
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: PciErrorKind::Unsupported,
        }
    }

    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: PciErrorKind::Invalid,
        }
    }

    #[must_use]
    pub const fn busy() -> Self {
        Self {
            kind: PciErrorKind::Busy,
        }
    }

    #[must_use]
    pub const fn not_present() -> Self {
        Self {
            kind: PciErrorKind::NotPresent,
        }
    }

    #[must_use]
    pub const fn timeout() -> Self {
        Self {
            kind: PciErrorKind::Timeout,
        }
    }

    #[must_use]
    pub const fn fault() -> Self {
        Self {
            kind: PciErrorKind::Fault,
        }
    }

    #[must_use]
    pub const fn state_conflict() -> Self {
        Self {
            kind: PciErrorKind::StateConflict,
        }
    }

    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self {
            kind: PciErrorKind::ResourceExhausted,
        }
    }

    #[must_use]
    pub const fn platform(code: i32) -> Self {
        Self {
            kind: PciErrorKind::Platform(code),
        }
    }

    #[must_use]
    pub const fn kind(self) -> PciErrorKind {
        self.kind
    }
}

impl fmt::Display for PciErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Unsupported => f.write_str("pci operation unsupported"),
            Self::Invalid => f.write_str("invalid pci request"),
            Self::Busy => f.write_str("pci resource busy"),
            Self::NotPresent => f.write_str("pci function not present"),
            Self::Timeout => f.write_str("pci operation timed out"),
            Self::Fault => f.write_str("pci fault"),
            Self::StateConflict => f.write_str("pci state conflict"),
            Self::ResourceExhausted => f.write_str("pci resources exhausted"),
            Self::Platform(code) => write!(f, "platform pci error {code}"),
        }
    }
}

impl fmt::Display for PciError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

/// PCI error-reporting and containment capability truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PciErrorReportingProfile {
    pub advanced_error_reporting: bool,
    pub downstream_port_containment: bool,
    pub ecrc_checking_capable: bool,
    pub ecrc_generation_capable: bool,
}

/// Error-reporting lane for one PCI function.
pub trait PciErrorReportingContract {
    /// Returns one truthful error-reporting capability snapshot.
    fn error_reporting_profile(&self) -> PciErrorReportingProfile;
}
