//! Windows fusion-pal synchronization backend.
//!
//! This backend exposes native SRW locks for mutex and rwlock semantics plus native Win32
//! semaphores. One-time initialization is composed from native Windows synchronization
//! primitives and is therefore withheld under `critical-safe`, matching the conservative hosted
//! policy used elsewhere in the PAL.

use core::cell::UnsafeCell;
#[cfg(not(feature = "critical-safe"))]
use core::sync::atomic::{
    AtomicU32,
    Ordering,
};
use core::time::Duration;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle,
    ERROR_ACCESS_DENIED,
    ERROR_ALREADY_EXISTS,
    ERROR_BUSY,
    ERROR_INVALID_PARAMETER,
    ERROR_NOT_ENOUGH_MEMORY,
    ERROR_NOT_SUPPORTED,
    ERROR_OUTOFMEMORY,
    ERROR_TIMEOUT,
    GetLastError,
    HANDLE,
    WAIT_ABANDONED,
    WAIT_FAILED,
    WAIT_OBJECT_0,
    WAIT_TIMEOUT,
    WIN32_ERROR,
};
#[cfg(feature = "critical-safe")]
use crate::contract::pal::runtime::sync::UnsupportedRawOnce;
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
    SyncBaseContract,
    SyncError,
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
    RawOnceContract,
};
use crate::contract::pal::runtime::sync::RawSemaphoreContract;
#[cfg(not(feature = "critical-safe"))]
use windows::Win32::System::Threading::{
    CONDITION_VARIABLE,
    CONDITION_VARIABLE_INIT,
    SleepConditionVariableSRW,
    WakeAllConditionVariable,
};
use windows::Win32::System::Threading::{
    AcquireSRWLockExclusive,
    AcquireSRWLockShared,
    CreateSemaphoreW,
    INFINITE,
    ReleaseSRWLockExclusive,
    ReleaseSRWLockShared,
    ReleaseSemaphore,
    SRWLOCK,
    SRWLOCK_INIT,
    TryAcquireSRWLockExclusive,
    TryAcquireSRWLockShared,
    WaitForSingleObject,
};

