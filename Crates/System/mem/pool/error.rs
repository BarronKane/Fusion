use core::fmt;

use crate::mem::resource::ResourceErrorKind;
use crate::sync::SyncErrorKind;

/// Classification of `MemoryPool` failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryPoolErrorKind {
    /// The supplied build or extent request was structurally invalid.
    InvalidRequest,
    /// The requested policy or contributor shape is not implemented honestly yet.
    UnsupportedPolicy,
    /// The supplied contributor does not belong in this pool.
    IncompatibleContributor,
    /// Fixed pool metadata was exhausted while trying to track extents or leases.
    MetadataExhausted,
    /// No suitable free extent currently exists in the pool.
    CapacityExhausted,
    /// The supplied lease does not identify a currently leased extent in this pool.
    UnknownLease,
    /// Resource-layer failure while validating or borrowing a contributor range.
    ResourceFailure(ResourceErrorKind),
    /// Internal synchronization failure while coordinating pool state.
    SynchronizationFailure(SyncErrorKind),
}

/// Error returned by `fusion-sys::mem::pool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryPoolError {
    /// Machine-readable reason for the failure.
    pub kind: MemoryPoolErrorKind,
}

impl MemoryPoolError {
    /// Returns an invalid-request error.
    #[must_use]
    pub const fn invalid_request() -> Self {
        Self {
            kind: MemoryPoolErrorKind::InvalidRequest,
        }
    }

    /// Returns an unsupported-policy error.
    #[must_use]
    pub const fn unsupported_policy() -> Self {
        Self {
            kind: MemoryPoolErrorKind::UnsupportedPolicy,
        }
    }

    /// Returns an incompatible-contributor error.
    #[must_use]
    pub const fn incompatible_contributor() -> Self {
        Self {
            kind: MemoryPoolErrorKind::IncompatibleContributor,
        }
    }

    /// Returns a metadata-exhausted error.
    #[must_use]
    pub const fn metadata_exhausted() -> Self {
        Self {
            kind: MemoryPoolErrorKind::MetadataExhausted,
        }
    }

    /// Returns a capacity-exhausted error.
    #[must_use]
    pub const fn capacity_exhausted() -> Self {
        Self {
            kind: MemoryPoolErrorKind::CapacityExhausted,
        }
    }

    /// Returns an unknown-lease error.
    #[must_use]
    pub const fn unknown_lease() -> Self {
        Self {
            kind: MemoryPoolErrorKind::UnknownLease,
        }
    }

    /// Wraps a resource-layer failure.
    #[must_use]
    pub const fn resource(kind: ResourceErrorKind) -> Self {
        Self {
            kind: MemoryPoolErrorKind::ResourceFailure(kind),
        }
    }

    /// Wraps a synchronization failure.
    #[must_use]
    pub const fn synchronization(kind: SyncErrorKind) -> Self {
        Self {
            kind: MemoryPoolErrorKind::SynchronizationFailure(kind),
        }
    }
}

impl fmt::Display for MemoryPoolErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest => f.write_str("invalid memory-pool request"),
            Self::UnsupportedPolicy => f.write_str("unsupported memory-pool policy"),
            Self::IncompatibleContributor => f.write_str("incompatible memory-pool contributor"),
            Self::MetadataExhausted => f.write_str("memory-pool metadata exhausted"),
            Self::CapacityExhausted => f.write_str("memory-pool capacity exhausted"),
            Self::UnknownLease => f.write_str("unknown memory-pool lease"),
            Self::ResourceFailure(kind) => write!(f, "memory-pool resource failure ({kind})"),
            Self::SynchronizationFailure(kind) => {
                write!(f, "memory-pool synchronization failure ({kind})")
            }
        }
    }
}

impl fmt::Display for MemoryPoolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}
