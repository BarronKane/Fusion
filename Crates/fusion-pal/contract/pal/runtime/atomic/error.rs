use core::fmt;

/// Error returned by fusion-pal atomic primitives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AtomicError {
    /// Specific atomic error kind.
    pub kind: AtomicErrorKind,
}

impl AtomicError {
    /// Constructs an unsupported-operation error.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: AtomicErrorKind::Unsupported,
        }
    }

    /// Constructs an invalid-request error.
    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: AtomicErrorKind::Invalid,
        }
    }

    /// Constructs a busy/resource-not-available error.
    #[must_use]
    pub const fn busy() -> Self {
        Self {
            kind: AtomicErrorKind::Busy,
        }
    }

    /// Constructs an overflow error.
    #[must_use]
    pub const fn overflow() -> Self {
        Self {
            kind: AtomicErrorKind::Overflow,
        }
    }

    /// Constructs a permission-denied error.
    #[must_use]
    pub const fn permission_denied() -> Self {
        Self {
            kind: AtomicErrorKind::PermissionDenied,
        }
    }

    /// Constructs a platform-specific error.
    #[must_use]
    pub const fn platform(code: i32) -> Self {
        Self {
            kind: AtomicErrorKind::Platform(code),
        }
    }
}

/// Classification of atomic-operation errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AtomicErrorKind {
    /// The platform or backend does not support the requested operation.
    Unsupported,
    /// The request was structurally invalid.
    Invalid,
    /// The primitive was busy or unavailable for immediate progress.
    Busy,
    /// The caller lacked permission for the requested operation.
    PermissionDenied,
    /// Arithmetic overflow occurred.
    Overflow,
    /// Backend-specific operating-system error code.
    Platform(i32),
}

impl fmt::Display for AtomicErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("unsupported atomic operation"),
            Self::Invalid => f.write_str("invalid atomic request"),
            Self::Busy => f.write_str("atomic primitive is busy"),
            Self::PermissionDenied => f.write_str("permission denied for atomic operation"),
            Self::Overflow => f.write_str("atomic arithmetic overflow"),
            Self::Platform(code) => write!(f, "platform atomic error ({code})"),
        }
    }
}

impl fmt::Display for AtomicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}
