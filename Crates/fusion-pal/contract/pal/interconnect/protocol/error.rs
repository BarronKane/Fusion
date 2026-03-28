//! Error surface for protocol contracts.

use core::fmt;

/// Error returned by one protocol contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProtocolError {
    kind: ProtocolErrorKind,
}

impl ProtocolError {
    #[must_use]
    pub const fn kind(self) -> ProtocolErrorKind {
        self.kind
    }

    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: ProtocolErrorKind::Unsupported,
        }
    }

    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: ProtocolErrorKind::Invalid,
        }
    }

    #[must_use]
    pub const fn transport_mismatch() -> Self {
        Self {
            kind: ProtocolErrorKind::TransportMismatch,
        }
    }
}

/// Classification of protocol failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProtocolErrorKind {
    Unsupported,
    Invalid,
    TransportMismatch,
}

impl fmt::Display for ProtocolErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("unsupported protocol"),
            Self::Invalid => f.write_str("invalid protocol descriptor"),
            Self::TransportMismatch => f.write_str("transport does not satisfy protocol"),
        }
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}
