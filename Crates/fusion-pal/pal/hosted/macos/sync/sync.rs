//! macOS fusion-pal synchronization backend.
//!
//! This backend exposes native pthread mutex/rwlock primitives. Additional primitives are
//! surfaced conservatively with explicit implementation-kind truth.

use core::cell::UnsafeCell;
#[cfg(not(feature = "critical-safe"))]
use core::sync::atomic::{
    AtomicU32,
    Ordering,
};

#[cfg(feature = "critical-safe")]
use crate::contract::pal::runtime::sync::UnsupportedRawOnce;
#[cfg(feature = "critical-safe")]
use crate::contract::pal::runtime::sync::UnsupportedSemaphore;
use crate::contract::pal::runtime::sync::{
    MutexCaps,
    MutexSupport,
    OnceSupport,
    PriorityInheritanceSupport,
    ProcessScopeSupport,
    RawMutex,
    RawRwLock,
    RecursionSupport,
    RobustnessSupport,
    RwLockCaps,
    RwLockFairnessSupport,
    RwLockSupport,
    SemaphoreSupport,
    SyncBase,
    SyncError,
    SyncErrorKind,
    SyncFallbackKind,
    SyncImplementationKind,
    SyncSupport,
    TimeoutCaps,
};
#[cfg(not(feature = "critical-safe"))]
use crate::contract::pal::runtime::sync::{
    OnceBeginResult,
    OnceCaps,
    OnceState,
    RawOnce,
    RawSemaphore,
    SemaphoreCaps,
};
use crate::pal::hosted::macos::capability::{
    DarwinRuntimeCapabilities,
    runtime_capabilities as darwin_runtime_capabilities,
};

const MACOS_MUTEX_SUPPORT: MutexSupport = MutexSupport {
    caps: MutexCaps::TRY_LOCK
        .union(MutexCaps::BLOCKING)
        .union(MutexCaps::STATIC_INIT),
    timeout: TimeoutCaps::empty(),
    priority_inheritance: PriorityInheritanceSupport::None,
    recursion: RecursionSupport::None,
    robustness: RobustnessSupport::None,
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Native,
    fallback: SyncFallbackKind::None,
};

const MACOS_RWLOCK_SUPPORT: RwLockSupport = RwLockSupport {
    caps: RwLockCaps::TRY_READ
        .union(RwLockCaps::TRY_WRITE)
        .union(RwLockCaps::BLOCKING_READ)
        .union(RwLockCaps::BLOCKING_WRITE)
        .union(RwLockCaps::STATIC_INIT),
    timeout: TimeoutCaps::empty(),
    fairness: RwLockFairnessSupport::None,
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Native,
    fallback: SyncFallbackKind::None,
};

#[cfg(not(feature = "critical-safe"))]
const MACOS_ONCE_SUPPORT: OnceSupport = OnceSupport {
    caps: OnceCaps::WAITING
        .union(OnceCaps::STATIC_INIT)
        .union(OnceCaps::RESET_ON_FAILURE),
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Emulated,
    fallback: SyncFallbackKind::None,
};

#[cfg(feature = "critical-safe")]
const MACOS_ONCE_SUPPORT: OnceSupport = OnceSupport::unsupported();

#[cfg(not(feature = "critical-safe"))]
const MACOS_SEMAPHORE_SUPPORT: SemaphoreSupport = SemaphoreSupport {
    caps: SemaphoreCaps::TRY_ACQUIRE
        .union(SemaphoreCaps::BLOCKING)
        .union(SemaphoreCaps::RELEASE_MANY),
    timeout: TimeoutCaps::empty(),
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Emulated,
    fallback: SyncFallbackKind::None,
};

#[cfg(feature = "critical-safe")]
const MACOS_SEMAPHORE_SUPPORT: SemaphoreSupport = SemaphoreSupport::unsupported();

#[cfg(not(feature = "critical-safe"))]
const ONCE_UNINITIALIZED: u32 = 0;
#[cfg(not(feature = "critical-safe"))]
const ONCE_RUNNING: u32 = 1;
#[cfg(not(feature = "critical-safe"))]
const ONCE_COMPLETE: u32 = 2;

/// macOS synchronization provider handle.
#[derive(Debug, Clone, Copy, Default)]
pub struct MacOsSync;

/// Selected raw mutex type for macOS builds.
pub type PlatformRawMutex = MacOsRawMutex;

