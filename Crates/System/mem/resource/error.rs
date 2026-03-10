use fusion_pal::sys::mem::{MemError, MemErrorKind};

/// Resource-layer error categories derived from request validation and PAL failures.
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
    /// Backend-specific PAL failure classification.
    Platform(MemErrorKind),
}

/// Error returned by memory-resource creation and operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceError {
    /// Machine-readable reason for the failure.
    pub kind: ResourceErrorKind,
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

    /// Wraps a backend PAL error category.
    #[must_use]
    pub const fn platform(kind: MemErrorKind) -> Self {
        Self {
            kind: ResourceErrorKind::Platform(kind),
        }
    }

    #[must_use]
    /// Converts a PAL request-time error into a resource-layer error.
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
    /// Converts a PAL operation-time error into a resource-layer error.
    pub const fn from_operation_error(value: MemError) -> Self {
        match value.kind {
            MemErrorKind::OutOfMemory => Self::out_of_memory(),
            MemErrorKind::Unsupported => Self::unsupported_operation(),
            other => Self::platform(other),
        }
    }
}