const WINDOWS_MUTEX_SUPPORT: MutexSupport = MutexSupport {
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

const WINDOWS_RWLOCK_SUPPORT: RwLockSupport = RwLockSupport {
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
const WINDOWS_ONCE_SUPPORT: OnceSupport = OnceSupport {
    caps: OnceCaps::WAITING
        .union(OnceCaps::STATIC_INIT)
        .union(OnceCaps::RESET_ON_FAILURE),
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Emulated,
    fallback: SyncFallbackKind::None,
};

#[cfg(feature = "critical-safe")]
const WINDOWS_ONCE_SUPPORT: OnceSupport = OnceSupport::unsupported();

const WINDOWS_SEMAPHORE_SUPPORT: SemaphoreSupport = SemaphoreSupport {
    caps: crate::contract::pal::runtime::sync::SemaphoreCaps::TRY_ACQUIRE
        .union(crate::contract::pal::runtime::sync::SemaphoreCaps::BLOCKING)
        .union(crate::contract::pal::runtime::sync::SemaphoreCaps::ACQUIRE_FOR)
        .union(crate::contract::pal::runtime::sync::SemaphoreCaps::RELEASE_MANY),
    timeout: TimeoutCaps::RELATIVE,
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Native,
    fallback: SyncFallbackKind::None,
};

#[cfg(not(feature = "critical-safe"))]
const ONCE_UNINITIALIZED: u32 = 0;
#[cfg(not(feature = "critical-safe"))]
const ONCE_RUNNING: u32 = 1;
#[cfg(not(feature = "critical-safe"))]
const ONCE_COMPLETE: u32 = 2;

/// Windows synchronization provider handle.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsSync;

/// Selected raw mutex type for Windows builds.
pub type PlatformRawMutex = WindowsRawMutex;

/// Selected semaphore type for Windows builds.
pub type PlatformSemaphore = WindowsSemaphore;

#[cfg(not(feature = "critical-safe"))]
/// Selected raw once type for Windows builds.
pub type PlatformRawOnce = WindowsRawOnce;
#[cfg(feature = "critical-safe")]
/// Selected raw once type for Windows builds.
pub type PlatformRawOnce = UnsupportedRawOnce;

/// Selected raw rwlock type for Windows builds.
pub type PlatformRawRwLock = WindowsRawRwLock;

/// Target-selected synchronization provider alias for Windows builds.
pub type PlatformSync = WindowsSync;

/// Backend truth for the selected raw mutex implementation on Windows.
pub const PLATFORM_RAW_MUTEX_IMPLEMENTATION: SyncImplementationKind =
    SyncImplementationKind::Native;

#[cfg(not(feature = "critical-safe"))]
/// Backend truth for the selected raw once implementation on Windows.
pub const PLATFORM_RAW_ONCE_IMPLEMENTATION: SyncImplementationKind =
    SyncImplementationKind::Emulated;
#[cfg(feature = "critical-safe")]
/// Backend truth for the selected raw once implementation on Windows.
pub const PLATFORM_RAW_ONCE_IMPLEMENTATION: SyncImplementationKind =
    SyncImplementationKind::Unsupported;

/// Backend truth for the selected raw rwlock implementation on Windows.
pub const PLATFORM_RAW_RWLOCK_IMPLEMENTATION: SyncImplementationKind =
    SyncImplementationKind::Native;

/// Returns the process-wide Windows synchronization provider handle.
#[must_use]
pub const fn system_sync() -> PlatformSync {
    PlatformSync::new()
}

impl WindowsSync {
    /// Creates a new Windows synchronization provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl SyncBaseContract for WindowsSync {
    fn support(&self) -> SyncSupport {
        SyncSupport {
            mutex: WINDOWS_MUTEX_SUPPORT,
            once: WINDOWS_ONCE_SUPPORT,
            semaphore: WINDOWS_SEMAPHORE_SUPPORT,
            rwlock: WINDOWS_RWLOCK_SUPPORT,
        }
    }
}

/// Windows raw mutex backed by an SRW lock in exclusive mode.
#[derive(Debug)]
pub struct WindowsRawMutex {
    inner: UnsafeCell<SRWLOCK>,
}

impl WindowsRawMutex {
    /// Creates a new unlocked Windows raw mutex.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: UnsafeCell::new(SRWLOCK_INIT),
        }
    }

    fn as_mut_ptr(&self) -> *mut SRWLOCK {
        self.inner.get()
    }
}

impl Default for WindowsRawMutex {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: SRW lock exclusive operations enforce mutual exclusion with acquire/release semantics.
unsafe impl RawMutex for WindowsRawMutex {
    fn support(&self) -> MutexSupport {
        WINDOWS_MUTEX_SUPPORT
    }

    fn lock(&self) -> Result<(), SyncError> {
        unsafe { AcquireSRWLockExclusive(self.as_mut_ptr()) };
        Ok(())
    }

    fn try_lock(&self) -> Result<bool, SyncError> {
        Ok(unsafe { TryAcquireSRWLockExclusive(self.as_mut_ptr()) })
    }

