//! Cortex-M bare-metal synchronization backend.
//!
//! This backend intentionally exposes only local, spin-based primitives. There is no kernel
//! waiter queue hiding behind the curtains, and pretending otherwise would just be a more
//! sophisticated form of lying.

use core::hint::spin_loop;
#[cfg(target_has_atomic = "8")]
use core::sync::atomic::AtomicU8;
#[cfg(target_has_atomic = "16")]
use core::sync::atomic::AtomicU16;
use core::sync::atomic::{AtomicU32, Ordering};
use core::time::Duration;

use crate::pal::sync::{
    MutexCaps, MutexSupport, OnceBeginResult, OnceCaps, OnceState, OnceSupport,
    PriorityInheritanceSupport, ProcessScopeSupport, RawMutex, RawOnce, RawRwLock, RawSemaphore,
    RecursionSupport, RobustnessSupport, RwLockCaps, RwLockFairnessSupport, RwLockSupport,
    SemaphoreCaps, SemaphoreSupport, SyncBase, SyncError, SyncFallbackKind, SyncImplementationKind,
    SyncSupport, TimeoutCaps, WaitOutcome, WaitPrimitive, WaitSupport,
};

const MUTEX_UNLOCKED: u8 = 0;
const MUTEX_LOCKED: u8 = 1;

const ONCE_UNINITIALIZED: u8 = 0;
const ONCE_RUNNING: u8 = 1;
const ONCE_COMPLETE: u8 = 2;

#[cfg(target_has_atomic = "16")]
const RWLOCK_WRITER: u16 = 0x8000;
#[cfg(target_has_atomic = "16")]
const RWLOCK_READERS_MASK: u16 = 0x7fff;

#[cfg(target_has_atomic = "8")]
const CORTEX_M_MUTEX_SUPPORT: MutexSupport = MutexSupport {
    caps: MutexCaps::TRY_LOCK
        .union(MutexCaps::BLOCKING)
        .union(MutexCaps::STATIC_INIT),
    timeout: TimeoutCaps::empty(),
    priority_inheritance: PriorityInheritanceSupport::None,
    recursion: RecursionSupport::None,
    robustness: RobustnessSupport::None,
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Emulated,
    fallback: SyncFallbackKind::SpinOnly,
};

#[cfg(not(target_has_atomic = "8"))]
const CORTEX_M_MUTEX_SUPPORT: MutexSupport = MutexSupport::unsupported();

#[cfg(target_has_atomic = "8")]
const CORTEX_M_ONCE_SUPPORT: OnceSupport = OnceSupport {
    caps: OnceCaps::WAITING
        .union(OnceCaps::STATIC_INIT)
        .union(OnceCaps::RESET_ON_FAILURE),
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Emulated,
    fallback: SyncFallbackKind::SpinOnly,
};

#[cfg(not(target_has_atomic = "8"))]
const CORTEX_M_ONCE_SUPPORT: OnceSupport = OnceSupport::unsupported();

#[cfg(target_has_atomic = "16")]
const CORTEX_M_SEMAPHORE_SUPPORT: SemaphoreSupport = SemaphoreSupport {
    caps: SemaphoreCaps::TRY_ACQUIRE
        .union(SemaphoreCaps::BLOCKING)
        .union(SemaphoreCaps::RELEASE_MANY),
    timeout: TimeoutCaps::empty(),
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Emulated,
    fallback: SyncFallbackKind::SpinOnly,
};

#[cfg(not(target_has_atomic = "16"))]
const CORTEX_M_SEMAPHORE_SUPPORT: SemaphoreSupport = SemaphoreSupport::unsupported();

#[cfg(target_has_atomic = "16")]
const CORTEX_M_RWLOCK_SUPPORT: RwLockSupport = RwLockSupport {
    caps: RwLockCaps::TRY_READ
        .union(RwLockCaps::TRY_WRITE)
        .union(RwLockCaps::BLOCKING_READ)
        .union(RwLockCaps::BLOCKING_WRITE)
        .union(RwLockCaps::STATIC_INIT),
    timeout: TimeoutCaps::empty(),
    fairness: RwLockFairnessSupport::None,
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Emulated,
    fallback: SyncFallbackKind::SpinOnly,
};

