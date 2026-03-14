//! Backend-neutral fusion-pal contract for read/write locks.

use core::time::Duration;

use super::{RwLockSupport, SyncError};

/// Low-level reader/writer lock contract implemented by selected platform backends.
///
/// The baseline contract intentionally makes no fairness, upgrade, downgrade, or recursive
/// locking guarantee. Those semantics must be surfaced explicitly through support metadata
/// rather than assumed from the type name.
///
/// # Safety
///
/// Implementations must uphold acquire/release memory ordering and ensure that:
/// - multiple readers may coexist only when no writer holds the lock
/// - writers hold exclusive ownership until `write_unlock_unchecked`
pub unsafe trait RawRwLock: Send + Sync {
    /// Reports the support surface of this rwlock instance.
    fn support(&self) -> RwLockSupport;

    /// Blocks until a shared/read lock is acquired or an error occurs.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot complete the acquisition honestly.
    fn read_lock(&self) -> Result<(), SyncError>;

    /// Attempts to acquire a shared/read lock without blocking.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot evaluate the acquisition honestly.
    fn try_read_lock(&self) -> Result<bool, SyncError>;

    /// Attempts to acquire a shared/read lock within a relative timeout.
    ///
    /// # Errors
    ///
    /// Returns an error if timed acquisition is unsupported or the backend cannot evaluate
    /// the request honestly.
    fn read_lock_for(&self, _timeout: Duration) -> Result<bool, SyncError> {
        Err(SyncError::unsupported())
    }

    /// Blocks until an exclusive/write lock is acquired or an error occurs.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot complete the acquisition honestly.
    fn write_lock(&self) -> Result<(), SyncError>;

    /// Attempts to acquire an exclusive/write lock without blocking.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot evaluate the acquisition honestly.
    fn try_write_lock(&self) -> Result<bool, SyncError>;

    /// Attempts to acquire an exclusive/write lock within a relative timeout.
    ///
    /// # Errors
    ///
    /// Returns an error if timed acquisition is unsupported or the backend cannot evaluate
    /// the request honestly.
    fn write_lock_for(&self, _timeout: Duration) -> Result<bool, SyncError> {
        Err(SyncError::unsupported())
    }

    /// Releases one currently held shared/read lock.
    ///
    /// # Safety
    ///
    /// The caller must currently hold a matching read lock and must call this exactly once
    /// for that successful acquisition.
    unsafe fn read_unlock_unchecked(&self);

    /// Releases the currently held exclusive/write lock.
    ///
    /// # Safety
    ///
    /// The caller must currently hold the write lock and must call this exactly once for
    /// that successful acquisition.
    unsafe fn write_unlock_unchecked(&self);
}