    unsafe fn unlock_unchecked(&self) {
        unsafe { ReleaseSRWLockExclusive(self.as_mut_ptr()) };
    }
}

// SAFETY: the lock state is synchronized solely through SRW lock operations.
unsafe impl Send for WindowsRawMutex {}
// SAFETY: shared references synchronize through the underlying SRW lock.
unsafe impl Sync for WindowsRawMutex {}

#[cfg(not(feature = "critical-safe"))]
/// Windows raw once primitive composed from SRW lock + condition variable + state word.
#[derive(Debug)]
pub struct WindowsRawOnce {
    gate: UnsafeCell<SRWLOCK>,
    ready: UnsafeCell<CONDITION_VARIABLE>,
    state: AtomicU32,
}

#[cfg(not(feature = "critical-safe"))]
impl WindowsRawOnce {
    /// Creates a new uninitialized Windows raw once primitive.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            gate: UnsafeCell::new(SRWLOCK_INIT),
            ready: UnsafeCell::new(CONDITION_VARIABLE_INIT),
            state: AtomicU32::new(ONCE_UNINITIALIZED),
        }
    }

    fn gate(&self) -> *mut SRWLOCK {
        self.gate.get()
    }

    fn ready(&self) -> *mut CONDITION_VARIABLE {
        self.ready.get()
    }
}

#[cfg(not(feature = "critical-safe"))]
impl Default for WindowsRawOnce {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(feature = "critical-safe"))]
impl RawOnceContract for WindowsRawOnce {
    fn support(&self) -> OnceSupport {
        WINDOWS_ONCE_SUPPORT
    }

    fn state(&self) -> OnceState {
        match self.state.load(Ordering::Acquire) {
            ONCE_RUNNING => OnceState::Running,
            ONCE_COMPLETE => OnceState::Complete,
            _ => OnceState::Uninitialized,
        }
    }

    fn begin(&self) -> Result<OnceBeginResult, SyncError> {
        unsafe { AcquireSRWLockExclusive(self.gate()) };
        let result = match self.state.load(Ordering::Acquire) {
            ONCE_COMPLETE => Ok(OnceBeginResult::Complete),
            ONCE_RUNNING => Ok(OnceBeginResult::InProgress),
            ONCE_UNINITIALIZED => {
                self.state.store(ONCE_RUNNING, Ordering::Release);
                Ok(OnceBeginResult::Initialize)
            }
            _ => Err(SyncError::invalid()),
        };
        unsafe { ReleaseSRWLockExclusive(self.gate()) };
        result
    }

    fn wait(&self) -> Result<(), SyncError> {
        unsafe { AcquireSRWLockExclusive(self.gate()) };
        while self.state.load(Ordering::Acquire) == ONCE_RUNNING {
            if unsafe { SleepConditionVariableSRW(self.ready(), self.gate(), INFINITE, 0) }.is_err()
            {
                let error = map_win32_error(unsafe { GetLastError() });
                unsafe { ReleaseSRWLockExclusive(self.gate()) };
                return Err(error);
            }
        }
        unsafe { ReleaseSRWLockExclusive(self.gate()) };
        Ok(())
    }

    unsafe fn complete_unchecked(&self) {
        unsafe { AcquireSRWLockExclusive(self.gate()) };
        self.state.store(ONCE_COMPLETE, Ordering::Release);
        unsafe {
            WakeAllConditionVariable(self.ready());
            ReleaseSRWLockExclusive(self.gate());
        }
    }

    unsafe fn reset_unchecked(&self) {
        unsafe { AcquireSRWLockExclusive(self.gate()) };
        self.state.store(ONCE_UNINITIALIZED, Ordering::Release);
        unsafe {
            WakeAllConditionVariable(self.ready());
            ReleaseSRWLockExclusive(self.gate());
        }
    }
}

#[cfg(not(feature = "critical-safe"))]
// SAFETY: access is synchronized by the gate SRW lock.
unsafe impl Send for WindowsRawOnce {}
#[cfg(not(feature = "critical-safe"))]
// SAFETY: shared references operate only through synchronized state transitions.
unsafe impl Sync for WindowsRawOnce {}

/// Windows local counting semaphore backed by a kernel semaphore handle.
#[derive(Debug)]
pub struct WindowsSemaphore {
    handle: HANDLE,
    max: u32,
}

