use core::time::Duration;

use super::{
    SemaphoreSupport,
    SyncError,
};

/// Low-level counting semaphore contract implemented by selected platform backends.
pub trait RawSemaphoreContract: Send + Sync {
    /// Reports the support surface of this semaphore instance.
    fn support(&self) -> SemaphoreSupport;

    /// Blocks until a permit is acquired or an error occurs.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot complete the acquisition honestly.
    fn acquire(&self) -> Result<(), SyncError>;

    /// Attempts to acquire a permit without blocking.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot evaluate the acquisition honestly.
    fn try_acquire(&self) -> Result<bool, SyncError>;

    /// Attempts to acquire a permit within a relative timeout.
    ///
    /// # Errors
    ///
    /// Returns an error if timed acquisition is unsupported or the backend cannot evaluate
    /// the request honestly.
    fn acquire_for(&self, _timeout: Duration) -> Result<bool, SyncError> {
        Err(SyncError::unsupported())
    }

    /// Releases `permits` back to the semaphore.
    ///
    /// # Errors
    ///
    /// Returns an error if the requested release would violate semaphore limits.
    fn release(&self, permits: u32) -> Result<(), SyncError>;

    /// Returns the maximum permit count this semaphore can represent.
    fn max_permits(&self) -> u32;
}
