//! Error surface for native domain/courier/context contracts.

use core::fmt;

/// Error returned by one native domain/courier/context implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DomainError {
    kind: DomainErrorKind,
}

impl DomainError {
    #[must_use]
    pub const fn kind(self) -> DomainErrorKind {
        self.kind
    }

    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: DomainErrorKind::Unsupported,
        }
    }

    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: DomainErrorKind::Invalid,
        }
    }

    #[must_use]
    pub const fn busy() -> Self {
        Self {
            kind: DomainErrorKind::Busy,
        }
    }

    #[must_use]
    pub const fn permission_denied() -> Self {
        Self {
            kind: DomainErrorKind::PermissionDenied,
        }
    }

    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self {
            kind: DomainErrorKind::ResourceExhausted,
        }
    }

    #[must_use]
    pub const fn state_conflict() -> Self {
        Self {
            kind: DomainErrorKind::StateConflict,
        }
    }

    #[must_use]
    pub const fn not_found() -> Self {
        Self {
            kind: DomainErrorKind::NotFound,
        }
    }

    #[must_use]
    pub const fn not_visible() -> Self {
        Self {
            kind: DomainErrorKind::NotVisible,
        }
    }

    #[must_use]
    pub const fn platform(code: i32) -> Self {
        Self {
            kind: DomainErrorKind::Platform(code),
        }
    }
}

/// Classification of native domain/courier/context failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DomainErrorKind {
    Unsupported,
    Invalid,
    Busy,
    PermissionDenied,
    ResourceExhausted,
    StateConflict,
    NotFound,
    NotVisible,
    Platform(i32),
}

impl fmt::Display for DomainErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("unsupported domain operation"),
            Self::Invalid => f.write_str("invalid domain request"),
            Self::Busy => f.write_str("domain object is busy"),
            Self::PermissionDenied => f.write_str("domain permission denied"),
            Self::ResourceExhausted => f.write_str("domain resources exhausted"),
            Self::StateConflict => f.write_str("domain state conflict"),
            Self::NotFound => f.write_str("domain object not found"),
            Self::NotVisible => f.write_str("context is not visible to this courier"),
            Self::Platform(code) => write!(f, "platform domain error ({code})"),
        }
    }
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}