#[cfg(not(target_has_atomic = "16"))]
const CORTEX_M_RWLOCK_SUPPORT: RwLockSupport = RwLockSupport::unsupported();

/// Cortex-M synchronization provider handle.
#[derive(Debug, Clone, Copy, Default)]
pub struct CortexMSync;

/// Cortex-M raw mutex implemented with a local spin word.
#[derive(Debug)]
pub struct CortexMRawMutex {
    #[cfg(target_has_atomic = "8")]
    state: AtomicU8,
}

/// Cortex-M raw once primitive implemented with a local spin word.
#[derive(Debug)]
pub struct CortexMRawOnce {
    #[cfg(target_has_atomic = "8")]
    state: AtomicU8,
}

/// Cortex-M counting semaphore implemented with a local spin counter.
#[derive(Debug)]
pub struct CortexMSemaphore {
    #[cfg(target_has_atomic = "16")]
    permits: AtomicU16,
    #[cfg(target_has_atomic = "16")]
    max: u16,
}

/// Cortex-M raw reader/writer lock implemented with a local spin word.
#[derive(Debug)]
pub struct CortexMRawRwLock {
    #[cfg(target_has_atomic = "16")]
    state: AtomicU16,
}

/// Selected raw mutex type for Cortex-M builds.
pub type PlatformRawMutex = CortexMRawMutex;

/// Selected semaphore type for Cortex-M builds.
pub type PlatformSemaphore = CortexMSemaphore;

/// Selected raw once type for Cortex-M builds.
pub type PlatformRawOnce = CortexMRawOnce;

/// Selected raw rwlock type for Cortex-M builds.
pub type PlatformRawRwLock = CortexMRawRwLock;

/// Target-selected synchronization provider alias for Cortex-M builds.
pub type PlatformSync = CortexMSync;

/// Backend truth for the selected raw mutex implementation on Cortex-M.
#[cfg(target_has_atomic = "8")]
pub const PLATFORM_RAW_MUTEX_IMPLEMENTATION: SyncImplementationKind =
    SyncImplementationKind::Emulated;

/// Backend truth for the selected raw mutex implementation on Cortex-M.
#[cfg(not(target_has_atomic = "8"))]
pub const PLATFORM_RAW_MUTEX_IMPLEMENTATION: SyncImplementationKind =
    SyncImplementationKind::Unsupported;

/// Backend truth for the selected raw once implementation on Cortex-M.
#[cfg(target_has_atomic = "8")]
pub const PLATFORM_RAW_ONCE_IMPLEMENTATION: SyncImplementationKind =
    SyncImplementationKind::Emulated;

/// Backend truth for the selected raw once implementation on Cortex-M.
#[cfg(not(target_has_atomic = "8"))]
pub const PLATFORM_RAW_ONCE_IMPLEMENTATION: SyncImplementationKind =
    SyncImplementationKind::Unsupported;

/// Backend truth for the selected raw rwlock implementation on Cortex-M.
#[cfg(target_has_atomic = "16")]
pub const PLATFORM_RAW_RWLOCK_IMPLEMENTATION: SyncImplementationKind =
    SyncImplementationKind::Emulated;

/// Backend truth for the selected raw rwlock implementation on Cortex-M.
#[cfg(not(target_has_atomic = "16"))]
pub const PLATFORM_RAW_RWLOCK_IMPLEMENTATION: SyncImplementationKind =
    SyncImplementationKind::Unsupported;

/// Returns the process-wide Cortex-M synchronization provider handle.
#[must_use]
pub const fn system_sync() -> PlatformSync {
    PlatformSync::new()
}

impl CortexMSync {
    /// Creates a new Cortex-M synchronization provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl SyncBase for CortexMSync {
    fn support(&self) -> SyncSupport {
        SyncSupport {
            wait: WaitSupport::unsupported(),
            mutex: CORTEX_M_MUTEX_SUPPORT,
            once: CORTEX_M_ONCE_SUPPORT,
            semaphore: CORTEX_M_SEMAPHORE_SUPPORT,
            rwlock: CORTEX_M_RWLOCK_SUPPORT,
        }
    }
}

impl WaitPrimitive for CortexMSync {
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

impl CortexMRawMutex {
    /// Creates a new unlocked Cortex-M raw mutex.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            #[cfg(target_has_atomic = "8")]
            state: AtomicU8::new(MUTEX_UNLOCKED),
        }
    }
}

