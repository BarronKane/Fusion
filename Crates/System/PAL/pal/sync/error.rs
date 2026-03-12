use core::fmt;

/// Error returned by PAL synchronization primitives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SyncError {
    /// Specific synchronization error kind.
    pub kind: SyncErrorKind,
}

impl SyncError {
    /// Constructs an unsupported-operation error.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: SyncErrorKind::Unsupported,
        }
    }

    /// Constructs an invalid-request error.
    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: SyncErrorKind::Invalid,
        }
    }

    /// Constructs a busy/resource-not-available error.
    #[must_use]
    pub const fn busy() -> Self {
        Self {
            kind: SyncErrorKind::Busy,
        }
    }

    /// Constructs an overflow error.
    #[must_use]
    pub const fn overflow() -> Self {
        Self {
            kind: SyncErrorKind::Overflow,
        }
    }

    /// Constructs a permission-denied error.
    #[must_use]
    pub const fn permission_denied() -> Self {
        Self {
            kind: SyncErrorKind::PermissionDenied,
        }
    }

    /// Constructs a platform-specific error.
    #[must_use]
    pub const fn platform(code: i32) -> Self {
        Self {
            kind: SyncErrorKind::Platform(code),
        }
    }
}

/// Classification of synchronization errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyncErrorKind {
    /// The platform or backend does not support the requested operation.
    Unsupported,
    /// The request was structurally invalid.
    Invalid,
    /// The primitive was busy or unavailable for immediate progress.
    Busy,
    /// The caller lacked permission for the requested operation.
    PermissionDenied,
    /// Arithmetic or count tracking overflow occurred.
    Overflow,
    /// Backend-specific operating-system error code.
    Platform(i32),
}

impl fmt::Display for SyncErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("unsupported synchronization operation"),
            Self::Invalid => f.write_str("invalid synchronization request"),
            Self::Busy => f.write_str("synchronization primitive is busy"),
            Self::PermissionDenied => {
                f.write_str("permission denied for synchronization operation")
            }
            Self::Overflow => f.write_str("synchronization count overflow"),
            Self::Platform(code) => write!(f, "platform synchronization error ({code})"),
        }
    }
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}
