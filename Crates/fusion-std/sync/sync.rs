//! Public synchronization facade.
//!
//! `fusion-std::sync` stays rooted in the canonical `fusion-sys` primitives, but this layer is
//! allowed to add runtime policy where the public runtime contract actually needs it. Cooperative
//! green scheduling is one of those cases: mutex and rwlock guards acquired in green context now
//! participate in runtime lock-depth accounting so scheduler handoff points can reject yielding or
//! parking while a cooperative lock is still held.
//!
//! # Example
//!
//! ```rust
//! use fusion_std::sync::Mutex;
//!
//! # fn demo() -> Result<(), fusion_std::sync::SyncError> {
//! let value = Mutex::new(1_u32);
//! *value.lock()? += 1;
//! assert_eq!(*value.lock()?, 2);
//! # Ok(())
//! # }
//! # assert!(demo().is_ok());
//! ```

use core::marker::PhantomData;
use core::mem::ManuallyDrop;
use core::num::NonZeroU16;
use core::ops::{
    Deref,
    DerefMut,
};

use fusion_sys::sync as sys_sync;
pub use fusion_sys::sync::{
    LeftRight,
    LeftRightReadGuard,
    MutexCaps,
    MutexSupport,
    Once,
    OnceBeginResult,
    OnceCaps,
    OnceLock,
    OnceState,
    OnceSupport,
    PriorityInheritanceSupport,
    ProcessScopeSupport,
    RawMutex,
    RawOnce,
    RawRwLock,
    RawSemaphore,
    RecursionSupport,
    RobustnessSupport,
    RwLockCaps,
    RwLockFairnessSupport,
    RwLockSupport,
    Semaphore,
    SemaphoreCaps,
    SemaphoreSupport,
    SharedHeader,
    SharedRelease,
    SpinMutex,
    SyncBase,
    SyncError,
    SyncErrorKind,
    SyncFallbackKind,
    SyncImplementationKind,
    SyncSupport,
    ThinMutex,
    ThinMutexGuard,
    TimeoutCaps,
};

use crate::thread::{
    CooperativeExclusionSpan,
    CooperativeGreenLockToken,
    enter_current_green_cooperative_lock,
    exit_current_green_cooperative_lock,
};

/// Explicit cooperative lock rank used for ordered green-context lock acquisition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CooperativeLockRank(NonZeroU16);

impl CooperativeLockRank {
    /// Creates one explicit cooperative lock rank.
    ///
    /// # Errors
    ///
    /// Returns an error when the supplied rank is zero.
    pub const fn new(rank: u16) -> Result<Self, SyncError> {
        match NonZeroU16::new(rank) {
            Some(rank) => Ok(Self(rank)),
            None => Err(SyncError::invalid()),
        }
    }

    /// Returns the concrete numeric rank.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0.get()
    }
}

/// Data-carrying mutex with runtime-aware cooperative guard accounting.
#[derive(Debug)]
pub struct Mutex<T: ?Sized> {
    cooperative_rank: Option<CooperativeLockRank>,
    cooperative_span: Option<CooperativeExclusionSpan>,
    inner: sys_sync::Mutex<T>,
}

impl<T> Mutex<T> {
    /// Creates a new mutex protecting `value`.
    #[must_use]
    pub const fn new(value: T) -> Self {
        Self {
            inner: sys_sync::Mutex::new(value),
            cooperative_rank: None,
            cooperative_span: None,
        }
    }

    /// Creates a new ranked mutex protecting `value`.
    #[must_use]
    pub const fn ranked(value: T, rank: CooperativeLockRank) -> Self {
        Self {
            inner: sys_sync::Mutex::new(value),
            cooperative_rank: Some(rank),
            cooperative_span: None,
        }
    }

    /// Creates a new mutex protecting `value` inside one named cooperative exclusion span.
    #[must_use]
    pub const fn spanned(value: T, span: CooperativeExclusionSpan) -> Self {
        Self {
            inner: sys_sync::Mutex::new(value),
            cooperative_rank: None,
            cooperative_span: Some(span),
        }
    }

    /// Creates a new ranked mutex protecting `value` inside one named cooperative exclusion span.
    #[must_use]
    pub const fn ranked_spanned(
        value: T,
        rank: CooperativeLockRank,
        span: CooperativeExclusionSpan,
    ) -> Self {
        Self {
            inner: sys_sync::Mutex::new(value),
            cooperative_rank: Some(rank),
            cooperative_span: Some(span),
        }
    }
}