#[cfg(not(feature = "critical-safe"))]
/// Selected semaphore type for macOS builds.
pub type PlatformSemaphore = MacOsSemaphore;
#[cfg(feature = "critical-safe")]
/// Selected semaphore type for macOS builds.
pub type PlatformSemaphore = UnsupportedSemaphore;

#[cfg(not(feature = "critical-safe"))]
/// Selected raw once type for macOS builds.
pub type PlatformRawOnce = MacOsRawOnce;
#[cfg(feature = "critical-safe")]
/// Selected raw once type for macOS builds.
pub type PlatformRawOnce = UnsupportedRawOnce;

/// Selected raw rwlock type for macOS builds.
pub type PlatformRawRwLock = MacOsRawRwLock;

/// Target-selected synchronization provider alias for macOS builds.
pub type PlatformSync = MacOsSync;

/// Backend truth for the selected raw mutex implementation on macOS.
pub const PLATFORM_RAW_MUTEX_IMPLEMENTATION: SyncImplementationKind =
    SyncImplementationKind::Native;

#[cfg(not(feature = "critical-safe"))]
/// Backend truth for the selected raw once implementation on macOS.
pub const PLATFORM_RAW_ONCE_IMPLEMENTATION: SyncImplementationKind =
    SyncImplementationKind::Emulated;
#[cfg(feature = "critical-safe")]
/// Backend truth for the selected raw once implementation on macOS.
pub const PLATFORM_RAW_ONCE_IMPLEMENTATION: SyncImplementationKind =
    SyncImplementationKind::Unsupported;

/// Backend truth for the selected raw rwlock implementation on macOS.
pub const PLATFORM_RAW_RWLOCK_IMPLEMENTATION: SyncImplementationKind =
    SyncImplementationKind::Native;

/// Returns the process-wide macOS synchronization provider handle.
#[must_use]
pub const fn system_sync() -> PlatformSync {
    PlatformSync::new()
}

/// Returns the cached Darwin runtime capability snapshot used by the macOS sync backend.
#[must_use]
pub fn runtime_sync_capabilities() -> DarwinRuntimeCapabilities {
    darwin_runtime_capabilities()
}

impl MacOsSync {
    /// Creates a new macOS synchronization provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl SyncBase for MacOsSync {
    fn support(&self) -> SyncSupport {
        SyncSupport {
            mutex: MACOS_MUTEX_SUPPORT,
            once: MACOS_ONCE_SUPPORT,
            semaphore: MACOS_SEMAPHORE_SUPPORT,
            rwlock: MACOS_RWLOCK_SUPPORT,
        }
    }
}

/// macOS raw mutex backed by `pthread_mutex_t`.
#[derive(Debug)]
pub struct MacOsRawMutex {
    inner: UnsafeCell<libc::pthread_mutex_t>,
}

impl MacOsRawMutex {
    /// Creates a new unlocked macOS raw mutex.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: UnsafeCell::new(libc::PTHREAD_MUTEX_INITIALIZER),
        }
    }

    fn as_mut_ptr(&self) -> *mut libc::pthread_mutex_t {
        self.inner.get()
    }
}

impl Default for MacOsRawMutex {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for MacOsRawMutex {
    fn drop(&mut self) {
        let rc = unsafe { libc::pthread_mutex_destroy(self.inner.get()) };
        debug_assert!(rc == 0 || rc == libc::EBUSY || rc == libc::EINVAL);
    }
}

// SAFETY: pthread mutex operations enforce mutual exclusion with acquire/release semantics.
unsafe impl RawMutex for MacOsRawMutex {
    fn support(&self) -> MutexSupport {
        MACOS_MUTEX_SUPPORT
    }

    fn lock(&self) -> Result<(), SyncError> {
        let rc = unsafe { libc::pthread_mutex_lock(self.as_mut_ptr()) };
        if rc == 0 { Ok(()) } else { Err(map_errno(rc)) }
    }

    fn try_lock(&self) -> Result<bool, SyncError> {
        let rc = unsafe { libc::pthread_mutex_trylock(self.as_mut_ptr()) };
        if rc == 0 {
            Ok(true)
        } else if rc == libc::EBUSY {
            Ok(false)
        } else {
            Err(map_errno(rc))
        }
    }

