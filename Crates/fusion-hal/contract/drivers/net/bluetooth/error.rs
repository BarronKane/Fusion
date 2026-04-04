//! Error types for generic Bluetooth backends.

use core::fmt;

/// Kind of failure returned by a generic Bluetooth backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BluetoothErrorKind {
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
    /// The requested peer, connection, or attribute is no longer present.
    Disconnected,
    /// The operation timed out.
    TimedOut,
    /// The operation was denied by security or policy state.
    PermissionDenied,
    /// Backend-specific failure code.
    Platform(i32),
}

/// Error returned by a generic Bluetooth backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothError {
    kind: BluetoothErrorKind,
}

impl BluetoothError {
    /// Creates an unsupported-operation error.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: BluetoothErrorKind::Unsupported,
        }
    }

    /// Creates an invalid-request error.
    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: BluetoothErrorKind::Invalid,
        }
    }

    /// Creates a busy-backend error.
    #[must_use]
    pub const fn busy() -> Self {
        Self {
            kind: BluetoothErrorKind::Busy,
        }
    }

    /// Creates a resource-exhausted error.
    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self {
            kind: BluetoothErrorKind::ResourceExhausted,
        }
    }

    /// Creates a state-conflict error.
    #[must_use]
    pub const fn state_conflict() -> Self {
        Self {
            kind: BluetoothErrorKind::StateConflict,
        }
    }

    /// Creates a disconnected error.
    #[must_use]
    pub const fn disconnected() -> Self {
        Self {
            kind: BluetoothErrorKind::Disconnected,
        }
    }

    /// Creates a timeout error.
    #[must_use]
    pub const fn timed_out() -> Self {
        Self {
            kind: BluetoothErrorKind::TimedOut,
        }
    }

    /// Creates a permission-denied error.
    #[must_use]
    pub const fn permission_denied() -> Self {
        Self {
            kind: BluetoothErrorKind::PermissionDenied,
        }
    }

    /// Creates a platform-specific error.
    #[must_use]
    pub const fn platform(code: i32) -> Self {
        Self {
            kind: BluetoothErrorKind::Platform(code),
        }
    }

    /// Returns the concrete Bluetooth error kind.
    #[must_use]
    pub const fn kind(self) -> BluetoothErrorKind {
        self.kind
    }
}

impl fmt::Display for BluetoothErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Unsupported => f.write_str("bluetooth operation unsupported"),
            Self::Invalid => f.write_str("invalid bluetooth request"),
            Self::Busy => f.write_str("bluetooth resource busy"),
            Self::ResourceExhausted => f.write_str("bluetooth resources exhausted"),
            Self::StateConflict => f.write_str("bluetooth state conflict"),
            Self::Disconnected => f.write_str("bluetooth peer disconnected"),
            Self::TimedOut => f.write_str("bluetooth operation timed out"),
            Self::PermissionDenied => f.write_str("bluetooth permission denied"),
            Self::Platform(code) => write!(f, "platform bluetooth error {code}"),
        }
    }
}

impl fmt::Display for BluetoothError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}
