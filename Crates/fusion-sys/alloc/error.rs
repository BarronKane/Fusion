use core::fmt;

use crate::mem::pool::{MemoryPoolError, MemoryPoolErrorKind};
use crate::mem::resource::{ResourceError, ResourceErrorKind};
use crate::sync::SyncErrorKind;

/// Allocation-layer failure classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AllocErrorKind {
    /// The requested allocation shape was invalid.
    InvalidRequest,
    /// The referenced allocator domain does not exist.
    InvalidDomain,
    /// The requested allocator operation is unsupported.
    Unsupported,
    /// Current policy forbids the requested allocator mode.
    PolicyDenied,
    /// Fixed allocator metadata or builder storage was exhausted.
    MetadataExhausted,
    /// No bounded allocator capacity was available.
    CapacityExhausted,
    /// The request failed because backing memory was exhausted.
    OutOfMemory,
    /// Lower-level governed-resource failure.
    ResourceFailure(ResourceErrorKind),
    /// Lower-level pool-substrate failure.
    PoolFailure(MemoryPoolErrorKind),
    /// Internal synchronization failure while coordinating allocator state.
    SynchronizationFailure(SyncErrorKind),
}

/// Error returned by `fusion-sys::alloc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AllocError {
    /// Machine-readable reason for the failure.
    pub kind: AllocErrorKind,
}

impl AllocError {
    /// Returns an invalid-request error.
    #[must_use]
    pub const fn invalid_request() -> Self {
        Self {
            kind: AllocErrorKind::InvalidRequest,
        }
    }

    /// Returns an invalid-domain error.
    #[must_use]
    pub const fn invalid_domain() -> Self {
        Self {
            kind: AllocErrorKind::InvalidDomain,
        }
    }

    /// Returns an unsupported-operation error.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: AllocErrorKind::Unsupported,
        }
    }

    /// Returns a policy-denied error.
    #[must_use]
    pub const fn policy_denied() -> Self {
        Self {
            kind: AllocErrorKind::PolicyDenied,
        }
    }

    /// Returns a metadata-exhausted error.
    #[must_use]
    pub const fn metadata_exhausted() -> Self {
        Self {
            kind: AllocErrorKind::MetadataExhausted,
        }
    }

    /// Returns a bounded-capacity-exhausted error.
    #[must_use]
    pub const fn capacity_exhausted() -> Self {
        Self {
            kind: AllocErrorKind::CapacityExhausted,
        }
    }

    /// Returns an out-of-memory error.
    #[must_use]
    pub const fn out_of_memory() -> Self {
        Self {
            kind: AllocErrorKind::OutOfMemory,
        }
    }

    /// Returns an internal synchronization failure.
    #[must_use]
    pub const fn synchronization(kind: SyncErrorKind) -> Self {
        Self {
            kind: AllocErrorKind::SynchronizationFailure(kind),
        }
    }
}

impl From<ResourceError> for AllocError {
    fn from(value: ResourceError) -> Self {
        match value.kind {
            ResourceErrorKind::OutOfMemory => Self::out_of_memory(),
            kind => Self {
                kind: AllocErrorKind::ResourceFailure(kind),
            },
        }
    }
}

impl From<MemoryPoolError> for AllocError {
    fn from(value: MemoryPoolError) -> Self {
        match value.kind {
            MemoryPoolErrorKind::CapacityExhausted | MemoryPoolErrorKind::MetadataExhausted => {
                Self::capacity_exhausted()
            }
            kind => Self {
                kind: AllocErrorKind::PoolFailure(kind),
            },
        }
    }
}

impl fmt::Display for AllocErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest => f.write_str("invalid allocation request"),
            Self::InvalidDomain => f.write_str("invalid allocator domain"),
            Self::Unsupported => f.write_str("allocator operation unsupported"),
            Self::PolicyDenied => f.write_str("allocator policy denied the request"),
            Self::MetadataExhausted => f.write_str("allocator metadata exhausted"),
            Self::CapacityExhausted => f.write_str("allocator capacity exhausted"),
            Self::OutOfMemory => f.write_str("allocator exhausted backing memory"),
            Self::ResourceFailure(kind) => write!(f, "resource failure ({kind})"),
            Self::PoolFailure(kind) => write!(f, "pool-substrate failure ({kind})"),
            Self::SynchronizationFailure(kind) => {
                write!(f, "allocator synchronization failure ({kind})")
            }
        }
    }
}

impl fmt::Display for AllocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}
