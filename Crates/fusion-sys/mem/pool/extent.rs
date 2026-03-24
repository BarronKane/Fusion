use crate::mem::resource::RangeView;
use crate::mem::resource::ResourceRange;
use core::marker::PhantomData;

/// Unique identifier for a live extent lease returned by a pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryPoolLeaseId(pub u64);

/// Extent lease handed to the allocator-facing layer above `MemoryPool`.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct MemoryPoolLease {
    /// Unique lease identifier.
    pub(super) id: MemoryPoolLeaseId,
    /// Member that owns the leased extent.
    pub(super) member: super::MemoryPoolMemberId,
    /// Byte range relative to the member's resource base.
    pub(super) range: ResourceRange,
}

impl MemoryPoolLease {
    /// Returns the unique lease identifier.
    #[must_use]
    pub const fn id(&self) -> MemoryPoolLeaseId {
        self.id
    }

    /// Returns the member that owns this lease.
    #[must_use]
    pub const fn member(&self) -> super::MemoryPoolMemberId {
        self.member
    }

    /// Returns the leased range relative to the owning member's base.
    #[must_use]
    pub const fn range(&self) -> ResourceRange {
        self.range
    }

    /// Returns the leased length in bytes.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.range.len
    }

    /// Returns `true` when the lease is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.range.len == 0
    }
}

/// Borrowed view of an active pool lease.
///
/// This couples raw-address borrowing to both the pool and the live lease borrow so callers
/// cannot move the lease into `release_extent` while still holding the returned view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryPoolLeaseView<'a> {
    view: RangeView<'a>,
    _lease: PhantomData<&'a MemoryPoolLease>,
}

impl<'a> MemoryPoolLeaseView<'a> {
    #[must_use]
    pub(super) const fn new(view: RangeView<'a>) -> Self {
        Self {
            view,
            _lease: PhantomData,
        }
    }

    /// Returns the leased length in bytes.
    #[must_use]
    pub const fn len(self) -> usize {
        self.view.len()
    }

    /// Returns `true` when the leased range is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.view.is_empty()
    }

    /// Returns the borrowed underlying range view.
    #[must_use]
    pub const fn as_range_view(self) -> RangeView<'a> {
        self.view
    }
}

/// Request for a pool-managed extent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryPoolExtentRequest {
    /// Required extent length in bytes.
    pub len: usize,
    /// Required byte alignment for the returned extent address.
    pub align: usize,
}

impl MemoryPoolExtentRequest {
    /// Creates an extent request with a default byte alignment of 1.
    #[must_use]
    pub const fn new(len: usize) -> Self {
        Self { len, align: 1 }
    }

    /// Returns the minimum contributor/resource bytes needed to satisfy this request from an
    /// arbitrarily aligned base address.
    ///
    /// This is the honest provisioning length for caller-owned or pre-existing regions when the
    /// pool must still be free to choose an aligned offset within that region.
    #[must_use]
    pub const fn provisioning_len(self) -> Option<usize> {
        let padding = self.align.saturating_sub(1);
        self.len.checked_add(padding)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum ExtentDisposition {
    Free,
    Leased(MemoryPoolLeaseId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct ExtentRecord {
    pub member_index: usize,
    pub range: ResourceRange,
    pub disposition: ExtentDisposition,
}
