use core::fmt;
use core::sync::atomic::{AtomicUsize, Ordering};

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

#[cfg(test)]
mod tests {
    use super::{SharedHeader, SharedRelease};
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