    unsafe fn unlock_unchecked(&self) {
        let rc = unsafe { libc::pthread_mutex_unlock(self.as_mut_ptr()) };
        debug_assert!(rc == 0 || rc == libc::EPERM || rc == libc::EINVAL);
    }
}

/// macOS condition variable backed by `pthread_cond_t`.
#[cfg(not(feature = "critical-safe"))]
#[derive(Debug)]
struct MacOsCondVar {
    inner: UnsafeCell<libc::pthread_cond_t>,
}

#[cfg(not(feature = "critical-safe"))]
impl MacOsCondVar {
    const fn new() -> Self {
        Self {
            inner: UnsafeCell::new(libc::PTHREAD_COND_INITIALIZER),
        }
    }

    fn as_mut_ptr(&self) -> *mut libc::pthread_cond_t {
        self.inner.get()
    }

    fn wait(&self, mutex: &MacOsRawMutex) -> Result<(), SyncError> {
        let rc = unsafe { libc::pthread_cond_wait(self.as_mut_ptr(), mutex.as_mut_ptr()) };
        if rc == 0 { Ok(()) } else { Err(map_errno(rc)) }
    }

    fn notify_one(&self) {
        let rc = unsafe { libc::pthread_cond_signal(self.as_mut_ptr()) };
        debug_assert!(rc == 0 || rc == libc::EINVAL);
    }

    fn notify_all(&self) {
        let rc = unsafe { libc::pthread_cond_broadcast(self.as_mut_ptr()) };
        debug_assert!(rc == 0 || rc == libc::EINVAL);
    }
}

#[cfg(not(feature = "critical-safe"))]
impl Drop for MacOsCondVar {
    fn drop(&mut self) {
        let rc = unsafe { libc::pthread_cond_destroy(self.inner.get()) };
        debug_assert!(rc == 0 || rc == libc::EBUSY || rc == libc::EINVAL);
    }
}

#[cfg(not(feature = "critical-safe"))]
// SAFETY: access is coordinated by external synchronization through associated mutexes.
unsafe impl Send for MacOsCondVar {}
#[cfg(not(feature = "critical-safe"))]
// SAFETY: shared references only call pthread synchronization operations.
unsafe impl Sync for MacOsCondVar {}

#[cfg(not(feature = "critical-safe"))]
/// macOS raw once primitive emulated with mutex + condvar + state word.
#[derive(Debug)]
pub struct MacOsRawOnce {
    gate: MacOsRawMutex,
    ready: MacOsCondVar,
    state: AtomicU32,
}

#[cfg(not(feature = "critical-safe"))]
impl MacOsRawOnce {
    /// Creates a new uninitialized macOS raw once primitive.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            gate: MacOsRawMutex::new(),
            ready: MacOsCondVar::new(),
            state: AtomicU32::new(ONCE_UNINITIALIZED),
        }
    }
}

#[cfg(not(feature = "critical-safe"))]
impl Default for MacOsRawOnce {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(feature = "critical-safe"))]
impl RawOnce for MacOsRawOnce {
    fn support(&self) -> OnceSupport {
        MACOS_ONCE_SUPPORT
    }

    fn state(&self) -> OnceState {
        match self.state.load(Ordering::Acquire) {
            ONCE_RUNNING => OnceState::Running,
            ONCE_COMPLETE => OnceState::Complete,
            _ => OnceState::Uninitialized,
        }
    }

    fn begin(&self) -> Result<OnceBeginResult, SyncError> {
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

    fn wait(&self) -> Result<(), SyncError> {
        self.gate.lock()?;
        while self.state.load(Ordering::Acquire) == ONCE_RUNNING {
            self.ready.wait(&self.gate)?;
        }
        // SAFETY: this thread acquired `gate` above and still holds it.
        unsafe { self.gate.unlock_unchecked() };
        Ok(())
    }

    unsafe fn complete_unchecked(&self) {
        self.state.store(ONCE_COMPLETE, Ordering::Release);
        self.ready.notify_all();
    }

    unsafe fn reset_unchecked(&self) {
        self.state.store(ONCE_UNINITIALIZED, Ordering::Release);
        self.ready.notify_all();
    }
}

#[cfg(not(feature = "critical-safe"))]
/// macOS local counting semaphore emulated with mutex + condvar + permit word.
#[derive(Debug)]
pub struct MacOsSemaphore {
    gate: MacOsRawMutex,
    ready: MacOsCondVar,
    permits: AtomicU32,
    max: u32,
}

#[cfg(not(feature = "critical-safe"))]
impl MacOsSemaphore {
    /// Creates a new macOS semaphore with the given initial and maximum permit counts.
    pub fn new(initial: u32, max: u32) -> Result<Self, SyncError> {
        if max == 0 || initial > max {
            return Err(SyncError::invalid());
        }

        Ok(Self {
            gate: MacOsRawMutex::new(),
            ready: MacOsCondVar::new(),
            permits: AtomicU32::new(initial),
            max,
        })
    }
}

#[cfg(not(feature = "critical-safe"))]
impl RawSemaphore for MacOsSemaphore {
    fn support(&self) -> SemaphoreSupport {
        MACOS_SEMAPHORE_SUPPORT
    }

