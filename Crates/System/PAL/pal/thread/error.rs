use core::fmt;

/// Error returned by PAL thread operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadError {
    /// Specific thread error kind.
    kind: ThreadErrorKind,
}

impl ThreadError {
    /// Returns the concrete thread error kind.
    #[must_use]
    pub const fn kind(self) -> ThreadErrorKind {
        self.kind
    }

    /// Constructs an unsupported-operation error.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: ThreadErrorKind::Unsupported,
        }
    }

    /// Constructs an invalid-request error.
    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: ThreadErrorKind::Invalid,
        }
    }

    /// Constructs a busy or currently-unavailable error.
    #[must_use]
    pub const fn busy() -> Self {
        Self {
            kind: ThreadErrorKind::Busy,
        }
    }

    /// Constructs a permission-denied error.
    #[must_use]
    pub const fn permission_denied() -> Self {
        Self {
            kind: ThreadErrorKind::PermissionDenied,
        }
    }

    /// Constructs a resource-exhausted error.
    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self {
            kind: ThreadErrorKind::ResourceExhausted,
        }
    }

    /// Constructs a timeout error.
    #[must_use]
    pub const fn timeout() -> Self {
        Self {
            kind: ThreadErrorKind::Timeout,
        }
    }

    /// Constructs a thread-state conflict error.
    #[must_use]
    pub const fn state_conflict() -> Self {
        Self {
            kind: ThreadErrorKind::StateConflict,
        }
    }

    /// Constructs a placement-rejected error.
    #[must_use]
    pub const fn placement_denied() -> Self {
        Self {
            kind: ThreadErrorKind::PlacementDenied,
        }
    }

    /// Constructs a scheduler-rejected error.
    #[must_use]
    pub const fn scheduler_denied() -> Self {
        Self {
            kind: ThreadErrorKind::SchedulerDenied,
        }
    }

    /// Constructs a stack-configuration-rejected error.
    #[must_use]
    pub const fn stack_denied() -> Self {
        Self {
            kind: ThreadErrorKind::StackDenied,
        }
    }

    /// Constructs a platform-specific error.
    #[must_use]
    pub const fn platform(code: i32) -> Self {
        Self {
            kind: ThreadErrorKind::Platform(code),
        }
    }
}

/// Classification of PAL thread errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadErrorKind {
    /// The platform or backend does not support the requested operation.
    Unsupported,
    /// The request was structurally invalid.
    Invalid,
    /// The backend cannot make immediate progress.
    Busy,
    /// The caller lacked permission for the requested operation.
    PermissionDenied,
    /// The system could not provide the necessary runtime resources.
    ResourceExhausted,
    /// The requested operation timed out.
    Timeout,
    /// The thread or handle is in the wrong state for the requested operation.
    StateConflict,
    /// The requested placement policy could not be honored honestly.
    PlacementDenied,
    /// The requested scheduler policy could not be honored honestly.
    SchedulerDenied,
    /// The requested stack or startup-memory policy could not be honored honestly.
    StackDenied,
    /// Backend-specific operating-system or runtime error code.
    Platform(i32),
}

impl fmt::Display for ThreadErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("unsupported thread operation"),
            Self::Invalid => f.write_str("invalid thread request"),
            Self::Busy => f.write_str("thread operation is busy"),
            Self::PermissionDenied => f.write_str("permission denied for thread operation"),
            Self::ResourceExhausted => f.write_str("thread resources exhausted"),
            Self::Timeout => f.write_str("thread operation timed out"),
            Self::StateConflict => f.write_str("thread handle is in the wrong state"),
            Self::PlacementDenied => f.write_str("thread placement request was denied"),
            Self::SchedulerDenied => f.write_str("thread scheduler request was denied"),
            Self::StackDenied => f.write_str("thread stack request was denied"),
            Self::Platform(code) => write!(f, "platform thread error ({code})"),
        }
    }
}

impl fmt::Display for ThreadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}