impl WindowsSemaphore {
    /// Creates a new Windows semaphore with the given initial and maximum permit counts.
    ///
    /// # Errors
    ///
    /// Returns an error if the permit bounds are invalid or the OS cannot create the semaphore.
    pub fn new(initial: u32, max: u32) -> Result<Self, SyncError> {
        if initial > max || max == 0 {
            return Err(SyncError::invalid());
        }

        let initial = i32::try_from(initial).map_err(|_| SyncError::overflow())?;
        let max_i32 = i32::try_from(max).map_err(|_| SyncError::overflow())?;
        let handle = unsafe { CreateSemaphoreW(None, initial, max_i32, PCWSTR::null()) }
            .map_err(|_| map_win32_error(unsafe { GetLastError() }))?;

        Ok(Self { handle, max })
    }

    fn wait(&self, timeout_ms: u32) -> Result<bool, SyncError> {
        match unsafe { WaitForSingleObject(self.handle, timeout_ms) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            WAIT_FAILED => Err(map_win32_error(unsafe { GetLastError() })),
            WAIT_ABANDONED => Err(SyncError::platform(WAIT_ABANDONED.0 as i32)),
            other => Err(SyncError::platform(other.0 as i32)),
        }
    }
}

impl Drop for WindowsSemaphore {
    fn drop(&mut self) {
        if !self.handle.is_invalid() && !self.handle.0.is_null() {
            let rc = unsafe { CloseHandle(self.handle) };
            debug_assert!(rc.is_ok());
        }
    }
}

impl RawSemaphoreContract for WindowsSemaphore {
    fn support(&self) -> SemaphoreSupport {
        WINDOWS_SEMAPHORE_SUPPORT
    }

    fn acquire(&self) -> Result<(), SyncError> {
        match self.wait(INFINITE)? {
            true => Ok(()),
            false => Err(SyncError::platform(WAIT_TIMEOUT.0 as i32)),
        }
    }

    fn try_acquire(&self) -> Result<bool, SyncError> {
        self.wait(0)
    }

    fn acquire_for(&self, timeout: Duration) -> Result<bool, SyncError> {
        self.wait(duration_to_timeout_ms(timeout)?)
    }

    fn release(&self, permits: u32) -> Result<(), SyncError> {
        if permits == 0 {
            return Err(SyncError::invalid());
        }

        let permits = i32::try_from(permits).map_err(|_| SyncError::overflow())?;
        unsafe { ReleaseSemaphore(self.handle, permits, None) }
            .map_err(|_| map_win32_error(unsafe { GetLastError() }))
    }

    fn max_permits(&self) -> u32 {
        self.max
    }
}

// SAFETY: the owned handle can be transferred across threads and the kernel object provides the
// synchronization semantics.
unsafe impl Send for WindowsSemaphore {}
// SAFETY: shared references only invoke kernel synchronization operations on the handle.
unsafe impl Sync for WindowsSemaphore {}

/// Windows raw rwlock backed by an SRW lock.
#[derive(Debug)]
pub struct WindowsRawRwLock {
    inner: UnsafeCell<SRWLOCK>,
}

impl WindowsRawRwLock {
    /// Creates a new unlocked Windows raw rwlock.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: UnsafeCell::new(SRWLOCK_INIT),
        }
    }

    fn as_mut_ptr(&self) -> *mut SRWLOCK {
        self.inner.get()
    }
}

impl Default for WindowsRawRwLock {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: SRW lock shared/exclusive operations enforce the reader/writer ownership contract.
unsafe impl RawRwLock for WindowsRawRwLock {
    fn support(&self) -> RwLockSupport {
        WINDOWS_RWLOCK_SUPPORT
    }

    fn read_lock(&self) -> Result<(), SyncError> {
        unsafe { AcquireSRWLockShared(self.as_mut_ptr()) };
        Ok(())
    }

    fn try_read_lock(&self) -> Result<bool, SyncError> {
        Ok(unsafe { TryAcquireSRWLockShared(self.as_mut_ptr()) })
    }