// SAFETY: this mutex uses an atomic ownership word with acquire/release semantics and requires
// callers to uphold balanced unlock discipline through the raw contract.
unsafe impl RawMutex for CortexMRawMutex {
    fn support(&self) -> MutexSupport {
        CORTEX_M_MUTEX_SUPPORT
    }

    fn lock(&self) -> Result<(), SyncError> {
        loop {
            if self.try_lock()? {
                return Ok(());
            }
            spin_loop();
        }
    }

    fn try_lock(&self) -> Result<bool, SyncError> {
        #[cfg(target_has_atomic = "8")]
        {
            return Ok(self
                .state
                .compare_exchange(
                    MUTEX_UNLOCKED,
                    MUTEX_LOCKED,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                )
                .is_ok());
        }

        #[cfg(not(target_has_atomic = "8"))]
        {
            Err(SyncError::unsupported())
        }
    }

    unsafe fn unlock_unchecked(&self) {
        #[cfg(target_has_atomic = "8")]
        {
            self.state.store(MUTEX_UNLOCKED, Ordering::Release);
        }
    }
}

impl CortexMRawOnce {
    /// Creates a new uninitialized Cortex-M raw once primitive.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            #[cfg(target_has_atomic = "8")]
            state: AtomicU8::new(ONCE_UNINITIALIZED),
        }
    }
}

impl RawOnce for CortexMRawOnce {
    fn support(&self) -> OnceSupport {
        CORTEX_M_ONCE_SUPPORT
    }

    fn state(&self) -> OnceState {
        #[cfg(target_has_atomic = "8")]
        {
            return match self.state.load(Ordering::Acquire) {
                ONCE_RUNNING => OnceState::Running,
                ONCE_COMPLETE => OnceState::Complete,
                _ => OnceState::Uninitialized,
            };
        }

        #[cfg(not(target_has_atomic = "8"))]
        {
            OnceState::Uninitialized
        }
    }

    fn begin(&self) -> Result<OnceBeginResult, SyncError> {
        #[cfg(target_has_atomic = "8")]
        {
            loop {
                match self.state.load(Ordering::Acquire) {
                    ONCE_COMPLETE => return Ok(OnceBeginResult::Complete),
                    ONCE_RUNNING => return Ok(OnceBeginResult::InProgress),
                    ONCE_UNINITIALIZED => {
                        if self
                            .state
                            .compare_exchange(
                                ONCE_UNINITIALIZED,
                                ONCE_RUNNING,
                                Ordering::Acquire,
                                Ordering::Relaxed,
                            )
                            .is_ok()
                        {
                            return Ok(OnceBeginResult::Initialize);
                        }
                    }
                    _ => return Err(SyncError::invalid()),
                }
            }
        }

        #[cfg(not(target_has_atomic = "8"))]
        {
            Err(SyncError::unsupported())
        }
    }

    fn wait(&self) -> Result<(), SyncError> {
        #[cfg(target_has_atomic = "8")]
        {
            while self.state.load(Ordering::Acquire) == ONCE_RUNNING {
                spin_loop();
            }
            return Ok(());
        }

        #[cfg(not(target_has_atomic = "8"))]
        {
            Err(SyncError::unsupported())
        }
    }

    unsafe fn complete_unchecked(&self) {
        #[cfg(target_has_atomic = "8")]
        {
            self.state.store(ONCE_COMPLETE, Ordering::Release);
        }
    }

    unsafe fn reset_unchecked(&self) {
        #[cfg(target_has_atomic = "8")]
        {
            self.state.store(ONCE_UNINITIALIZED, Ordering::Release);
        }
    }
}

impl CortexMSemaphore {
    /// Creates a new Cortex-M semaphore with a bounded permit range.
    ///
    /// # Errors
    ///
    /// Returns an error when the requested bounds cannot be represented honestly on the
    /// selected target.
    pub const fn new(initial: u32, max: u32) -> Result<Self, SyncError> {
        #[cfg(target_has_atomic = "16")]
        {
            if initial > max || max > u16::MAX as u32 {
                return Err(SyncError::invalid());
            }

            return Ok(Self {
                permits: AtomicU16::new(initial as u16),
                max: max as u16,
            });
        }

        #[cfg(not(target_has_atomic = "16"))]
        {
            let _ = initial;
            let _ = max;
            Err(SyncError::unsupported())
        }
    }
}

