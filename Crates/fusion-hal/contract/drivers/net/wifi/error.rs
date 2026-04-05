//! Error types for generic Wi-Fi backends.

use core::fmt;

/// Kind of failure returned by a generic Wi-Fi backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WifiErrorKind {
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
    /// The requested link or peer is no longer present.
    Disconnected,
    /// The operation timed out.
    TimedOut,
    /// The operation was denied by security or policy state.
    PermissionDenied,
    /// Backend-specific failure code.
    Platform(i32),
}

/// Error returned by a generic Wi-Fi backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiError {
    kind: WifiErrorKind,
}

impl WifiError {
    /// Creates an unsupported-operation error.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: WifiErrorKind::Unsupported,
        }
    }

    /// Creates an invalid-request error.
    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: WifiErrorKind::Invalid,
        }
    }

    /// Creates a busy-backend error.
    #[must_use]
    pub const fn busy() -> Self {
        Self {
            kind: WifiErrorKind::Busy,
        }
    }

    /// Creates a resource-exhausted error.
    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self {
            kind: WifiErrorKind::ResourceExhausted,
        }
    }

    /// Creates a state-conflict error.
    #[must_use]
    pub const fn state_conflict() -> Self {
        Self {
            kind: WifiErrorKind::StateConflict,
        }
    }

    /// Creates a disconnected error.
    #[must_use]
    pub const fn disconnected() -> Self {
        Self {
            kind: WifiErrorKind::Disconnected,
        }
    }

    /// Creates a timeout error.
    #[must_use]
    pub const fn timed_out() -> Self {
        Self {
            kind: WifiErrorKind::TimedOut,
        }
    }

    /// Creates a permission-denied error.
    #[must_use]
    pub const fn permission_denied() -> Self {
        Self {
            kind: WifiErrorKind::PermissionDenied,
        }
    }

    /// Creates a platform-specific error.
    #[must_use]
    pub const fn platform(code: i32) -> Self {
        Self {
            kind: WifiErrorKind::Platform(code),
        }
    }

    /// Returns the concrete Wi-Fi error kind.
    #[must_use]
    pub const fn kind(self) -> WifiErrorKind {
        self.kind
    }
}

impl fmt::Display for WifiErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Unsupported => f.write_str("wifi operation unsupported"),
            Self::Invalid => f.write_str("invalid wifi request"),
            Self::Busy => f.write_str("wifi resource busy"),
            Self::ResourceExhausted => f.write_str("wifi resources exhausted"),
            Self::StateConflict => f.write_str("wifi state conflict"),
            Self::Disconnected => f.write_str("wifi link disconnected"),
            Self::TimedOut => f.write_str("wifi operation timed out"),
            Self::PermissionDenied => f.write_str("wifi permission denied"),
            Self::Platform(code) => write!(f, "platform wifi error {code}"),
        }
    }
}

impl fmt::Display for WifiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}