    fn write_lock(&self) -> Result<(), SyncError> {
        unsafe { AcquireSRWLockExclusive(self.as_mut_ptr()) };
        Ok(())
    }

    fn try_write_lock(&self) -> Result<bool, SyncError> {
        Ok(unsafe { TryAcquireSRWLockExclusive(self.as_mut_ptr()) })
    }

    unsafe fn read_unlock_unchecked(&self) {
        unsafe { ReleaseSRWLockShared(self.as_mut_ptr()) };
    }

    unsafe fn write_unlock_unchecked(&self) {
        unsafe { ReleaseSRWLockExclusive(self.as_mut_ptr()) };
    }
}

// SAFETY: the lock state is synchronized solely through SRW lock operations.
unsafe impl Send for WindowsRawRwLock {}
// SAFETY: shared references synchronize through the underlying SRW lock.
unsafe impl Sync for WindowsRawRwLock {}

fn duration_to_timeout_ms(duration: Duration) -> Result<u32, SyncError> {
    let millis = duration.as_millis();
    if millis == 0 && !duration.is_zero() {
        return Ok(1);
    }
    u32::try_from(millis).map_err(|_| SyncError::overflow())
}

const fn map_win32_error(error: WIN32_ERROR) -> SyncError {
    match error {
        ERROR_NOT_ENOUGH_MEMORY | ERROR_OUTOFMEMORY => SyncError::platform(error.0 as i32),
        ERROR_INVALID_PARAMETER => SyncError::invalid(),
        ERROR_ACCESS_DENIED => SyncError::permission_denied(),
        ERROR_ALREADY_EXISTS | ERROR_BUSY | ERROR_TIMEOUT => SyncError::busy(),
        ERROR_NOT_SUPPORTED => SyncError::unsupported(),
        _ => SyncError::platform(error.0 as i32),
    }
}

#[cfg(all(test, feature = "std", target_os = "windows"))]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn windows_sync_support_reports_native_mutex_rwlock_and_semaphore() {
        let support = system_sync().support();
        assert_eq!(support.mutex.implementation, SyncImplementationKind::Native);
        assert_eq!(
            support.rwlock.implementation,
            SyncImplementationKind::Native
        );
        assert_eq!(
            support.semaphore.implementation,
            SyncImplementationKind::Native
        );
    }

    #[test]
    fn windows_sync_support_matches_feature_policy_for_once() {
        let support = system_sync().support();

        #[cfg(not(feature = "critical-safe"))]
        {
            assert_eq!(
                support.once.implementation,
                SyncImplementationKind::Emulated
            );
        }

        #[cfg(feature = "critical-safe")]
        {
            assert_eq!(
                support.once.implementation,
                SyncImplementationKind::Unsupported
            );
        }
    }

    #[test]
    fn windows_semaphore_tracks_permits() {
        let semaphore = WindowsSemaphore::new(1, 2).expect("valid semaphore");
        assert!(
            semaphore
                .try_acquire()
                .expect("first try_acquire should succeed")
        );
        assert!(
            !semaphore
                .try_acquire()
                .expect("second try_acquire should see no permit")
        );
        semaphore.release(1).expect("release should succeed");
        assert!(semaphore.try_acquire().expect("permit should be available"));
    }

    #[cfg(not(feature = "critical-safe"))]
    #[test]
    fn windows_raw_once_reset_allows_retry() {
        let once = WindowsRawOnce::new();
        assert_eq!(
            once.begin().expect("begin should succeed"),
            OnceBeginResult::Initialize
        );
        unsafe { once.reset_unchecked() };
        assert_eq!(once.state(), OnceState::Uninitialized);
        assert_eq!(
            once.begin().expect("second begin should succeed"),
            OnceBeginResult::Initialize
        );
        unsafe { once.complete_unchecked() };
        assert_eq!(once.state(), OnceState::Complete);
    }
}
