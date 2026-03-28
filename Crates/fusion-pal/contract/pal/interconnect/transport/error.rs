//! Error surface for transport-layer contracts.

use core::fmt;

/// Error returned by one transport implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TransportError {
    kind: TransportErrorKind,
}

impl TransportError {
    #[must_use]
    pub const fn kind(self) -> TransportErrorKind {
        self.kind
    }

    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: TransportErrorKind::Unsupported,
        }
    }

    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: TransportErrorKind::Invalid,
        }
    }

    #[must_use]
    pub const fn busy() -> Self {
        Self {
            kind: TransportErrorKind::Busy,
        }
    }

    #[must_use]
    pub const fn permission_denied() -> Self {
        Self {
            kind: TransportErrorKind::PermissionDenied,
        }
    }

    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self {
            kind: TransportErrorKind::ResourceExhausted,
        }
    }

    #[must_use]
    pub const fn state_conflict() -> Self {
        Self {
            kind: TransportErrorKind::StateConflict,
        }
    }

    #[must_use]
    pub const fn not_attached() -> Self {
        Self {
            kind: TransportErrorKind::NotAttached,
        }
    }

    #[must_use]
    pub const fn platform(code: i32) -> Self {
        Self {
            kind: TransportErrorKind::Platform(code),
        }
    }
}

/// Classification of transport-layer failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportErrorKind {
    Unsupported,
    Invalid,
    Busy,
    PermissionDenied,
    ResourceExhausted,
    StateConflict,
    NotAttached,
    Platform(i32),
}

impl fmt::Display for TransportErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("unsupported transport operation"),
            Self::Invalid => f.write_str("invalid transport request"),
            Self::Busy => f.write_str("transport is busy"),
            Self::PermissionDenied => f.write_str("transport permission denied"),
            Self::ResourceExhausted => f.write_str("transport resources exhausted"),
            Self::StateConflict => f.write_str("transport state conflict"),
            Self::NotAttached => f.write_str("transport attachment not found"),
            Self::Platform(code) => write!(f, "platform transport error ({code})"),
        }
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}
