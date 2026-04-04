//! Thin non-recursive mutex that selects the strongest currently available backend path.

use fusion_pal::sys::sync::{
    PLATFORM_RAW_MUTEX_IMPLEMENTATION,
    PlatformRawMutex,
    SyncImplementationKind,
};

use super::SpinMutex;
use super::{
    MutexSupport,
    RawMutex,
    SyncError,
};

#[derive(Debug)]
enum ThinMutexInner {
    Platform(PlatformRawMutex),
    Spin(SpinMutex),
}

/// Small non-recursive mutex intended for internal infrastructure and narrow critical sections.
///
/// This mutex intentionally makes no fairness or FIFO wake guarantee. It prefers the fusion-pal's
/// selected local raw mutex and falls back to a spin mutex only when the backend reports that
/// no stronger raw mutex exists.
#[derive(Debug)]
pub struct ThinMutex {
    inner: ThinMutexInner,
}

impl ThinMutex {
    /// Creates a new thin mutex.
    #[must_use]
    pub const fn new() -> Self {
        match PLATFORM_RAW_MUTEX_IMPLEMENTATION {
            SyncImplementationKind::Unsupported => Self {
                inner: ThinMutexInner::Spin(SpinMutex::new()),
            },
            _ => Self {
                inner: ThinMutexInner::Platform(PlatformRawMutex::new()),
            },
        }
    }

    /// Returns the support surface of the selected inner mutex.
    #[must_use]
    pub fn support(&self) -> MutexSupport {
        match &self.inner {
            ThinMutexInner::Platform(inner) => inner.support(),
            ThinMutexInner::Spin(inner) => inner.support(),
        }
    }

    /// Acquires the mutex and returns a guard that releases it on drop.
    ///
    /// # Errors
    ///
    /// Returns an error if the selected backend lock cannot be acquired honestly.
    pub fn lock(&self) -> Result<ThinMutexGuard<'_>, SyncError> {
        match &self.inner {
            ThinMutexInner::Platform(inner) => inner.lock()?,
            ThinMutexInner::Spin(inner) => inner.lock()?,
        }
        Ok(ThinMutexGuard { mutex: self })
    }

    /// Attempts to acquire the mutex without blocking.
    ///
    /// # Errors
    ///
    /// Returns an error if the selected backend cannot evaluate the acquisition honestly.
    pub fn try_lock(&self) -> Result<Option<ThinMutexGuard<'_>>, SyncError> {
        let locked = match &self.inner {
            ThinMutexInner::Platform(inner) => inner.try_lock()?,
            ThinMutexInner::Spin(inner) => inner.try_lock()?,
        };

        if locked {
            Ok(Some(ThinMutexGuard { mutex: self }))
        } else {
            Ok(None)
        }
    }
}

impl Default for ThinMutex {
    fn default() -> Self {
        Self::new()
    }
}

/// Guard returned while a [`ThinMutex`] is held.
#[must_use]
pub struct ThinMutexGuard<'a> {
    mutex: &'a ThinMutex,
}

impl Drop for ThinMutexGuard<'_> {
    fn drop(&mut self) {
        // SAFETY: this guard exists only after a successful lock acquisition and drops once.
        unsafe {
            match &self.mutex.inner {
                ThinMutexInner::Platform(inner) => inner.unlock_unchecked(),
                ThinMutexInner::Spin(inner) => inner.unlock_unchecked(),
            }
        };
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

    #[test]
    fn thin_mutex_serializes_threads() {
        let lock = Arc::new(ThinMutex::new());
        let counter = Arc::new(AtomicU32::new(0));
        let mut threads = self::std::vec::Vec::new();

        for _ in 0..4 {
            let lock = Arc::clone(&lock);
            let counter = Arc::clone(&counter);
            threads.push(thread::spawn(move || {
                for _ in 0..250 {
                    let _guard = lock.lock().expect("thin mutex should lock");
                    counter.fetch_add(1, Ordering::Relaxed);
                }
            }));
        }

        for thread in threads {
            thread.join().expect("thread should finish");
        }

        assert_eq!(counter.load(Ordering::Relaxed), 1_000);
    }
}
