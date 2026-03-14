//! Error types for PAL event polling.

use core::fmt;

/// Kind of failure returned by an event backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventErrorKind {
    /// The requested capability is unsupported.
    Unsupported,
    /// The source handle, key, or request was invalid.
    Invalid,
    /// The backend is temporarily busy.
    Busy,
    /// The operation timed out.
    Timeout,
    /// Resources such as file descriptors or completion slots were exhausted.
    ResourceExhausted,
    /// The request conflicted with current backend state.
    StateConflict,
    /// Opaque backend-specific failure code.
    Platform(i32),
}

/// Error returned by a PAL event backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventError {
    /// Concrete event error classification.
    kind: EventErrorKind,
}

impl EventError {
    /// Creates an unsupported-operation error.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: EventErrorKind::Unsupported,
        }
    }

    /// Creates an invalid-argument error.
    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: EventErrorKind::Invalid,
        }
    }

    /// Creates a busy-backend error.
    #[must_use]
    pub const fn busy() -> Self {
        Self {
            kind: EventErrorKind::Busy,
        }
    }

    /// Creates a timeout error.
    #[must_use]
    pub const fn timeout() -> Self {
        Self {
            kind: EventErrorKind::Timeout,
        }
    }

    /// Creates a resource-exhaustion error.
    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self {
            kind: EventErrorKind::ResourceExhausted,
        }
    }

    /// Creates a state-conflict error.
    #[must_use]
    pub const fn state_conflict() -> Self {
        Self {
            kind: EventErrorKind::StateConflict,
        }
    }

    /// Creates a platform-specific error.
    #[must_use]
    pub const fn platform(code: i32) -> Self {
        Self {
            kind: EventErrorKind::Platform(code),
        }
    }

    /// Returns the concrete event error kind.
    #[must_use]
    pub const fn kind(self) -> EventErrorKind {
        self.kind
    }
}

impl fmt::Display for EventErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Unsupported => f.write_str("event polling unsupported"),
            Self::Invalid => f.write_str("invalid event request"),
            Self::Busy => f.write_str("event backend busy"),
            Self::Timeout => f.write_str("event poll timed out"),
            Self::ResourceExhausted => f.write_str("event resources exhausted"),
            Self::StateConflict => f.write_str("event state conflict"),
            Self::Platform(code) => write!(f, "platform event error {code}"),
        }
    }
}

impl fmt::Display for EventError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}
