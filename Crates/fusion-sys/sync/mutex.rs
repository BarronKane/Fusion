//! Data-carrying mutex built on top of [`ThinMutex`].

use core::cell::UnsafeCell;
use core::ops::{
    Deref,
    DerefMut,
};

use super::{
    SyncError,
    ThinMutex,
    ThinMutexGuard,
};

/// Data-carrying mutex with no poisoning or hidden allocation.
#[derive(Debug)]
pub struct Mutex<T: ?Sized> {
    lock: ThinMutex,
    value: UnsafeCell<T>,
}

impl<T> Mutex<T> {
    /// Creates a new mutex protecting `value`.
    #[must_use]
    pub const fn new(value: T) -> Self {
        Self {
            lock: ThinMutex::new(),
            value: UnsafeCell::new(value),
        }
    }
}

impl<T: ?Sized> Mutex<T> {
    /// Acquires the mutex and returns a mutable guard.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying thin mutex cannot be acquired honestly.
    pub fn lock(&self) -> Result<MutexGuard<'_, T>, SyncError> {
        let guard = self.lock.lock()?;
        Ok(MutexGuard {
            mutex: self,
            _guard: guard,
        })
    }

    /// Attempts to acquire the mutex without blocking.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying thin mutex cannot evaluate the acquisition
    /// honestly.
    pub fn try_lock(&self) -> Result<Option<MutexGuard<'_, T>>, SyncError> {
        Ok(self.lock.try_lock()?.map(|guard| MutexGuard {
            mutex: self,
            _guard: guard,
        }))
    }
}

unsafe impl<T: ?Sized + Send> Send for Mutex<T> {}
unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}

/// Guard returned while a [`Mutex`] is held.
#[must_use]
pub struct MutexGuard<'a, T: ?Sized> {
    mutex: &'a Mutex<T>,
    _guard: ThinMutexGuard<'a>,
}

impl<T: ?Sized> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: the guard holds exclusive logical access for the duration of this borrow.
        unsafe { &*self.mutex.value.get() }
    }
}

impl<T: ?Sized> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: the guard holds exclusive logical access for the duration of this borrow.
        unsafe { &mut *self.mutex.value.get() }
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
