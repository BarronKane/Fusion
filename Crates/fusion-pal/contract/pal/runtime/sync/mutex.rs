use core::time::Duration;

use super::{
    MutexSupport,
    SyncError,
};

/// Low-level raw mutex contract implemented by selected platform backends.
///
/// This contract intentionally makes no fairness, FIFO wake, or starvation-freedom
/// guarantee. Backends may allow barging or scheduler-dependent wake order unless a future
/// extension advertises a stronger policy explicitly.
///
/// # Safety
///
/// Implementations must uphold acquire/release memory-ordering guarantees and ensure that
/// successful acquisition confers exclusive ownership until `unlock_unchecked` is called.
pub unsafe trait RawMutex: Send + Sync {
    /// Reports the support surface of this mutex instance.
    fn support(&self) -> MutexSupport;

    /// Blocks until the mutex is acquired or an error occurs.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot complete the acquisition honestly.
    fn lock(&self) -> Result<(), SyncError>;

    /// Attempts to acquire the mutex without blocking.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot evaluate the acquisition honestly.
    fn try_lock(&self) -> Result<bool, SyncError>;

    /// Attempts to acquire the mutex within a relative timeout.
    ///
    /// # Errors
    ///
    /// Returns an error if timed acquisition is unsupported or the backend cannot evaluate
    /// the request honestly.
    fn lock_for(&self, _timeout: Duration) -> Result<bool, SyncError> {
        Err(SyncError::unsupported())
    }

    /// Releases the mutex held by the current owner.
    ///
    /// # Safety
    ///
    /// The caller must currently hold the mutex and must call this exactly once for each
    /// successful acquisition path.
    unsafe fn unlock_unchecked(&self);
}
