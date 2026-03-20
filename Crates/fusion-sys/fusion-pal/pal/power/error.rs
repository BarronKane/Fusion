//! Error types for fusion-pal power control.

use core::fmt;

/// Kind of failure returned by a power backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PowerErrorKind {
    /// The requested capability is unsupported.
    Unsupported,
    /// The requested mode or argument was invalid.
    Invalid,
    /// The backend is temporarily busy and cannot enter the mode now.
    Busy,
    /// The request conflicted with current backend state.
    StateConflict,
    /// Opaque backend-specific failure code.
    Platform(i32),
}

/// Error returned by a fusion-pal power backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PowerError {
    kind: PowerErrorKind,
}

impl PowerError {
    /// Creates an unsupported-operation error.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: PowerErrorKind::Unsupported,
        }
    }

    /// Creates an invalid-argument error.
    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: PowerErrorKind::Invalid,
        }
    }

    /// Creates a busy-backend error.
    #[must_use]
    pub const fn busy() -> Self {
        Self {
            kind: PowerErrorKind::Busy,
        }
    }

    /// Creates a state-conflict error.
    #[must_use]
    pub const fn state_conflict() -> Self {
        Self {
            kind: PowerErrorKind::StateConflict,
        }
    }

    /// Creates a platform-specific error.
    #[must_use]
    pub const fn platform(code: i32) -> Self {
        Self {
            kind: PowerErrorKind::Platform(code),
        }
    }

    /// Returns the concrete power error kind.
    #[must_use]
    pub const fn kind(self) -> PowerErrorKind {
        self.kind
    }
}

impl fmt::Display for PowerErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Unsupported => f.write_str("power control unsupported"),
            Self::Invalid => f.write_str("invalid power request"),
            Self::Busy => f.write_str("power backend busy"),
            Self::StateConflict => f.write_str("power state conflict"),
            Self::Platform(code) => write!(f, "platform power error {code}"),
        }
    }
}

impl fmt::Display for PowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}