    fn acquire(&self) -> Result<(), SyncError> {
        loop {
            let current = self.permits.load(Ordering::Acquire);
            if current != 0 {
                if self
                    .permits
                    .compare_exchange_weak(
                        current,
                        current - 1,
                        Ordering::AcqRel,
                        Ordering::Relaxed,
                    )
                    .is_ok()
                {
                    return Ok(());
                }
                continue;
            }

            self.gate.lock()?;
            while self.permits.load(Ordering::Acquire) == 0 {
                self.ready.wait(&self.gate)?;
            }
            // SAFETY: this thread acquired `gate` above and still holds it.
            unsafe { self.gate.unlock_unchecked() };
        }
    }

    fn try_acquire(&self) -> Result<bool, SyncError> {
        let mut current = self.permits.load(Ordering::Acquire);
        while current != 0 {
            match self.permits.compare_exchange_weak(
                current,
                current - 1,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(true),
                Err(observed) => current = observed,
            }
        }

        Ok(false)
    }

    fn release(&self, permits: u32) -> Result<(), SyncError> {
        if permits == 0 {
            return Err(SyncError::invalid());
        }

        self.gate.lock()?;
        let current = self.permits.load(Ordering::Acquire);
        let next = current
            .checked_add(permits)
            .ok_or_else(SyncError::overflow)?;
        if next > self.max {
            // SAFETY: this thread acquired `gate` above and still holds it.
            unsafe { self.gate.unlock_unchecked() };
            return Err(SyncError::overflow());
        }

        self.permits.store(next, Ordering::Release);
        // SAFETY: this thread acquired `gate` above and still holds it.
        unsafe { self.gate.unlock_unchecked() };
        if permits == 1 {
            self.ready.notify_one();
        } else {
            self.ready.notify_all();
        }
        Ok(())
    }

    fn max_permits(&self) -> u32 {
        self.max
    }
}

/// macOS raw rwlock backed by `pthread_rwlock_t`.
#[derive(Debug)]
pub struct MacOsRawRwLock {
    inner: UnsafeCell<libc::pthread_rwlock_t>,
}

impl MacOsRawRwLock {
    /// Creates a new unlocked macOS raw rwlock.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: UnsafeCell::new(libc::PTHREAD_RWLOCK_INITIALIZER),
        }
    }

    fn as_mut_ptr(&self) -> *mut libc::pthread_rwlock_t {
        self.inner.get()
    }
}

impl Default for MacOsRawRwLock {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for MacOsRawRwLock {
    fn drop(&mut self) {
        let rc = unsafe { libc::pthread_rwlock_destroy(self.inner.get()) };
        debug_assert!(rc == 0 || rc == libc::EBUSY || rc == libc::EINVAL);
    }
}

// SAFETY: pthread rwlock operations enforce shared/exclusive ownership and publication.
unsafe impl RawRwLock for MacOsRawRwLock {
    fn support(&self) -> RwLockSupport {
        MACOS_RWLOCK_SUPPORT
    }

    fn read_lock(&self) -> Result<(), SyncError> {
        let rc = unsafe { libc::pthread_rwlock_rdlock(self.as_mut_ptr()) };
        if rc == 0 { Ok(()) } else { Err(map_errno(rc)) }
    }

    fn try_read_lock(&self) -> Result<bool, SyncError> {
        let rc = unsafe { libc::pthread_rwlock_tryrdlock(self.as_mut_ptr()) };
        if rc == 0 {
            Ok(true)
        } else if rc == libc::EBUSY {
            Ok(false)
        } else {
            Err(map_errno(rc))
        }
    }