impl RawSemaphore for CortexMSemaphore {
    fn support(&self) -> SemaphoreSupport {
        CORTEX_M_SEMAPHORE_SUPPORT
    }

    fn acquire(&self) -> Result<(), SyncError> {
        loop {
            if self.try_acquire()? {
                return Ok(());
            }
            spin_loop();
        }
    }

    fn try_acquire(&self) -> Result<bool, SyncError> {
        #[cfg(target_has_atomic = "16")]
        {
            loop {
                let permits = self.permits.load(Ordering::Acquire);
                if permits == 0 {
                    return Ok(false);
                }

                if self
                    .permits
                    .compare_exchange(permits, permits - 1, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    return Ok(true);
                }
            }
        }

        #[cfg(not(target_has_atomic = "16"))]
        {
            Err(SyncError::unsupported())
        }
    }

    fn release(&self, permits: u32) -> Result<(), SyncError> {
        #[cfg(target_has_atomic = "16")]
        {
            if permits > u16::MAX as u32 {
                return Err(SyncError::overflow());
            }

            let permits = permits as u16;
            loop {
                let current = self.permits.load(Ordering::Acquire);
                let next = current
                    .checked_add(permits)
                    .filter(|next| *next <= self.max)
                    .ok_or_else(SyncError::overflow)?;

                if self
                    .permits
                    .compare_exchange(current, next, Ordering::Release, Ordering::Relaxed)
                    .is_ok()
                {
                    return Ok(());
                }
            }
        }

        #[cfg(not(target_has_atomic = "16"))]
        {
            let _ = permits;
            Err(SyncError::unsupported())
        }
    }

    fn max_permits(&self) -> u32 {
        #[cfg(target_has_atomic = "16")]
        {
            return u32::from(self.max);
        }

        #[cfg(not(target_has_atomic = "16"))]
        {
            0
        }
    }
}

impl CortexMRawRwLock {
    /// Creates a new unlocked Cortex-M raw rwlock.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            #[cfg(target_has_atomic = "16")]
            state: AtomicU16::new(0),
        }
    }
}

// SAFETY: this rwlock uses an atomic state word to serialize readers and writers with the raw
// acquire/release semantics required by the contract.
unsafe impl RawRwLock for CortexMRawRwLock {
    fn support(&self) -> RwLockSupport {
        CORTEX_M_RWLOCK_SUPPORT
    }

    fn read_lock(&self) -> Result<(), SyncError> {
        loop {
            if self.try_read_lock()? {
                return Ok(());
            }
            spin_loop();
        }
    }

    fn try_read_lock(&self) -> Result<bool, SyncError> {
        #[cfg(target_has_atomic = "16")]
        {
            loop {
                let state = self.state.load(Ordering::Acquire);
                if state & RWLOCK_WRITER != 0 {
                    return Ok(false);
                }

                let readers = state & RWLOCK_READERS_MASK;
                if readers == RWLOCK_READERS_MASK {
                    return Err(SyncError::overflow());
                }

                if self
                    .state
                    .compare_exchange(state, state + 1, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
                {
                    return Ok(true);
                }
            }
        }

        #[cfg(not(target_has_atomic = "16"))]
        {
            Err(SyncError::unsupported())
        }
    }

    fn write_lock(&self) -> Result<(), SyncError> {
        loop {
            if self.try_write_lock()? {
                return Ok(());
            }
            spin_loop();
        }
    }

    fn try_write_lock(&self) -> Result<bool, SyncError> {
        #[cfg(target_has_atomic = "16")]
        {
            return Ok(self
                .state
                .compare_exchange(0, RWLOCK_WRITER, Ordering::Acquire, Ordering::Relaxed)
                .is_ok());
        }

        #[cfg(not(target_has_atomic = "16"))]
        {
            Err(SyncError::unsupported())
        }
    }

    unsafe fn read_unlock_unchecked(&self) {
        #[cfg(target_has_atomic = "16")]
        {
            self.state.fetch_sub(1, Ordering::Release);
        }
    }

    unsafe fn write_unlock_unchecked(&self) {
        #[cfg(target_has_atomic = "16")]
        {
            self.state.store(0, Ordering::Release);
        }
    }
}
