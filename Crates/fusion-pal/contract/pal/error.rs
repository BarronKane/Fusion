//! Error types for fusion-pal hardware queries.

use core::fmt;

/// Kind of failure returned by a hardware-query provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HardwareErrorKind {
    /// The requested capability is unsupported.
    Unsupported,
    /// The request was structurally invalid.
    Invalid,
    /// The provider is temporarily busy.
    Busy,
    /// Resources needed for the query were exhausted.
    ResourceExhausted,
    /// The request conflicted with the current provider state.
    StateConflict,
    /// Opaque provider-specific failure code.
    Platform(i32),
}

/// Error returned by a fusion-pal hardware-query provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HardwareError {
    /// Concrete hardware error classification.
    kind: HardwareErrorKind,
}

impl HardwareError {
    /// Creates an unsupported-operation error.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: HardwareErrorKind::Unsupported,
        }
    }

    /// Creates an invalid-request error.
    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: HardwareErrorKind::Invalid,
        }
    }

    /// Creates a busy-provider error.
    #[must_use]
    pub const fn busy() -> Self {
        Self {
            kind: HardwareErrorKind::Busy,
        }
    }

    /// Creates a resource-exhaustion error.
    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self {
            kind: HardwareErrorKind::ResourceExhausted,
        }
    }

    /// Creates a state-conflict error.
    #[must_use]
    pub const fn state_conflict() -> Self {
        Self {
            kind: HardwareErrorKind::StateConflict,
        }
    }

    /// Creates a platform-specific error.
    #[must_use]
    pub const fn platform(code: i32) -> Self {
        Self {
            kind: HardwareErrorKind::Platform(code),
        }
    }

    /// Returns the concrete error kind.
    #[must_use]
    pub const fn kind(self) -> HardwareErrorKind {
        self.kind
    }
}

impl fmt::Display for HardwareErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Unsupported => f.write_str("hardware query unsupported"),
            Self::Invalid => f.write_str("invalid hardware query"),
            Self::Busy => f.write_str("hardware provider busy"),
            Self::ResourceExhausted => f.write_str("hardware query resources exhausted"),
            Self::StateConflict => f.write_str("hardware query state conflict"),
            Self::Platform(code) => write!(f, "platform hardware error {code}"),
        }
    }
}

impl fmt::Display for HardwareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}
