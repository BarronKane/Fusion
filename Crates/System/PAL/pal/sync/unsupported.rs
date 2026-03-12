//! Backend-neutral unsupported synchronization implementations.
//!
//! These types intentionally realize the PAL sync contracts in an always-unsupported form.
//! They are useful for hosted stubs and any other backend path that needs to report truthful
//! absence of synchronization primitives without inventing platform-specific behavior.

use core::sync::atomic::AtomicU32;
use core::time::Duration;

use super::{
    MutexSupport, RawMutex, RawSemaphore, SemaphoreSupport, SyncBase, SyncError, SyncSupport,
    WaitOutcome, WaitPrimitive, WaitSupport,
};

/// Unsupported synchronization provider placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedSync;

/// Unsupported raw mutex placeholder.
#[derive(Debug, Default)]
pub struct UnsupportedRawMutex;

/// Unsupported counting semaphore placeholder.
#[derive(Debug, Default)]
pub struct UnsupportedSemaphore;

impl UnsupportedSync {
    /// Creates a new unsupported synchronization provider placeholder.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl SyncBase for UnsupportedSync {
    fn support(&self) -> SyncSupport {
        SyncSupport::unsupported()
    }
}

impl WaitPrimitive for UnsupportedSync {
    fn support(&self) -> WaitSupport {
        WaitSupport::unsupported()
    }

    fn wait_while_equal(
        &self,
        _word: &AtomicU32,
        _expected: u32,
        _timeout: Option<Duration>,
    ) -> Result<WaitOutcome, SyncError> {
        Err(SyncError::unsupported())
    }

    fn wake_one(&self, _word: &AtomicU32) -> Result<usize, SyncError> {
        Err(SyncError::unsupported())
    }

    fn wake_all(&self, _word: &AtomicU32) -> Result<usize, SyncError> {
        Err(SyncError::unsupported())
    }
}

impl UnsupportedRawMutex {
    /// Creates a new unsupported raw mutex placeholder.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

// SAFETY: this type never successfully acquires a lock and therefore cannot violate mutex
// ownership semantics through its no-op unsupported surface.
unsafe impl RawMutex for UnsupportedRawMutex {
    fn support(&self) -> MutexSupport {
        MutexSupport::unsupported()
    }

    fn lock(&self) -> Result<(), SyncError> {
        Err(SyncError::unsupported())
    }

    fn try_lock(&self) -> Result<bool, SyncError> {
        Err(SyncError::unsupported())
    }

    unsafe fn unlock_unchecked(&self) {}
}

impl UnsupportedSemaphore {
    /// Returns an unsupported semaphore construction result.
    ///
    /// # Errors
    ///
    /// Always returns `Unsupported`, because this placeholder intentionally does not expose
    /// a realizable semaphore implementation.
    pub const fn new(_initial: u32, _max: u32) -> Result<Self, SyncError> {
        Err(SyncError::unsupported())
    }
}

impl RawSemaphore for UnsupportedSemaphore {
    fn support(&self) -> SemaphoreSupport {
        SemaphoreSupport::unsupported()
    }

    fn acquire(&self) -> Result<(), SyncError> {
        Err(SyncError::unsupported())
    }

    fn try_acquire(&self) -> Result<bool, SyncError> {
        Err(SyncError::unsupported())
    }

    fn release(&self, _permits: u32) -> Result<(), SyncError> {
        Err(SyncError::unsupported())
    }

    fn max_permits(&self) -> u32 {
        0
    }
}
