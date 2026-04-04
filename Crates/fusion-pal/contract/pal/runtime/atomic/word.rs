use core::sync::atomic::Ordering;
use core::time::Duration;

use super::{
    AtomicError,
    AtomicWaitWord32Support,
    AtomicWord32Support,
};

/// Result of a compare-and-exchange attempt on one 32-bit atomic word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AtomicCompareExchangeOutcome32 {
    /// The exchange completed successfully.
    Exchanged,
    /// The observed value did not match the requested `current` value.
    Mismatch(u32),
}

/// Result of one raw wait operation over a 32-bit atomic word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AtomicWaitOutcome {
    /// The wait ended because a wake or equivalent event occurred.
    Woken,
    /// The waited-on word no longer matched the expected value.
    Mismatch,
    /// The wait timed out before a wake or mismatch occurred.
    TimedOut,
    /// The wait was interrupted and the caller should decide whether to retry.
    Interrupted,
}

/// One semantic 32-bit atomic word surface.
pub trait AtomicWord32: Send + Sync {
    /// Reports the truthful support surface of this atomic word.
    fn support(&self) -> AtomicWord32Support;

    /// Reports the truthful wait/wake support surface of this atomic word.
    fn wait_support(&self) -> AtomicWaitWord32Support;

    /// Loads the current value.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot perform the load honestly.
    fn load(&self, ordering: Ordering) -> Result<u32, AtomicError>;

    /// Stores a new value.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot perform the store honestly.
    fn store(&self, value: u32, ordering: Ordering) -> Result<(), AtomicError>;

    /// Swaps in one new value and returns the old one.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot perform the exchange honestly.
    fn swap(&self, value: u32, ordering: Ordering) -> Result<u32, AtomicError>;

    /// Performs a compare-and-exchange operation.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot perform the operation honestly.
    fn compare_exchange(
        &self,
        current: u32,
        new: u32,
        success: Ordering,
        failure: Ordering,
    ) -> Result<AtomicCompareExchangeOutcome32, AtomicError>;

    /// Atomically adds `value`, returning the previous word.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot perform the operation honestly.
    fn fetch_add(&self, value: u32, ordering: Ordering) -> Result<u32, AtomicError>;

    /// Atomically subtracts `value`, returning the previous word.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot perform the operation honestly.
    fn fetch_sub(&self, value: u32, ordering: Ordering) -> Result<u32, AtomicError>;

    /// Atomically ANDs `value`, returning the previous word.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot perform the operation honestly.
    fn fetch_and(&self, value: u32, ordering: Ordering) -> Result<u32, AtomicError>;

    /// Atomically ORs `value`, returning the previous word.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot perform the operation honestly.
    fn fetch_or(&self, value: u32, ordering: Ordering) -> Result<u32, AtomicError>;

    /// Atomically XORs `value`, returning the previous word.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot perform the operation honestly.
    fn fetch_xor(&self, value: u32, ordering: Ordering) -> Result<u32, AtomicError>;

    /// Waits while this word remains equal to `expected`.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot perform the wait honestly.
    fn wait_while_equal(
        &self,
        expected: u32,
        timeout: Option<Duration>,
    ) -> Result<AtomicWaitOutcome, AtomicError>;

    /// Wakes up to one waiter on this word.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot perform the wake honestly.
    fn wake_one(&self) -> Result<usize, AtomicError>;

    /// Wakes all waiters on this word.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot perform the wake honestly.
    fn wake_all(&self) -> Result<usize, AtomicError>;
}
