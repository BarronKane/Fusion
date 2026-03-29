//! Backend-neutral hosted-fiber helper vocabulary.

mod unsupported;

use core::fmt;

pub use unsupported::*;

/// Fault-promotion callback invoked from a platform fault handler.
pub type PlatformElasticFaultHandler = fn(usize) -> bool;

/// Support surface for hosted-fiber runtime helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberHostSupport {
    /// Whether the backend can install an elastic-stack fault handler.
    pub elastic_stack_faults: bool,
    /// Whether the backend can install an alternate signal stack for carrier threads.
    pub signal_stack: bool,
    /// Whether the backend can create a wake signal compatible with readiness polling.
    pub wake_signal: bool,
}

/// Concrete hosted-fiber helper error kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FiberHostErrorKind {
    /// Hosted-fiber runtime support is unavailable on this backend.
    Unsupported,
    /// The caller supplied an invalid request.
    Invalid,
    /// Runtime state could not be coordinated honestly.
    StateConflict,
    /// The backend ran out of resources.
    ResourceExhausted,
    /// Lower-level platform failure with an opaque OS error code.
    Platform(i32),
}

/// Concrete hosted-fiber helper error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberHostError {
    kind: FiberHostErrorKind,
}

impl FiberHostError {
    /// Creates an unsupported hosted-fiber helper error.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: FiberHostErrorKind::Unsupported,
        }
    }

    /// Creates an invalid-request error.
    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: FiberHostErrorKind::Invalid,
        }
    }

    /// Creates a state-conflict error.
    #[must_use]
    pub const fn state_conflict() -> Self {
        Self {
            kind: FiberHostErrorKind::StateConflict,
        }
    }

    /// Creates a resource-exhaustion error.
    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self {
            kind: FiberHostErrorKind::ResourceExhausted,
        }
    }

    /// Creates a platform-error wrapper.
    #[must_use]
    pub const fn platform(code: i32) -> Self {
        Self {
            kind: FiberHostErrorKind::Platform(code),
        }
    }

    /// Returns the concrete hosted-fiber helper error kind.
    #[must_use]
    pub const fn kind(self) -> FiberHostErrorKind {
        self.kind
    }
}

impl fmt::Display for FiberHostErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Unsupported => f.write_str("hosted fiber support unavailable"),
            Self::Invalid => f.write_str("invalid hosted fiber request"),
            Self::StateConflict => f.write_str("hosted fiber state conflict"),
            Self::ResourceExhausted => f.write_str("hosted fiber resources exhausted"),
            Self::Platform(code) => write!(f, "hosted fiber platform error ({code})"),
        }
    }
}

impl fmt::Display for FiberHostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

/// Opaque token that identifies a wake signal target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlatformWakeToken(u64);

impl PlatformWakeToken {
    /// Returns one invalid token.
    #[must_use]
    pub const fn invalid() -> Self {
        Self(u64::MAX)
    }

    /// Rebuilds one token from raw storage.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Returns the raw storage representation for the token.
    #[must_use]
    pub const fn into_raw(self) -> u64 {
        self.0
    }

    /// Returns whether the token names one live wake target.
    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.0 != u64::MAX
    }
}
