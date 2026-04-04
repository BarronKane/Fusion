use core::fmt;
use core::marker::PhantomData;
use core::ops::Deref;
use core::ptr::NonNull;
use core::sync::atomic::{
    AtomicUsize,
    Ordering,
};

use fusion_pal::sys::sync::SyncError;

/// Result of releasing one intrusive shared-control reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SharedRelease {
    /// Other live references remain after this release.
    Remaining,
    /// This release consumed the final live reference.
    Last,
}

/// Checked shared-reference header for intrusive/shared control blocks.
///
/// This primitive only manages the reference count. Storage ownership and final
/// reclamation remain the caller's responsibility.
#[derive(Debug)]
pub struct SharedHeader {
    refs: AtomicUsize,
}

impl SharedHeader {
    /// Creates a header with one live owner.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            refs: AtomicUsize::new(1),
        }
    }

    /// Returns the current observed strong-reference count.
    #[must_use]
    pub fn ref_count(&self) -> usize {
        self.refs.load(Ordering::Acquire)
    }

    /// Attempts to retain one additional live reference.
    ///
    /// # Errors
    ///
    /// Returns [`SyncError::invalid()`] if the control block is already dead or
    /// [`SyncError::overflow()`] if the count would wrap.
    pub fn try_retain(&self) -> Result<(), SyncError> {
        self.refs
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                if current == 0 {
                    None
                } else {
                    current.checked_add(1)
                }
            })
            .map(|_| ())
            .map_err(|current| {
                if current == 0 {
                    SyncError::invalid()
                } else {
                    SyncError::overflow()
                }
            })
    }

    /// Releases one live reference.
    ///
    /// # Errors
    ///
    /// Returns [`SyncError::invalid()`] when the header was already dead.
    pub fn release(&self) -> Result<SharedRelease, SyncError> {
        self.refs
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_sub(1)
            })
            .map(|previous| {
                if previous == 1 {
                    SharedRelease::Last
                } else {
                    SharedRelease::Remaining
                }
            })
            .map_err(|_| SyncError::invalid())
    }
}

impl Default for SharedHeader {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SharedRelease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Remaining => f.write_str("shared control still has live owners"),
            Self::Last => f.write_str("shared control reached its final owner"),
        }
    }
}

/// Fallible retain/clone contract for shared lifetime handles.
pub trait Retainable: Sized {
    /// Error reported when the handle cannot be retained honestly.
    type Error;

    /// Attempts to retain one additional handle.
    ///
    /// # Errors
    ///
    /// Returns the backing-specific error when the handle cannot be retained honestly.
    fn try_retain(&self) -> Result<Self, Self::Error>;
}

/// Shared-handle contract for stable shared backing.
///
/// # Safety
///
/// Implementors must guarantee that the returned pointer remains valid for as long as every
/// retained handle produced through this contract stays alive.
pub unsafe trait SharedBacking<T>: Retainable {
    /// Returns the stable payload pointer backing this shared handle.
    fn as_ptr(&self) -> *const T;
}

/// Stable retained handle to immortal backing that outlives the entire process.
pub struct RetainedHandle<T: 'static> {
    ptr: NonNull<T>,
    _marker: PhantomData<&'static T>,
}

impl<T: 'static> Copy for RetainedHandle<T> {}

impl<T: 'static> Clone for RetainedHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

unsafe impl<T: Sync + 'static> Send for RetainedHandle<T> {}
unsafe impl<T: Sync + 'static> Sync for RetainedHandle<T> {}

impl<T: 'static> RetainedHandle<T> {
    /// Creates a retained handle from process-lifetime static backing.
    #[must_use]
    pub fn from_static(value: &'static T) -> Self {
        Self {
            ptr: NonNull::from(value),
            _marker: PhantomData,
        }
    }

    pub(crate) const fn from_nonnull(ptr: NonNull<T>) -> Self {
        Self {
            ptr,
            _marker: PhantomData,
        }
    }

    /// Returns the stable payload pointer.
    #[must_use]
    pub const fn as_ptr(&self) -> *const T {
        self.ptr.as_ptr()
    }
}

impl<T: 'static> Deref for RetainedHandle<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: `RetainedHandle<T>` is only constructed from immortal backing.
        unsafe { self.ptr.as_ref() }
    }
}

impl<T: 'static> AsRef<T> for RetainedHandle<T> {
    fn as_ref(&self) -> &T {
        self
    }
}

impl<T: 'static> fmt::Debug for RetainedHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RetainedHandle")
            .field("ptr", &self.ptr)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SharedHeader,
        SharedRelease,
    };
    use crate::sync::SyncErrorKind;

    #[test]
    fn shared_header_tracks_clone_and_final_release() {
        let header = SharedHeader::new();
        assert_eq!(header.ref_count(), 1);

        header.try_retain().expect("retain should succeed");
        assert_eq!(header.ref_count(), 2);
        assert_eq!(
            header.release().expect("first release should succeed"),
            SharedRelease::Remaining
        );
        assert_eq!(
            header.release().expect("final release should succeed"),
            SharedRelease::Last
        );
    }

    #[test]
    fn shared_header_rejects_retain_or_release_after_death() {
        let header = SharedHeader::new();
        assert_eq!(
            header.release().expect("final release should succeed"),
            SharedRelease::Last
        );
        assert_eq!(
            header
                .try_retain()
                .expect_err("dead header should reject retain")
                .kind,
            SyncErrorKind::Invalid
        );
        assert_eq!(
            header
                .release()
                .expect_err("dead header should reject release")
                .kind,
            SyncErrorKind::Invalid
        );
    }
}
