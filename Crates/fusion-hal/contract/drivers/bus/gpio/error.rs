//! Error types for generic GPIO backends.

use core::fmt;

/// Kind of failure returned by a generic GPIO backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpioErrorKind {
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

/// Error returned by a generic GPIO backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpioError {
    kind: GpioErrorKind,
}

impl GpioError {
    /// Creates an unsupported-operation error.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: GpioErrorKind::Unsupported,
        }
    }

    /// Creates an invalid-request error.
    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: GpioErrorKind::Invalid,
        }
    }

    /// Creates a busy-backend error.
    #[must_use]
    pub const fn busy() -> Self {
        Self {
            kind: GpioErrorKind::Busy,
        }
    }

    /// Creates a resource-exhausted error.
    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self {
            kind: GpioErrorKind::ResourceExhausted,
        }
    }

    /// Creates a state-conflict error.
    #[must_use]
    pub const fn state_conflict() -> Self {
        Self {
            kind: GpioErrorKind::StateConflict,
        }
    }

    /// Creates a platform-specific error.
    #[must_use]
    pub const fn platform(code: i32) -> Self {
        Self {
            kind: GpioErrorKind::Platform(code),
        }
    }

    /// Returns the concrete GPIO error kind.
    #[must_use]
    pub const fn kind(self) -> GpioErrorKind {
        self.kind
    }
}

impl fmt::Display for GpioErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Unsupported => f.write_str("gpio operation unsupported"),
            Self::Invalid => f.write_str("invalid gpio request"),
            Self::Busy => f.write_str("gpio resource busy"),
            Self::ResourceExhausted => f.write_str("gpio resources exhausted"),
            Self::StateConflict => f.write_str("gpio state conflict"),
            Self::Platform(code) => write!(f, "platform gpio error {code}"),
        }
    }
}

impl fmt::Display for GpioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}
