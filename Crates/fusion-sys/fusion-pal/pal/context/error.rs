//! Error types for fusion-pal context switching.

use core::fmt;

/// Kind of failure returned by a context-switch backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContextErrorKind {
    /// The requested capability is unsupported.
    Unsupported,
    /// The provided stack, entry, or saved context was invalid.
    Invalid,
    /// The backend could not make progress because the resource is busy.
    Busy,
    /// The operation was denied by backend policy or privilege rules.
    PermissionDenied,
    /// Resources such as stack backing or metadata were exhausted.
    ResourceExhausted,
    /// The request conflicted with the current context state.
    StateConflict,
    /// Opaque backend-specific failure code.
    Platform(i32),
}

/// Error returned by a fusion-pal context-switch backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContextError {
    /// Concrete context error classification.
    kind: ContextErrorKind,
}

impl ContextError {
    /// Creates an unsupported-operation error.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: ContextErrorKind::Unsupported,
        }
    }

    /// Creates an invalid-argument error.
    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: ContextErrorKind::Invalid,
        }
    }

    /// Creates a busy-resource error.
    #[must_use]
    pub const fn busy() -> Self {
        Self {
            kind: ContextErrorKind::Busy,
        }
    }

    /// Creates a permission-denied error.
    #[must_use]
    pub const fn permission_denied() -> Self {
        Self {
            kind: ContextErrorKind::PermissionDenied,
        }
    }

    /// Creates a resource-exhaustion error.
    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self {
            kind: ContextErrorKind::ResourceExhausted,
        }
    }

    /// Creates a state-conflict error.
    #[must_use]
    pub const fn state_conflict() -> Self {
        Self {
            kind: ContextErrorKind::StateConflict,
        }
    }

    /// Creates a platform-specific error.
    #[must_use]
    pub const fn platform(code: i32) -> Self {
        Self {
            kind: ContextErrorKind::Platform(code),
        }
    }

    /// Returns the concrete error kind.
    #[must_use]
    pub const fn kind(self) -> ContextErrorKind {
        self.kind
    }
}

impl fmt::Display for ContextErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Unsupported => f.write_str("context switching unsupported"),
            Self::Invalid => f.write_str("invalid context request"),
            Self::Busy => f.write_str("context backend busy"),
            Self::PermissionDenied => f.write_str("context operation denied"),
            Self::ResourceExhausted => f.write_str("context resources exhausted"),
            Self::StateConflict => f.write_str("context state conflict"),
            Self::Platform(code) => write!(f, "platform context error {code}"),
        }
    }
}

impl fmt::Display for ContextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}