    fn write_lock(&self) -> Result<(), SyncError> {
        let rc = unsafe { libc::pthread_rwlock_wrlock(self.as_mut_ptr()) };
        if rc == 0 { Ok(()) } else { Err(map_errno(rc)) }
    }

    fn try_write_lock(&self) -> Result<bool, SyncError> {
        let rc = unsafe { libc::pthread_rwlock_trywrlock(self.as_mut_ptr()) };
        if rc == 0 {
            Ok(true)
        } else if rc == libc::EBUSY {
            Ok(false)
        } else {
            Err(map_errno(rc))
        }
    }

    unsafe fn read_unlock_unchecked(&self) {
        let rc = unsafe { libc::pthread_rwlock_unlock(self.as_mut_ptr()) };
        debug_assert!(rc == 0 || rc == libc::EPERM || rc == libc::EINVAL);
    }

    unsafe fn write_unlock_unchecked(&self) {
        let rc = unsafe { libc::pthread_rwlock_unlock(self.as_mut_ptr()) };
        debug_assert!(rc == 0 || rc == libc::EPERM || rc == libc::EINVAL);
    }
}

// SAFETY: the mutex value is only accessed through pthread APIs that provide synchronization.
unsafe impl Send for MacOsRawMutex {}
// SAFETY: shared references synchronize through the underlying pthread mutex.
unsafe impl Sync for MacOsRawMutex {}

// SAFETY: the rwlock value is only accessed through pthread APIs that provide synchronization.
unsafe impl Send for MacOsRawRwLock {}
// SAFETY: shared references synchronize through the underlying pthread rwlock.
unsafe impl Sync for MacOsRawRwLock {}

#[cfg(not(feature = "critical-safe"))]
// SAFETY: state transitions are synchronized via atomics + mutex/condvar.
unsafe impl Send for MacOsRawOnce {}
#[cfg(not(feature = "critical-safe"))]
// SAFETY: shared references are synchronized internally.
unsafe impl Sync for MacOsRawOnce {}

#[cfg(not(feature = "critical-safe"))]
// SAFETY: state transitions are synchronized via atomics + mutex/condvar.
unsafe impl Send for MacOsSemaphore {}
#[cfg(not(feature = "critical-safe"))]
// SAFETY: shared references are synchronized internally.
unsafe impl Sync for MacOsSemaphore {}

const fn map_errno(code: i32) -> SyncError {
    match code {
        libc::EINVAL => SyncError::invalid(),
        libc::EBUSY | libc::EAGAIN | libc::EDEADLK => SyncError::busy(),
        libc::EPERM | libc::EACCES => SyncError::permission_denied(),
        libc::EOPNOTSUPP | libc::ENOTSUP => SyncError::unsupported(),
        _ => SyncError {
            kind: SyncErrorKind::Platform(code),
        },
    }
}

#[cfg(all(test, feature = "std", not(target_os = "none")))]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn macos_sync_support_reports_native_mutex_and_rwlock() {
        let support = system_sync().support();
        assert_eq!(support.mutex.implementation, SyncImplementationKind::Native);
        assert_eq!(
            support.rwlock.implementation,
            SyncImplementationKind::Native
        );
    }

    #[test]
    fn macos_sync_support_matches_feature_policy_for_once_and_semaphore() {
        let support = system_sync().support();

        #[cfg(feature = "critical-safe")]
        {
            assert_eq!(
                support.once.implementation,
                SyncImplementationKind::Unsupported
            );
            assert_eq!(
                support.semaphore.implementation,
                SyncImplementationKind::Unsupported
            );
        }

        #[cfg(not(feature = "critical-safe"))]
        {
            assert_eq!(
                support.once.implementation,
                SyncImplementationKind::Emulated
            );
            assert_eq!(
                support.semaphore.implementation,
                SyncImplementationKind::Emulated
            );
        }
    }

    #[cfg(not(feature = "critical-safe"))]
    #[test]
    fn macos_emulated_semaphore_tracks_permits() {
        let semaphore = MacOsSemaphore::new(1, 2).expect("valid semaphore");
        assert!(
            semaphore
                .try_acquire()
                .expect("first acquire should succeed")
        );
        assert!(
            !semaphore
                .try_acquire()
                .expect("second acquire should observe empty permits")
        );
        semaphore
            .release(1)
            .expect("release should restore one permit");
        assert!(
            semaphore
                .try_acquire()
                .expect("permit should be available again")
        );
    }
}
