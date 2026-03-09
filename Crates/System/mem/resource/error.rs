use fusion_pal::sys::mem::{MemError, MemErrorKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceErrorKind {
    InvalidRequest,
    UnsupportedRequest,
    UnsupportedOperation,
    ContractViolation,
    InvalidRange,
    OutOfMemory,
    Platform(MemErrorKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceError {
    pub kind: ResourceErrorKind,
}

impl ResourceError {
    #[must_use]
    pub const fn invalid_request() -> Self {
        Self {
            kind: ResourceErrorKind::InvalidRequest,
        }
    }

    #[must_use]
    pub const fn unsupported_request() -> Self {
        Self {
            kind: ResourceErrorKind::UnsupportedRequest,
        }
    }

    #[must_use]
    pub const fn unsupported_operation() -> Self {
        Self {
            kind: ResourceErrorKind::UnsupportedOperation,
        }
    }

    #[must_use]
    pub const fn contract_violation() -> Self {
        Self {
            kind: ResourceErrorKind::ContractViolation,
        }
    }

    #[must_use]
    pub const fn invalid_range() -> Self {
        Self {
            kind: ResourceErrorKind::InvalidRange,
        }
    }

    #[must_use]
    pub const fn out_of_memory() -> Self {
        Self {
            kind: ResourceErrorKind::OutOfMemory,
        }
    }

    #[must_use]
    pub const fn platform(kind: MemErrorKind) -> Self {
        Self {
            kind: ResourceErrorKind::Platform(kind),
        }
    }

    #[must_use]
    pub fn from_request_error(value: MemError) -> Self {
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
    pub fn from_operation_error(value: MemError) -> Self {
        match value.kind {
            MemErrorKind::OutOfMemory => Self::out_of_memory(),
            MemErrorKind::Unsupported => Self::unsupported_operation(),
            other => Self::platform(other),
        }
    }
}