impl<T: ?Sized> Mutex<T> {
    /// Acquires the mutex and returns a mutable guard.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying mutex cannot be acquired honestly.
    pub fn lock(&self) -> Result<MutexGuard<'_, T>, SyncError> {
        let guard = self.inner.lock()?;
        MutexGuard::new(guard, self.cooperative_rank, self.cooperative_span)
    }

    /// Attempts to acquire the mutex without blocking.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying mutex cannot evaluate the acquisition honestly.
    pub fn try_lock(&self) -> Result<Option<MutexGuard<'_, T>>, SyncError> {
        self.inner
            .try_lock()?
            .map(|guard| MutexGuard::new(guard, self.cooperative_rank, self.cooperative_span))
            .transpose()
    }
}

unsafe impl<T: ?Sized + Send> Send for Mutex<T> {}
unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}

/// Guard returned while a [`Mutex`] is held.
#[must_use]
pub struct MutexGuard<'a, T: ?Sized> {
    guard: ManuallyDrop<sys_sync::MutexGuard<'a, T>>,
    cooperative_lock: CooperativeGreenLockToken,
    _not_send: PhantomData<*mut ()>,
}

impl<'a, T: ?Sized> MutexGuard<'a, T> {
    fn new(
        guard: sys_sync::MutexGuard<'a, T>,
        rank: Option<CooperativeLockRank>,
        span: Option<CooperativeExclusionSpan>,
    ) -> Result<Self, SyncError> {
        let cooperative_lock =
            match enter_current_green_cooperative_lock(rank.map(CooperativeLockRank::get), span) {
                Ok(token) => token,
                Err(error) => {
                    drop(guard);
                    return Err(error);
                }
            };
        Ok(Self {
            guard: ManuallyDrop::new(guard),
            cooperative_lock,
            _not_send: PhantomData,
        })
    }
}

impl<T: ?Sized> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl<T: ?Sized> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard
    }
}

impl<T: ?Sized> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.guard);
        }
        exit_current_green_cooperative_lock(self.cooperative_lock);
    }
}

/// Data-carrying reader/writer lock with runtime-aware cooperative guard accounting.
#[derive(Debug)]
pub struct RwLock<T: ?Sized> {
    cooperative_rank: Option<CooperativeLockRank>,
    cooperative_span: Option<CooperativeExclusionSpan>,
    inner: sys_sync::RwLock<T>,
}

impl<T> RwLock<T> {
    /// Creates a new rwlock protecting `value`.
    #[must_use]
    pub const fn new(value: T) -> Self {
        Self {
            inner: sys_sync::RwLock::new(value),
            cooperative_rank: None,
            cooperative_span: None,
        }
    }

    /// Creates a new ranked rwlock protecting `value`.
    #[must_use]
    pub const fn ranked(value: T, rank: CooperativeLockRank) -> Self {
        Self {
            inner: sys_sync::RwLock::new(value),
            cooperative_rank: Some(rank),
            cooperative_span: None,
        }
    }

    /// Creates a new rwlock protecting `value` inside one named cooperative exclusion span.
    #[must_use]
    pub const fn spanned(value: T, span: CooperativeExclusionSpan) -> Self {
        Self {
            inner: sys_sync::RwLock::new(value),
            cooperative_rank: None,
            cooperative_span: Some(span),
        }
    }

    /// Creates a new ranked rwlock protecting `value` inside one named cooperative exclusion span.
    #[must_use]
    pub const fn ranked_spanned(
        value: T,
        rank: CooperativeLockRank,
        span: CooperativeExclusionSpan,
    ) -> Self {
        Self {
            inner: sys_sync::RwLock::new(value),
            cooperative_rank: Some(rank),
            cooperative_span: Some(span),
        }
    }
}

impl<T: ?Sized> RwLock<T> {
    /// Reports the support surface of the selected raw rwlock backend.
    #[must_use]
    pub fn support(&self) -> RwLockSupport {
        self.inner.support()
    }

