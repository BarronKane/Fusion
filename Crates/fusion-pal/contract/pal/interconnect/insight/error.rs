//! Error surface for insight/debug side-channel construction.

use core::fmt;

use crate::contract::pal::interconnect::channel::{ChannelError, ChannelErrorKind};

/// Error returned by one insight-side-channel surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InsightError {
    kind: InsightErrorKind,
}

impl InsightError {
    /// Returns the underlying error kind.
    #[must_use]
    pub const fn kind(self) -> InsightErrorKind {
        self.kind
    }

    /// Returns one unsupported insight error.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: InsightErrorKind::Unsupported,
        }
    }

    /// Returns one feature-disabled insight error.
    #[must_use]
    pub const fn not_enabled() -> Self {
        Self {
            kind: InsightErrorKind::NotEnabled,
        }
    }

    /// Returns one invalid insight error.
    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: InsightErrorKind::Invalid,
        }
    }

    /// Returns one channel-backed insight error.
    #[must_use]
    pub const fn channel(kind: ChannelErrorKind) -> Self {
        Self {
            kind: InsightErrorKind::Channel(kind),
        }
    }
}

impl From<ChannelError> for InsightError {
    fn from(value: ChannelError) -> Self {
        Self::channel(value.kind())
    }
}

/// Classification of insight-side-channel failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InsightErrorKind {
    Unsupported,
    NotEnabled,
    Invalid,
    Channel(ChannelErrorKind),
}

impl fmt::Display for InsightErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("unsupported insight operation"),
            Self::NotEnabled => f.write_str("debug-insights feature is not enabled"),
            Self::Invalid => f.write_str("invalid insight request"),
            Self::Channel(kind) => write!(f, "insight channel error: {kind}"),
        }
    }
}

impl fmt::Display for InsightError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}
