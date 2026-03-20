//! Backend-neutral unsupported synchronization implementations.
//!
//! These types intentionally realize the fusion-pal sync contracts in an always-unsupported form.
//! They are useful for hosted stubs and any other backend path that needs to report truthful
//! absence of synchronization primitives without inventing platform-specific behavior.

use core::sync::atomic::AtomicU32;
use core::time::Duration;

use super::{
    MutexSupport,
    OnceBeginResult,
    OnceState,
    OnceSupport,
    RawMutex,
    RawOnce,
    RawRwLock,
    RawSemaphore,
    RwLockSupport,
    SemaphoreSupport,
    SyncBase,
    SyncError,
    SyncSupport,
    WaitOutcome,
    WaitPrimitive,
    WaitSupport,
};

/// Unsupported synchronization provider placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedSync;

/// Unsupported raw mutex placeholder.
#[derive(Debug, Default)]
pub struct UnsupportedRawMutex;

/// Unsupported raw once placeholder.
#[derive(Debug, Default)]
pub struct UnsupportedRawOnce;

/// Unsupported counting semaphore placeholder.
#[derive(Debug, Default)]
pub struct UnsupportedSemaphore;

/// Unsupported raw rwlock placeholder.
#[derive(Debug, Default)]
pub struct UnsupportedRawRwLock;

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

impl UnsupportedRawOnce {
    /// Creates a new unsupported raw once placeholder.
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

impl RawOnce for UnsupportedRawOnce {
    fn support(&self) -> OnceSupport {
        OnceSupport::unsupported()
    }

    fn state(&self) -> OnceState {
        OnceState::Uninitialized
    }

    fn begin(&self) -> Result<OnceBeginResult, SyncError> {
        Err(SyncError::unsupported())
    }

    fn wait(&self) -> Result<(), SyncError> {
        Err(SyncError::unsupported())
    }

    unsafe fn complete_unchecked(&self) {}

    unsafe fn reset_unchecked(&self) {}
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

impl UnsupportedRawRwLock {
    /// Creates a new unsupported raw rwlock placeholder.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

// SAFETY: this type never successfully acquires read or write ownership and therefore cannot
// violate rwlock semantics through its unsupported surface.
unsafe impl RawRwLock for UnsupportedRawRwLock {
    fn support(&self) -> RwLockSupport {
        RwLockSupport::unsupported()
    }

    fn read_lock(&self) -> Result<(), SyncError> {
        Err(SyncError::unsupported())
    }

    fn try_read_lock(&self) -> Result<bool, SyncError> {
        Err(SyncError::unsupported())
    }

    fn write_lock(&self) -> Result<(), SyncError> {
        Err(SyncError::unsupported())
    }

    fn try_write_lock(&self) -> Result<bool, SyncError> {
        Err(SyncError::unsupported())
    }

    unsafe fn read_unlock_unchecked(&self) {}

    unsafe fn write_unlock_unchecked(&self) {}
}
