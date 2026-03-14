//! Small spin-based mutex fallback for platforms without a stronger backend lock.

use core::hint::spin_loop;
use core::sync::atomic::{AtomicBool, Ordering};

use fusion_pal::pal::sync::{
    MutexCaps, MutexSupport, PriorityInheritanceSupport, ProcessScopeSupport, RawMutex,
    RecursionSupport, RobustnessSupport, SyncError, SyncFallbackKind, SyncImplementationKind,
    TimeoutCaps,
};

const SPIN_MUTEX_SUPPORT: MutexSupport = MutexSupport {
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

/// Spin-based mutex fallback for internal narrow critical sections.
///
/// This lock is explicitly unfair and may allow barging under contention. It exists as a
/// last-resort fallback where a stronger fusion-pal-backed mutex is unavailable.
///
/// TODO: Replace spin fallback usage with stronger fusion-pal-backed mutexes on platforms that grow
/// honest native synchronization support. Busy waiting is a fallback, not a virtue.
#[derive(Debug)]
pub struct SpinMutex {
    state: AtomicBool,
}

impl SpinMutex {
    /// Creates a new unlocked spin mutex.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: AtomicBool::new(false),
        }
    }
}

impl Default for SpinMutex {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: `SpinMutex` uses acquire/release semantics on a single atomic ownership bit and
// requires balanced unlocks through the unsafe contract.
unsafe impl RawMutex for SpinMutex {
    fn support(&self) -> MutexSupport {
        SPIN_MUTEX_SUPPORT
    }

    fn lock(&self) -> Result<(), SyncError> {
        while self
            .state
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            spin_loop();
        }
        Ok(())
    }

    fn try_lock(&self) -> Result<bool, SyncError> {
        match self
            .state
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    unsafe fn unlock_unchecked(&self) {
        self.state.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicU32, Ordering};
    extern crate std;
    use self::std::sync::Arc;
    use self::std::thread;

    #[test]
    fn spin_mutex_serializes_threads() {
        let lock = Arc::new(SpinMutex::new());
        let counter = Arc::new(AtomicU32::new(0));
        let mut threads = self::std::vec::Vec::new();

        for _ in 0..4 {
            let lock = Arc::clone(&lock);
            let counter = Arc::clone(&counter);
            threads.push(thread::spawn(move || {
                for _ in 0..250 {
                    lock.lock().expect("spin mutex should lock");
                    counter.fetch_add(1, Ordering::Relaxed);
                    // SAFETY: this thread currently holds the lock.
                    unsafe { lock.unlock_unchecked() };
                }
            }));
        }

        for thread in threads {
            thread.join().expect("thread should finish");
        }

        assert_eq!(counter.load(Ordering::Relaxed), 1_000);
    }
}
