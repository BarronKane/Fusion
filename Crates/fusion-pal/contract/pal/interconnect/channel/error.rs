//! Error surface for channel contracts.

use core::fmt;

use crate::contract::pal::interconnect::protocol::ProtocolError;
use crate::contract::pal::interconnect::transport::TransportError;

/// Error returned by one channel implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChannelError {
    kind: ChannelErrorKind,
}

impl ChannelError {
    #[must_use]
    pub const fn kind(self) -> ChannelErrorKind {
        self.kind
    }

    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: ChannelErrorKind::Unsupported,
        }
    }

    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: ChannelErrorKind::Invalid,
        }
    }

    #[must_use]
    pub const fn busy() -> Self {
        Self {
            kind: ChannelErrorKind::Busy,
        }
    }

    #[must_use]
    pub const fn permission_denied() -> Self {
        Self {
            kind: ChannelErrorKind::PermissionDenied,
        }
    }

    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self {
            kind: ChannelErrorKind::ResourceExhausted,
        }
    }

    #[must_use]
    pub const fn state_conflict() -> Self {
        Self {
            kind: ChannelErrorKind::StateConflict,
        }
    }

    #[must_use]
    pub const fn protocol_mismatch() -> Self {
        Self {
            kind: ChannelErrorKind::ProtocolMismatch,
        }
    }

    #[must_use]
    pub const fn transport_denied() -> Self {
        Self {
            kind: ChannelErrorKind::TransportDenied,
        }
    }

    #[must_use]
    pub const fn platform(code: i32) -> Self {
        Self {
            kind: ChannelErrorKind::Platform(code),
        }
    }
}

impl From<TransportError> for ChannelError {
    fn from(value: TransportError) -> Self {
        match value.kind() {
            crate::contract::pal::interconnect::transport::TransportErrorKind::Unsupported => {
                Self::unsupported()
            }
            crate::contract::pal::interconnect::transport::TransportErrorKind::Invalid => {
                Self::invalid()
            }
            crate::contract::pal::interconnect::transport::TransportErrorKind::Busy => Self::busy(),
            crate::contract::pal::interconnect::transport::TransportErrorKind::PermissionDenied => {
                Self::permission_denied()
            }
            crate::contract::pal::interconnect::transport::TransportErrorKind::ResourceExhausted => {
                Self::resource_exhausted()
            }
            crate::contract::pal::interconnect::transport::TransportErrorKind::StateConflict => {
                Self::state_conflict()
            }
            crate::contract::pal::interconnect::transport::TransportErrorKind::NotAttached => {
                Self::transport_denied()
            }
            crate::contract::pal::interconnect::transport::TransportErrorKind::Platform(code) => {
                Self::platform(code)
            }
        }
    }
}

impl From<ProtocolError> for ChannelError {
    fn from(value: ProtocolError) -> Self {
        match value.kind() {
            crate::contract::pal::interconnect::protocol::ProtocolErrorKind::Unsupported => {
                Self::unsupported()
            }
            crate::contract::pal::interconnect::protocol::ProtocolErrorKind::Invalid => {
                Self::invalid()
            }
            crate::contract::pal::interconnect::protocol::ProtocolErrorKind::TransportMismatch => {
                Self::protocol_mismatch()
            }
        }
    }
}

/// Classification of channel failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelErrorKind {
    Unsupported,
    Invalid,
    Busy,
    PermissionDenied,
    ResourceExhausted,
    StateConflict,
    ProtocolMismatch,
    TransportDenied,
    Platform(i32),
}

impl fmt::Display for ChannelErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("unsupported channel operation"),
            Self::Invalid => f.write_str("invalid channel request"),
            Self::Busy => f.write_str("channel is busy"),
            Self::PermissionDenied => f.write_str("channel permission denied"),
            Self::ResourceExhausted => f.write_str("channel resources exhausted"),
            Self::StateConflict => f.write_str("channel state conflict"),
            Self::ProtocolMismatch => f.write_str("channel protocol mismatch"),
            Self::TransportDenied => f.write_str("channel transport denied"),
            Self::Platform(code) => write!(f, "platform channel error ({code})"),
        }
    }
}

impl fmt::Display for ChannelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}
