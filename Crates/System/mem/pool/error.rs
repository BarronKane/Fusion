use fusion_pal::sys::mem::{MemError, MemErrorKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoolErrorKind {
    InvalidRequest,
    UnsupportedRequirement,
    ProhibitionViolated,
    OutOfMemory,
    InvalidRange,
    Platform(MemErrorKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PoolError {
    pub kind: PoolErrorKind,
}

impl PoolError {
    #[must_use]
    pub const fn invalid_request() -> Self {
        Self {
            kind: PoolErrorKind::InvalidRequest,
        }
    }

    #[must_use]
    pub const fn unsupported_requirement() -> Self {
        Self {
            kind: PoolErrorKind::UnsupportedRequirement,
        }
    }

    #[must_use]
    pub const fn prohibition_violated() -> Self {
        Self {
            kind: PoolErrorKind::ProhibitionViolated,
        }
    }

    #[must_use]
    pub const fn out_of_memory() -> Self {
        Self {
            kind: PoolErrorKind::OutOfMemory,
        }
    }

    #[must_use]
    pub const fn invalid_range() -> Self {
        Self {
            kind: PoolErrorKind::InvalidRange,
        }
    }

    #[must_use]
    pub const fn platform(kind: MemErrorKind) -> Self {
        Self {
            kind: PoolErrorKind::Platform(kind),
        }
    }
}

impl From<MemError> for PoolError {
    fn from(value: MemError) -> Self {
        match value.kind {
            MemErrorKind::OutOfMemory => Self::out_of_memory(),
            MemErrorKind::Unsupported => Self::unsupported_requirement(),
            other => Self::platform(other),
        }
    }
}
