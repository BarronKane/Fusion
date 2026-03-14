use core::fmt;

use crate::sync::SyncErrorKind;
use fusion_pal::sys::mem::{MemError, MemErrorKind};

/// Resource-layer error categories derived from request validation and fusion-pal failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceErrorKind {
    /// The request shape or supplied values were invalid before reaching the platform.
    InvalidRequest,
    /// The request describes a valid concept that this resource/backend cannot realize.
    UnsupportedRequest,
    /// The resource exists, but the requested operation is not legal for this instance.
    UnsupportedOperation,
    /// The caller attempted to violate the resource's immutable contract.
    ContractViolation,
    /// A supplied subrange fell outside the resource or violated granularity rules.
    InvalidRange,
    /// The request failed because backing memory was exhausted.
    OutOfMemory,
    /// Backend-specific fusion-pal failure classification.
    Platform(MemErrorKind),
    /// Internal synchronization failure while coordinating resource state.
    SynchronizationFailure(SyncErrorKind),
}

/// Error returned by memory-resource creation and operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceError {
    /// Machine-readable reason for the failure.
    pub kind: ResourceErrorKind,
}

impl fmt::Display for ResourceErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest => f.write_str("invalid resource request"),
            Self::UnsupportedRequest => f.write_str("unsupported resource request"),
            Self::UnsupportedOperation => f.write_str("unsupported resource operation"),
            Self::ContractViolation => f.write_str("resource contract violation"),
            Self::InvalidRange => f.write_str("invalid resource range"),
            Self::OutOfMemory => f.write_str("resource allocation exhausted memory"),
            Self::Platform(kind) => write!(f, "platform resource failure ({kind})"),
            Self::SynchronizationFailure(kind) => {
                write!(f, "resource synchronization failure ({kind})")
            }
        }
    }
}

impl fmt::Display for ResourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)
    }
}

impl ResourceError {
    /// Returns an invalid-request error.
    #[must_use]
    pub const fn invalid_request() -> Self {
        Self {
            kind: ResourceErrorKind::InvalidRequest,
        }
    }

    /// Returns an unsupported-request error.
    #[must_use]
    pub const fn unsupported_request() -> Self {
        Self {
            kind: ResourceErrorKind::UnsupportedRequest,
        }
    }

    /// Returns an unsupported-operation error.
    #[must_use]
    pub const fn unsupported_operation() -> Self {
        Self {
            kind: ResourceErrorKind::UnsupportedOperation,
        }
    }

    /// Returns a contract-violation error.
    #[must_use]
    pub const fn contract_violation() -> Self {
        Self {
            kind: ResourceErrorKind::ContractViolation,
        }
    }

    /// Returns an invalid-range error.
    #[must_use]
    pub const fn invalid_range() -> Self {
        Self {
            kind: ResourceErrorKind::InvalidRange,
        }
    }

    /// Returns an out-of-memory error.
    #[must_use]
    pub const fn out_of_memory() -> Self {
        Self {
            kind: ResourceErrorKind::OutOfMemory,
        }
    }

    /// Wraps a backend fusion-pal error category.
    #[must_use]
    pub const fn platform(kind: MemErrorKind) -> Self {
        Self {
            kind: ResourceErrorKind::Platform(kind),
        }
    }

    /// Wraps an internal synchronization failure category.
    #[must_use]
    pub const fn synchronization(kind: SyncErrorKind) -> Self {
        Self {
            kind: ResourceErrorKind::SynchronizationFailure(kind),
        }
    }

    #[must_use]
    /// Converts a fusion-pal request-time error into a resource-layer error.
    pub const fn from_request_error(value: MemError) -> Self {
        match value.kind {
            MemErrorKind::OutOfMemory => Self::out_of_memory(),
            MemErrorKind::Unsupported => Self::unsupported_request(),
            MemErrorKind::InvalidInput
            | MemErrorKind::InvalidAddress
            | MemErrorKind::Misaligned
            | MemErrorKind::Overflow => Self::invalid_request(),
            other => Self::platform(other),
        }
    }

    #[must_use]
    /// Converts a fusion-pal operation-time error into a resource-layer error.
    pub const fn from_operation_error(value: MemError) -> Self {
        match value.kind {
            MemErrorKind::OutOfMemory => Self::out_of_memory(),
            MemErrorKind::Unsupported => Self::unsupported_operation(),
            other => Self::platform(other),
        }
    }

    #[must_use]
    /// Converts an internal synchronization failure into a resource-layer error.
    pub const fn from_sync_error(kind: SyncErrorKind) -> Self {
        Self::synchronization(kind)
    }
}
