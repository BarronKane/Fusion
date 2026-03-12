use core::sync::atomic::AtomicU32;
use core::time::Duration;

use super::{SyncError, WaitSupport};

/// Result of a raw wait operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WaitOutcome {
    /// The wait ended because a wake or equivalent event occurred.
    Woken,
    /// The waited-on word no longer matched the expected value.
    Mismatch,
    /// The wait timed out before a wake or mismatch occurred.
    TimedOut,
    /// The wait was interrupted and the caller should decide whether to retry.
    Interrupted,
}

/// Raw, process-local wait/wake primitive over a caller-owned atomic word.
///
/// The baseline contract is intentionally pinned to [`AtomicU32`]. That matches Linux futex
/// semantics directly and keeps the common denominator explicit for mutex and semaphore state
/// words. Backends that can wait on wider values may grow that as an extension rather than
/// widening this base trait prematurely.
pub trait WaitPrimitive: Send + Sync {
    /// Reports the wait/wake support surface offered by this backend.
    fn support(&self) -> WaitSupport;

    /// Waits while `word` remains equal to `expected`.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot perform the wait honestly.
    fn wait_while_equal(
        &self,
        word: &AtomicU32,
        expected: u32,
        timeout: Option<Duration>,
    ) -> Result<WaitOutcome, SyncError>;

    /// Wakes up to one waiter on `word`.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot perform the wake honestly.
    fn wake_one(&self, word: &AtomicU32) -> Result<usize, SyncError>;

    /// Wakes all waiters on `word`.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot perform the wake honestly.
    fn wake_all(&self, word: &AtomicU32) -> Result<usize, SyncError>;
}
