//! Data-carrying reader/writer lock built on top of the selected fusion-pal raw rwlock backend.

use core::cell::UnsafeCell;
use core::ops::{
    Deref,
    DerefMut,
};

use fusion_pal::sys::sync::{
    PlatformRawRwLock,
    RawRwLock,
};

use super::{
    RwLockSupport,
    SyncError,
};

/// Data-carrying reader/writer lock with no poisoning or hidden allocation.
#[derive(Debug)]
pub struct RwLock<T: ?Sized> {
    lock: PlatformRawRwLock,
    value: UnsafeCell<T>,
}

impl<T> RwLock<T> {
    /// Creates a new rwlock protecting `value`.
    #[must_use]
    pub const fn new(value: T) -> Self {
        Self {
            lock: PlatformRawRwLock::new(),
            value: UnsafeCell::new(value),
        }
    }
}

impl<T: ?Sized> RwLock<T> {
    /// Reports the support surface of the selected raw rwlock backend.
    #[must_use]
    pub fn support(&self) -> RwLockSupport {
        self.lock.support()
    }

    /// Acquires a shared/read lock.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying raw rwlock cannot complete the acquisition
    /// honestly.
    pub fn read(&self) -> Result<RwLockReadGuard<'_, T>, SyncError> {
        self.lock.read_lock()?;
        Ok(RwLockReadGuard { lock: self })
    }

    /// Attempts to acquire a shared/read lock without blocking.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying raw rwlock cannot evaluate the acquisition
    /// honestly.
    pub fn try_read(&self) -> Result<Option<RwLockReadGuard<'_, T>>, SyncError> {
        Ok(self
            .lock
            .try_read_lock()?
            .then_some(RwLockReadGuard { lock: self }))
    }

    /// Acquires an exclusive/write lock.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying raw rwlock cannot complete the acquisition
    /// honestly.
    pub fn write(&self) -> Result<RwLockWriteGuard<'_, T>, SyncError> {
        self.lock.write_lock()?;
        Ok(RwLockWriteGuard { lock: self })
    }

    /// Attempts to acquire an exclusive/write lock without blocking.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying raw rwlock cannot evaluate the acquisition
    /// honestly.
    pub fn try_write(&self) -> Result<Option<RwLockWriteGuard<'_, T>>, SyncError> {
        Ok(self
            .lock
            .try_write_lock()?
            .then_some(RwLockWriteGuard { lock: self }))
    }
}

unsafe impl<T: ?Sized + Send> Send for RwLock<T> {}
unsafe impl<T: ?Sized + Send + Sync> Sync for RwLock<T> {}

/// Guard returned while a shared/read lock is held.
#[must_use]
pub struct RwLockReadGuard<'a, T: ?Sized> {
    lock: &'a RwLock<T>,
}

impl<T: ?Sized> Deref for RwLockReadGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: the guard holds a live shared/read lock for the duration of this borrow.
        unsafe { &*self.lock.value.get() }
    }
}

impl<T: ?Sized> Drop for RwLockReadGuard<'_, T> {
    fn drop(&mut self) {
        // SAFETY: this guard exists only after a successful read lock acquisition and drops
        // exactly once for that acquisition.
        unsafe { self.lock.lock.read_unlock_unchecked() };
    }
}

/// Guard returned while an exclusive/write lock is held.
#[must_use]
pub struct RwLockWriteGuard<'a, T: ?Sized> {
    lock: &'a RwLock<T>,
}

impl<T: ?Sized> Deref for RwLockWriteGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: the guard holds a live exclusive/write lock for the duration of this borrow.
        unsafe { &*self.lock.value.get() }
    }
}

impl<T: ?Sized> DerefMut for RwLockWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: the guard holds a live exclusive/write lock for the duration of this borrow.
        unsafe { &mut *self.lock.value.get() }
    }
}

impl<T: ?Sized> Drop for RwLockWriteGuard<'_, T> {
    fn drop(&mut self) {
        // SAFETY: this guard exists only after a successful write lock acquisition and
        // drops exactly once for that acquisition.
        unsafe { self.lock.lock.write_unlock_unchecked() };
    }
}

#[cfg(all(test, feature = "std", not(target_os = "none")))]
mod tests {
    use super::*;
    use core::sync::atomic::{
        AtomicU32,
        Ordering,
    };
    extern crate std;
    use self::std::sync::Arc;
    use self::std::thread;
    use self::std::time::Duration;

    #[test]
    fn rwlock_allows_concurrent_readers() {
        let lock = Arc::new(RwLock::new(7_u32));
        let active = Arc::new(AtomicU32::new(0));
        let max_seen = Arc::new(AtomicU32::new(0));
        let mut threads = self::std::vec::Vec::new();

        for _ in 0..4 {
            let lock = Arc::clone(&lock);
            let active = Arc::clone(&active);
            let max_seen = Arc::clone(&max_seen);
            threads.push(thread::spawn(move || {
                let guard = lock.read().expect("read lock should succeed");
                assert_eq!(*guard, 7);
                let current = active.fetch_add(1, Ordering::AcqRel) + 1;
                update_max(&max_seen, current);
                thread::sleep(Duration::from_millis(5));
                active.fetch_sub(1, Ordering::AcqRel);
            }));
        }

        for thread in threads {
            thread.join().expect("reader should finish");
        }

        assert!(max_seen.load(Ordering::Acquire) >= 2);
    }

    #[test]
    fn rwlock_writer_excludes_other_access() {
        let lock = RwLock::new(1_u32);
        let mut writer = lock.write().expect("write lock should succeed");
        match lock.try_read() {
            Ok(guard) => assert!(guard.is_none()),
            Err(error) => assert_eq!(error, SyncError::busy()),
        }
        match lock.try_write() {
            Ok(guard) => assert!(guard.is_none()),
            Err(error) => assert_eq!(error, SyncError::busy()),
        }
        *writer = 9;
        drop(writer);
        assert_eq!(*lock.read().expect("read should succeed"), 9);
    }

    fn update_max(max_seen: &AtomicU32, candidate: u32) {
        let mut current = max_seen.load(Ordering::Acquire);
        while candidate > current {
            match max_seen.compare_exchange_weak(
                current,
                candidate,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(observed) => current = observed,
            }
        }
    }
}
