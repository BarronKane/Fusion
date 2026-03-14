use core::fmt;
use core::marker::PhantomData;
use core::ptr::NonNull;

use fusion_pal::sys::mem::Region;

use super::{ResourceError, ResourceRange};

/// Borrowed view of a governed contiguous range.
///
/// A `RangeView` keeps raw-address access tied to a live borrow of the owning resource or
/// reservation. Callers can inspect length and containment safely, but extracting the raw base
/// pointer remains explicit and unsafe because the resulting address becomes meaningless once the
/// owner is dropped or remapped.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct RangeView<'a> {
    region: Region,
    _owner: PhantomData<&'a ()>,
}

impl fmt::Debug for RangeView<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RangeView")
            .field("region", &self.region)
            .finish()
    }
}

impl RangeView<'_> {
    /// Creates a borrowed view from a raw region descriptor.
    #[must_use]
    pub(super) const fn new(region: Region) -> Self {
        Self {
            region,
            _owner: PhantomData,
        }
    }

    /// Returns the length of the governed range.
    #[must_use]
    pub const fn len(self) -> usize {
        self.region.len
    }

    /// Returns `true` when the borrowed range is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.region.len == 0
    }

    /// Returns the exclusive end address of the governed range when it does not overflow the
    /// address space.
    #[must_use]
    pub fn checked_end_addr(self) -> Option<usize> {
        self.region.checked_end_addr()
    }

    /// Returns the exclusive end address of the governed range when it does not overflow the
    /// address space.
    #[must_use]
    pub fn end_addr(self) -> Option<usize> {
        self.checked_end_addr()
    }

    /// Returns `true` when `ptr` lies within the borrowed range.
    #[must_use]
    pub fn contains(self, ptr: *const u8) -> bool {
        self.region.contains(ptr as usize)
    }

    /// Returns a checked borrowed subrange.
    ///
    /// # Errors
    /// Returns an error when the requested range is empty or falls outside the borrowed range.
    pub fn subrange(self, range: ResourceRange) -> Result<Self, ResourceError> {
        if range.len == 0 {
            return Err(ResourceError::invalid_range());
        }

        self.region
            .subrange(range.offset, range.len)
            .map(Self::new)
            .map_err(|_| ResourceError::invalid_range())
    }

    /// Returns the raw base pointer of the borrowed range.
    ///
    /// # Safety
    /// The returned pointer is only valid while the owning resource or reservation remains live
    /// and unchanged. Callers must not retain it beyond the borrow represented by this view.
    #[must_use]
    pub const unsafe fn base(self) -> NonNull<u8> {
        self.region.base
    }

    /// Returns the raw fusion-pal region descriptor behind this borrowed view.
    ///
    /// # Safety
    /// The returned region is non-owning and only valid while the owning resource or
    /// reservation remains live and unchanged. Treating it as a durable ownership token is
    /// incorrect.
    #[must_use]
    pub const unsafe fn raw_region(self) -> Region {
        self.region
    }
}
