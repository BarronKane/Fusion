//! Error types for interrupt-vector ownership backends.

use core::fmt;

/// Kind of failure returned by one vector-ownership backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VectorErrorKind {
    /// The requested capability is unsupported.
    Unsupported,
    /// The request was structurally invalid.
    Invalid,
    /// One slot or exception is already bound.
    AlreadyBound,
    /// One slot or exception is not currently bound.
    NotBound,
    /// The requested slot or exception is reserved by the system.
    Reserved,
    /// The request conflicts with current backend state.
    StateConflict,
    /// One builder or table failed its seal-time validation.
    SealViolation,
    /// The builder or owned table is already sealed.
    Sealed,
    /// The requested core or topology scope does not match the active mode.
    CoreMismatch,
    /// The requested security world does not match the active mode.
    WorldMismatch,
    /// The system could not provide the necessary runtime resources.
    ResourceExhausted,
    /// Backend-specific failure code.
    Platform(i32),
}

/// Error returned by one vector-ownership backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VectorError {
    kind: VectorErrorKind,
}

impl VectorError {
    /// Creates an unsupported-operation error.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: VectorErrorKind::Unsupported,
        }
    }

    /// Creates an invalid-request error.
    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: VectorErrorKind::Invalid,
        }
    }

    /// Creates an already-bound error.
    #[must_use]
    pub const fn already_bound() -> Self {
        Self {
            kind: VectorErrorKind::AlreadyBound,
        }
    }

    /// Creates a not-bound error.
    #[must_use]
    pub const fn not_bound() -> Self {
        Self {
            kind: VectorErrorKind::NotBound,
        }
    }

    /// Creates a reserved-slot error.
    #[must_use]
    pub const fn reserved() -> Self {
        Self {
            kind: VectorErrorKind::Reserved,
        }
    }

    /// Creates a state-conflict error.
    #[must_use]
    pub const fn state_conflict() -> Self {
        Self {
            kind: VectorErrorKind::StateConflict,
        }
    }

    /// Creates a seal-violation error.
    #[must_use]
    pub const fn seal_violation() -> Self {
        Self {
            kind: VectorErrorKind::SealViolation,
        }
    }

    /// Creates a sealed error.
    #[must_use]
    pub const fn sealed() -> Self {
        Self {
            kind: VectorErrorKind::Sealed,
        }
    }

    /// Creates a core-mismatch error.
    #[must_use]
    pub const fn core_mismatch() -> Self {
        Self {
            kind: VectorErrorKind::CoreMismatch,
        }
    }

    /// Creates a world-mismatch error.
    #[must_use]
    pub const fn world_mismatch() -> Self {
        Self {
            kind: VectorErrorKind::WorldMismatch,
        }
    }

    /// Creates a resource-exhausted error.
    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self {
            kind: VectorErrorKind::ResourceExhausted,
        }
    }

    /// Creates a platform-specific error.
    #[must_use]
    pub const fn platform(code: i32) -> Self {
        Self {
            kind: VectorErrorKind::Platform(code),
        }
    }

    /// Returns the concrete vector-ownership error kind.
    #[must_use]
    pub const fn kind(self) -> VectorErrorKind {
        self.kind
    }
}

impl fmt::Display for VectorErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Unsupported => f.write_str("vector operation unsupported"),
            Self::Invalid => f.write_str("invalid vector request"),
            Self::AlreadyBound => f.write_str("vector slot already bound"),
            Self::NotBound => f.write_str("vector slot not bound"),
            Self::Reserved => f.write_str("vector slot reserved"),
            Self::StateConflict => f.write_str("vector state conflict"),
            Self::SealViolation => f.write_str("vector seal validation failed"),
            Self::Sealed => f.write_str("vector table already sealed"),
            Self::CoreMismatch => f.write_str("vector core mismatch"),
            Self::WorldMismatch => f.write_str("vector security-domain mismatch"),
            Self::ResourceExhausted => f.write_str("vector resources exhausted"),
            Self::Platform(code) => write!(f, "platform vector error {code}"),
        }
    }
}

impl fmt::Display for VectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}
