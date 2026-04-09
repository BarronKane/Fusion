//! Canonical USB contract error vocabulary.

use core::fmt;

/// Kind of failure returned by USB-family contract surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsbErrorKind {
    Unsupported,
    Invalid,
    Busy,
    Disconnected,
    Timeout,
    Stall,
    Protocol,
    Overcurrent,
    StateConflict,
    ResourceExhausted,
    Platform(i32),
}

/// Error returned by USB-family contract surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbError {
    kind: UsbErrorKind,
}

impl UsbError {
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: UsbErrorKind::Unsupported,
        }
    }

    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: UsbErrorKind::Invalid,
        }
    }

    #[must_use]
    pub const fn busy() -> Self {
        Self {
            kind: UsbErrorKind::Busy,
        }
    }

    #[must_use]
    pub const fn disconnected() -> Self {
        Self {
            kind: UsbErrorKind::Disconnected,
        }
    }

    #[must_use]
    pub const fn timeout() -> Self {
        Self {
            kind: UsbErrorKind::Timeout,
        }
    }

    #[must_use]
    pub const fn stall() -> Self {
        Self {
            kind: UsbErrorKind::Stall,
        }
    }

    #[must_use]
    pub const fn protocol() -> Self {
        Self {
            kind: UsbErrorKind::Protocol,
        }
    }

    #[must_use]
    pub const fn overcurrent() -> Self {
        Self {
            kind: UsbErrorKind::Overcurrent,
        }
    }

    #[must_use]
    pub const fn state_conflict() -> Self {
        Self {
            kind: UsbErrorKind::StateConflict,
        }
    }

    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self {
            kind: UsbErrorKind::ResourceExhausted,
        }
    }

    #[must_use]
    pub const fn platform(code: i32) -> Self {
        Self {
            kind: UsbErrorKind::Platform(code),
        }
    }

    #[must_use]
    pub const fn kind(self) -> UsbErrorKind {
        self.kind
    }
}

impl fmt::Display for UsbErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Unsupported => f.write_str("usb operation unsupported"),
            Self::Invalid => f.write_str("invalid usb request"),
            Self::Busy => f.write_str("usb resource busy"),
            Self::Disconnected => f.write_str("usb link disconnected"),
            Self::Timeout => f.write_str("usb operation timed out"),
            Self::Stall => f.write_str("usb endpoint stalled"),
            Self::Protocol => f.write_str("usb protocol error"),
            Self::Overcurrent => f.write_str("usb overcurrent condition"),
            Self::StateConflict => f.write_str("usb state conflict"),
            Self::ResourceExhausted => f.write_str("usb resources exhausted"),
            Self::Platform(code) => write!(f, "platform usb error {code}"),
        }
    }
}

impl fmt::Display for UsbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}
