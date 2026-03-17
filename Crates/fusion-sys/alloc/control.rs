use core::fmt;
use core::mem::{ManuallyDrop, align_of, size_of};
use core::ops::Deref;
use core::ptr::{self, NonNull, addr_of_mut};

use crate::sync::{SharedHeader, SharedRelease};

use super::{AllocError, AssignedPoolExtent, MemoryPoolExtentRequest};

#[repr(C)]
struct ControlBlock<T> {
    header: SharedHeader,
    value: T,
    extent: ManuallyDrop<AssignedPoolExtent>,
}

pub struct ControlLease<T> {
    ptr: NonNull<ControlBlock<T>>,
}

unsafe impl<T: Send> Send for ControlLease<T> {}
unsafe impl<T: Sync> Sync for ControlLease<T> {}

impl<T> fmt::Debug for ControlLease<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ControlLease")
            .field("ptr", &self.ptr)
            .finish_non_exhaustive()
    }
}

impl<T> ControlLease<T> {
    /// Returns the extent request needed to host one control block for `T`.
    ///
    /// # Errors
    ///
    /// Returns an error when the control-block shape cannot be represented honestly.
    pub const fn extent_request() -> Result<MemoryPoolExtentRequest, AllocError> {
        let len = size_of::<ControlBlock<T>>();
        if len == 0 {
            return Err(AllocError::invalid_request());
        }
        Ok(MemoryPoolExtentRequest {
            len,
            align: align_of::<ControlBlock<T>>(),
        })
    }

    pub(crate) fn new(extent: AssignedPoolExtent, value: T) -> Result<Self, AllocError> {
        let region = extent.region();
        if region.len < size_of::<ControlBlock<T>>() {
            return Err(AllocError::invalid_request());
        }
        if !(region.base.as_ptr() as usize).is_multiple_of(align_of::<ControlBlock<T>>()) {
            return Err(AllocError::invalid_request());
        }

        let ptr = region.base.cast::<ControlBlock<T>>();
        // SAFETY: the assigned extent is uniquely owned here, sufficiently aligned, and large
        // enough to host the control block exactly once.
        unsafe {
            ptr.as_ptr().write(ControlBlock {
                header: SharedHeader::new(),
                value,
                extent: ManuallyDrop::new(extent),
            });
        }

        Ok(Self { ptr })
    }

    /// Attempts to retain one additional control lease.
    ///
    /// # Errors
    ///
    /// Returns an error when the shared-control count cannot be retained honestly.
    pub fn try_clone(&self) -> Result<Self, AllocError> {
        self.block()
            .header
            .try_retain()
            .map_err(|error| AllocError::synchronization(error.kind))?;
        Ok(Self { ptr: self.ptr })
    }

    /// Returns the stable payload pointer for this control lease.
    #[must_use]
    pub const fn as_ptr(&self) -> *const T {
        core::ptr::from_ref(&self.block().value)
    }

    const fn block(&self) -> &ControlBlock<T> {
        // SAFETY: `ptr` always points at a live control block while a lease exists.
        unsafe { self.ptr.as_ref() }
    }
}

impl<T> Deref for ControlLease<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.block().value
    }
}

impl<T> AsRef<T> for ControlLease<T> {
    fn as_ref(&self) -> &T {
        self
    }
}

impl<T> Drop for ControlLease<T> {
    fn drop(&mut self) {
        let Ok(release) = self.block().header.release() else {
            return;
        };
        if release != SharedRelease::Last {
            return;
        }

        let block = self.ptr.as_ptr();
        // SAFETY: the final lease exclusively owns the control block. The value must be dropped
        // before releasing the backing extent because the block storage itself resides inside
        // that extent.
        unsafe {
            ptr::drop_in_place(addr_of_mut!((*block).value));
            let extent = ManuallyDrop::take(&mut (*block).extent);
            drop(extent);
        }
    }
}
