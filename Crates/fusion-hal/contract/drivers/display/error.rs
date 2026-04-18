//! Shared display driver-family errors.

use core::fmt;

/// Kind of failure surfaced by display driver families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayErrorKind {
    Unsupported,
    Invalid,
    Busy,
    ResourceExhausted,
    StateConflict,
    Timeout,
    Disconnected,
    NegotiationFailed,
    Platform(i32),
}

/// Shared display driver-family error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayError {
    kind: DisplayErrorKind,
}

impl DisplayError {
    #[must_use]
    pub const fn new(kind: DisplayErrorKind) -> Self {
        Self { kind }
    }

    #[must_use]
    pub const fn kind(self) -> DisplayErrorKind {
        self.kind
    }

    #[must_use]
    pub const fn unsupported() -> Self {
        Self::new(DisplayErrorKind::Unsupported)
    }

    #[must_use]
    pub const fn invalid() -> Self {
        Self::new(DisplayErrorKind::Invalid)
    }

    #[must_use]
    pub const fn busy() -> Self {
        Self::new(DisplayErrorKind::Busy)
    }

    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self::new(DisplayErrorKind::ResourceExhausted)
    }

    #[must_use]
    pub const fn state_conflict() -> Self {
        Self::new(DisplayErrorKind::StateConflict)
    }

    #[must_use]
    pub const fn timeout() -> Self {
        Self::new(DisplayErrorKind::Timeout)
    }

    #[must_use]
    pub const fn disconnected() -> Self {
        Self::new(DisplayErrorKind::Disconnected)
    }

    #[must_use]
    pub const fn negotiation_failed() -> Self {
        Self::new(DisplayErrorKind::NegotiationFailed)
    }

    #[must_use]
    pub const fn platform(code: i32) -> Self {
        Self::new(DisplayErrorKind::Platform(code))
    }
}

impl fmt::Display for DisplayErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Unsupported => f.write_str("display operation unsupported"),
            Self::Invalid => f.write_str("invalid display request"),
            Self::Busy => f.write_str("display resource busy"),
            Self::ResourceExhausted => f.write_str("display resources exhausted"),
            Self::StateConflict => f.write_str("display state conflict"),
            Self::Timeout => f.write_str("display operation timed out"),
            Self::Disconnected => f.write_str("display sink disconnected"),
            Self::NegotiationFailed => f.write_str("display negotiation failed"),
            Self::Platform(code) => write!(f, "display platform error {code}"),
        }
    }
}

impl fmt::Display for DisplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

/// Result alias for display driver-family work.
pub type DisplayResult<T> = Result<T, DisplayError>;