    /// Acquires a shared/read lock.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying rwlock cannot complete the acquisition honestly.
    pub fn read(&self) -> Result<RwLockReadGuard<'_, T>, SyncError> {
        let guard = self.inner.read()?;
        RwLockReadGuard::new(guard, self.cooperative_rank, self.cooperative_span)
    }

    /// Attempts to acquire a shared/read lock without blocking.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying rwlock cannot evaluate the acquisition honestly.
    pub fn try_read(&self) -> Result<Option<RwLockReadGuard<'_, T>>, SyncError> {
        self.inner
            .try_read()?
            .map(|guard| RwLockReadGuard::new(guard, self.cooperative_rank, self.cooperative_span))
            .transpose()
    }

    /// Acquires an exclusive/write lock.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying rwlock cannot complete the acquisition honestly.
    pub fn write(&self) -> Result<RwLockWriteGuard<'_, T>, SyncError> {
        let guard = self.inner.write()?;
        RwLockWriteGuard::new(guard, self.cooperative_rank, self.cooperative_span)
    }

    /// Attempts to acquire an exclusive/write lock without blocking.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying rwlock cannot evaluate the acquisition honestly.
    pub fn try_write(&self) -> Result<Option<RwLockWriteGuard<'_, T>>, SyncError> {
        self.inner
            .try_write()?
            .map(|guard| RwLockWriteGuard::new(guard, self.cooperative_rank, self.cooperative_span))
            .transpose()
    }
}

unsafe impl<T: ?Sized + Send> Send for RwLock<T> {}
unsafe impl<T: ?Sized + Send + Sync> Sync for RwLock<T> {}

/// Guard returned while a shared/read lock is held.
#[must_use]
pub struct RwLockReadGuard<'a, T: ?Sized> {
    guard: ManuallyDrop<sys_sync::RwLockReadGuard<'a, T>>,
    cooperative_lock: CooperativeGreenLockToken,
    _not_send: PhantomData<*mut ()>,
}

impl<'a, T: ?Sized> RwLockReadGuard<'a, T> {
    fn new(
        guard: sys_sync::RwLockReadGuard<'a, T>,
        rank: Option<CooperativeLockRank>,
        span: Option<CooperativeExclusionSpan>,
    ) -> Result<Self, SyncError> {
        let cooperative_lock =
            match enter_current_green_cooperative_lock(rank.map(CooperativeLockRank::get), span) {
                Ok(token) => token,
                Err(error) => {
                    drop(guard);
                    return Err(error);
                }
            };
        Ok(Self {
            guard: ManuallyDrop::new(guard),
            cooperative_lock,
            _not_send: PhantomData,
        })
    }
}

impl<T: ?Sized> Deref for RwLockReadGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl<T: ?Sized> Drop for RwLockReadGuard<'_, T> {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.guard);
        }
        exit_current_green_cooperative_lock(self.cooperative_lock);
    }
}

/// Guard returned while an exclusive/write lock is held.
#[must_use]
pub struct RwLockWriteGuard<'a, T: ?Sized> {
    guard: ManuallyDrop<sys_sync::RwLockWriteGuard<'a, T>>,
    cooperative_lock: CooperativeGreenLockToken,
    _not_send: PhantomData<*mut ()>,
}

impl<'a, T: ?Sized> RwLockWriteGuard<'a, T> {
    fn new(
        guard: sys_sync::RwLockWriteGuard<'a, T>,
        rank: Option<CooperativeLockRank>,
        span: Option<CooperativeExclusionSpan>,
    ) -> Result<Self, SyncError> {
        let cooperative_lock =
            match enter_current_green_cooperative_lock(rank.map(CooperativeLockRank::get), span) {
                Ok(token) => token,
                Err(error) => {
                    drop(guard);
                    return Err(error);
                }
            };
        Ok(Self {
            guard: ManuallyDrop::new(guard),
            cooperative_lock,
            _not_send: PhantomData,
        })
    }
}

impl<T: ?Sized> Deref for RwLockWriteGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl<T: ?Sized> DerefMut for RwLockWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard
    }
}

impl<T: ?Sized> Drop for RwLockWriteGuard<'_, T> {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.guard);
        }
        exit_current_green_cooperative_lock(self.cooperative_lock);
    }
}

#[cfg(all(test, feature = "std", not(target_os = "none")))]
mod tests {
    use super::*;
    extern crate std;
    use self::std::sync::Arc;
    use self::std::thread;

    #[test]
    fn mutex_protects_data() {
        let value = Arc::new(Mutex::new(0_u32));
        let mut threads = self::std::vec::Vec::new();

        for _ in 0..4 {
            let value = Arc::clone(&value);
            threads.push(thread::spawn(move || {
                for _ in 0..250 {
                    let mut guard = value.lock().expect("mutex should lock");
                    *guard += 1;
                }
            }));
        }

        for thread in threads {
            thread.join().expect("thread should finish");
        }

        assert_eq!(*value.lock().expect("mutex should lock"), 1_000);
    }
}
