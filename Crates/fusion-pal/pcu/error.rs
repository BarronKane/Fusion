//! Error types for programmable-IO backends.

use core::fmt;

/// Kind of failure returned by a programmable-IO backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuErrorKind {
    /// The requested capability is unsupported.
    Unsupported,
    /// The request was structurally invalid.
    Invalid,
    /// The backend or resource is currently busy.
    Busy,
    /// The system could not provide the required runtime resources.
    ResourceExhausted,
    /// The request conflicted with current backend state.
    StateConflict,
    /// Backend-specific failure code.
    Platform(i32),
}

/// Error returned by a programmable-IO backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuError {
    kind: PcuErrorKind,
}

impl PcuError {
    /// Creates an unsupported-operation error.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: PcuErrorKind::Unsupported,
        }
    }

    /// Creates an invalid-request error.
    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: PcuErrorKind::Invalid,
        }
    }

    /// Creates a busy-backend error.
    #[must_use]
    pub const fn busy() -> Self {
        Self {
            kind: PcuErrorKind::Busy,
        }
    }

    /// Creates a resource-exhausted error.
    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self {
            kind: PcuErrorKind::ResourceExhausted,
        }
    }

    /// Creates a state-conflict error.
    #[must_use]
    pub const fn state_conflict() -> Self {
        Self {
            kind: PcuErrorKind::StateConflict,
        }
    }

    /// Creates a platform-specific error.
    #[must_use]
    pub const fn platform(code: i32) -> Self {
        Self {
            kind: PcuErrorKind::Platform(code),
        }
    }

    /// Returns the concrete programmable-IO error kind.
    #[must_use]
    pub const fn kind(self) -> PcuErrorKind {
        self.kind
    }
}

impl fmt::Display for PcuErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Unsupported => f.write_str("programmable-io operation unsupported"),
            Self::Invalid => f.write_str("invalid programmable-io request"),
            Self::Busy => f.write_str("programmable-io resource busy"),
            Self::ResourceExhausted => f.write_str("programmable-io resources exhausted"),
            Self::StateConflict => f.write_str("programmable-io state conflict"),
            Self::Platform(code) => write!(f, "platform programmable-io error {code}"),
        }
    }
}

impl fmt::Display for PcuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}
