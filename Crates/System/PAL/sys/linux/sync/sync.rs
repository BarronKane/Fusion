//! Linux PAL synchronization backend.
//!
//! The Linux backend exposes futex-backed local wait/wake, a thin non-recursive mutex built
//! on that wait primitive, and a local counting semaphore composed over the same futex word.
//! Richer semantics such as PI mutexes, robustness, or process-shared primitives remain
//! capability-gated extensions rather than baseline promises.
//!
//! The current backend intentionally sticks to baseline `FUTEX_WAIT` / `FUTEX_WAKE` style
//! operations. If future work adds newer futex operations with stronger timeout or clock
//! semantics, those paths need the same kernel-version gating discipline already used by the
//! memory backend.

use core::convert::TryFrom;
use core::sync::atomic::{AtomicU32, Ordering};
use core::time::Duration;

use rustix::io::Errno;
use rustix::thread::futex;

use crate::pal::sync::{
    MutexCaps, MutexSupport, PriorityInheritanceSupport, ProcessScopeSupport, RawMutex,
    RawSemaphore, RecursionSupport, RobustnessSupport, SemaphoreCaps, SemaphoreSupport, SyncBase,
    SyncError, SyncErrorKind, SyncImplementationKind, SyncSupport, TimeoutCaps, WaitCaps,
    WaitOutcome, WaitPrimitive, WaitSupport,
};

const UNLOCKED: u32 = 0;
const LOCKED: u32 = 1;
const CONTENDED: u32 = 2;

const LINUX_WAIT_SUPPORT: WaitSupport = WaitSupport {
    caps: WaitCaps::WAIT_WHILE_EQUAL
        .union(WaitCaps::WAKE_ONE)
        .union(WaitCaps::WAKE_ALL)
        .union(WaitCaps::SPURIOUS_WAKE),
    timeout: TimeoutCaps::RELATIVE.union(TimeoutCaps::RELATIVE_MONOTONIC),
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Native,
};

const LINUX_MUTEX_SUPPORT: MutexSupport = MutexSupport {
    caps: MutexCaps::TRY_LOCK
        .union(MutexCaps::BLOCKING)
        .union(MutexCaps::STATIC_INIT),
    timeout: TimeoutCaps::empty(),
    priority_inheritance: PriorityInheritanceSupport::None,
    recursion: RecursionSupport::None,
    robustness: RobustnessSupport::None,
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Native,
};

const LINUX_SEMAPHORE_SUPPORT: SemaphoreSupport = SemaphoreSupport {
    caps: SemaphoreCaps::TRY_ACQUIRE
        .union(SemaphoreCaps::BLOCKING)
        .union(SemaphoreCaps::RELEASE_MANY),
    timeout: TimeoutCaps::empty(),
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Emulated,
};

/// Linux synchronization provider handle.
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxSync;

/// Selected raw mutex type for Linux builds.
pub type PlatformRawMutex = LinuxRawMutex;

/// Selected semaphore type for Linux builds.
pub type PlatformSemaphore = LinuxSemaphore;

/// Target-selected synchronization provider alias for Linux builds.
pub type PlatformSync = LinuxSync;

/// Backend truth for the selected raw mutex implementation on Linux.
pub const PLATFORM_RAW_MUTEX_IMPLEMENTATION: SyncImplementationKind =
    SyncImplementationKind::Native;

/// Returns the process-wide Linux synchronization provider handle.
#[must_use]
pub const fn system_sync() -> PlatformSync {
    PlatformSync::new()
}

impl LinuxSync {
    /// Creates a new Linux synchronization provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl SyncBase for LinuxSync {
    fn support(&self) -> SyncSupport {
        SyncSupport {
            wait: LINUX_WAIT_SUPPORT,
            mutex: LINUX_MUTEX_SUPPORT,
            semaphore: LINUX_SEMAPHORE_SUPPORT,
        }
    }
}

impl WaitPrimitive for LinuxSync {
    fn support(&self) -> WaitSupport {
        LINUX_WAIT_SUPPORT
    }

    fn wait_while_equal(
        &self,
        word: &AtomicU32,
        expected: u32,
        timeout: Option<Duration>,
    ) -> Result<WaitOutcome, SyncError> {
        futex_wait_private(word, expected, timeout)
    }

    fn wake_one(&self, word: &AtomicU32) -> Result<usize, SyncError> {
        futex::wake(word, futex::Flags::PRIVATE, 1).map_err(map_errno)
    }

    fn wake_all(&self, word: &AtomicU32) -> Result<usize, SyncError> {
        futex::wake(word, futex::Flags::PRIVATE, u32::MAX).map_err(map_errno)
    }
}

/// Thin local Linux mutex built over a futex word.
#[derive(Debug)]
pub struct LinuxRawMutex {
    state: AtomicU32,
}

impl LinuxRawMutex {
    /// Creates a new unlocked Linux raw mutex.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: AtomicU32::new(UNLOCKED),
        }
    }

    fn lock_contended(&self) -> Result<(), SyncError> {
        let mut state = self.state.load(Ordering::Relaxed);
        loop {
            if state == UNLOCKED {
                match self.state.compare_exchange_weak(
                    UNLOCKED,
                    LOCKED,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => return Ok(()),
                    Err(observed) => {
                        state = observed;
                        continue;
                    }
                }
            }

            if state != CONTENDED {
                state = self.state.swap(CONTENDED, Ordering::Acquire);
                if state == UNLOCKED {
                    return Ok(());
                }
            }

            match futex_wait_private(&self.state, CONTENDED, None)? {
                WaitOutcome::Woken
                | WaitOutcome::Mismatch
                | WaitOutcome::Interrupted
                | WaitOutcome::TimedOut => {
                    state = self.state.load(Ordering::Relaxed);
                }
            }
        }
    }
}

// SAFETY: `LinuxRawMutex` enforces exclusive ownership with acquire/release semantics over a
// single futex word and requires balanced unlocks through the unsafe contract.
unsafe impl RawMutex for LinuxRawMutex {
    fn support(&self) -> MutexSupport {
        LINUX_MUTEX_SUPPORT
    }

    fn lock(&self) -> Result<(), SyncError> {
        if self
            .state
            .compare_exchange(UNLOCKED, LOCKED, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            return Ok(());
        }

        self.lock_contended()
    }

    fn try_lock(&self) -> Result<bool, SyncError> {
        match self
            .state
            .compare_exchange(UNLOCKED, LOCKED, Ordering::Acquire, Ordering::Relaxed)
        {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    unsafe fn unlock_unchecked(&self) {
        if self.state.swap(UNLOCKED, Ordering::Release) == CONTENDED {
            let _ = futex::wake(&self.state, futex::Flags::PRIVATE, 1);
        }
    }
}

/// Linux local counting semaphore composed over a futex word.
#[derive(Debug)]
pub struct LinuxSemaphore {
    permits: AtomicU32,
    max: u32,
}

impl LinuxSemaphore {
    /// Creates a new Linux semaphore with the given initial and maximum permit counts.
    pub const fn new(initial: u32, max: u32) -> Result<Self, SyncError> {
        if max == 0 || initial > max {
            return Err(SyncError::invalid());
        }

        Ok(Self {
            permits: AtomicU32::new(initial),
            max,
        })
    }
}

impl RawSemaphore for LinuxSemaphore {
    fn support(&self) -> SemaphoreSupport {
        LINUX_SEMAPHORE_SUPPORT
    }

    fn acquire(&self) -> Result<(), SyncError> {
        loop {
            let current = self.permits.load(Ordering::Acquire);
            if current == 0 {
                match futex_wait_private(&self.permits, 0, None)? {
                    WaitOutcome::Woken
                    | WaitOutcome::Mismatch
                    | WaitOutcome::Interrupted
                    | WaitOutcome::TimedOut => {}
                }
                continue;
            }

            if self
                .permits
                .compare_exchange_weak(current, current - 1, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                return Ok(());
            }
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

        let mut current = self.permits.load(Ordering::Acquire);
        loop {
            let next = current
                .checked_add(permits)
                .ok_or_else(SyncError::overflow)?;
            if next > self.max {
                return Err(SyncError::overflow());
            }

            match self.permits.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    let _ = futex::wake(&self.permits, futex::Flags::PRIVATE, permits);
                    return Ok(());
                }
                Err(observed) => current = observed,
            }
        }
    }

    fn max_permits(&self) -> u32 {
        self.max
    }
}

fn futex_wait_private(
    word: &AtomicU32,
    expected: u32,
    timeout: Option<Duration>,
) -> Result<WaitOutcome, SyncError> {
    let timeout_storage = duration_to_timespec(timeout)?;
    match futex::wait(
        word,
        futex::Flags::PRIVATE,
        expected,
        timeout_storage.as_ref(),
    ) {
        Ok(()) => Ok(WaitOutcome::Woken),
        Err(Errno::AGAIN) => Ok(WaitOutcome::Mismatch),
        Err(Errno::TIMEDOUT) => Ok(WaitOutcome::TimedOut),
        Err(Errno::INTR) => Ok(WaitOutcome::Interrupted),
        Err(errno) => Err(map_errno(errno)),
    }
}

fn duration_to_timespec(timeout: Option<Duration>) -> Result<Option<futex::Timespec>, SyncError> {
    timeout
        .map(|duration| {
            let secs = i64::try_from(duration.as_secs()).map_err(|_| SyncError::overflow())?;
            let nsecs = i64::from(duration.subsec_nanos());
            Ok(futex::Timespec {
                tv_sec: secs,
                tv_nsec: nsecs,
            })
        })
        .transpose()
}

const fn map_errno(errno: Errno) -> SyncError {
    match errno {
        Errno::INVAL => SyncError::invalid(),
        Errno::AGAIN | Errno::BUSY => SyncError::busy(),
        Errno::PERM | Errno::ACCESS => SyncError::permission_denied(),
        _ => SyncError {
            kind: SyncErrorKind::Platform(errno.raw_os_error()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::AtomicU32;
    extern crate std;
    use self::std::sync::Arc;
    use self::std::thread;
    use self::std::time::Duration as StdDuration;

    #[test]
    fn linux_raw_mutex_serializes_threads() {
        let lock = Arc::new(LinuxRawMutex::new());
        let counter = Arc::new(AtomicU32::new(0));
        let mut threads = self::std::vec::Vec::new();

        for _ in 0..4 {
            let lock = Arc::clone(&lock);
            let counter = Arc::clone(&counter);
            threads.push(thread::spawn(move || {
                for _ in 0..500 {
                    lock.lock().expect("linux raw mutex should lock");
                    counter.fetch_add(1, Ordering::Relaxed);
                    // SAFETY: this thread currently holds the lock.
                    unsafe { lock.unlock_unchecked() };
                }
            }));
        }

        for thread in threads {
            thread.join().expect("thread should finish");
        }

        assert_eq!(counter.load(Ordering::Relaxed), 2_000);
    }

    #[test]
    fn linux_semaphore_tracks_permits() {
        let semaphore = LinuxSemaphore::new(1, 2).expect("valid semaphore");
        assert!(semaphore.try_acquire().expect("try_acquire should work"));
        assert!(
            !semaphore
                .try_acquire()
                .expect("second try_acquire should fail")
        );
        semaphore.release(1).expect("release should work");
        assert!(
            semaphore
                .try_acquire()
                .expect("permit should be available again")
        );
    }

    #[test]
    fn linux_semaphore_overflow_is_rejected() {
        let semaphore = LinuxSemaphore::new(1, 2).expect("valid semaphore");
        assert!(matches!(
            semaphore.release(2),
            Err(SyncError {
                kind: SyncErrorKind::Overflow
            })
        ));
    }

    #[test]
    fn linux_semaphore_blocking_acquire_waits_for_release() {
        let semaphore = Arc::new(LinuxSemaphore::new(0, 1).expect("valid semaphore"));
        let waiter = {
            let semaphore = Arc::clone(&semaphore);
            thread::spawn(move || {
                semaphore
                    .acquire()
                    .expect("acquire should eventually succeed");
                1_u32
            })
        };

        thread::sleep(StdDuration::from_millis(10));
        semaphore.release(1).expect("release should wake waiter");
        assert_eq!(waiter.join().expect("waiter should finish"), 1);
    }

    #[test]
    fn linux_semaphore_release_many_wakes_multiple_waiters() {
        let semaphore = Arc::new(LinuxSemaphore::new(0, 4).expect("valid semaphore"));
        let completed = Arc::new(AtomicU32::new(0));
        let mut waiters = self::std::vec::Vec::new();

        for _ in 0..3 {
            let semaphore = Arc::clone(&semaphore);
            let completed = Arc::clone(&completed);
            waiters.push(thread::spawn(move || {
                semaphore
                    .acquire()
                    .expect("acquire should eventually succeed");
                completed.fetch_add(1, Ordering::Relaxed);
            }));
        }

        thread::sleep(StdDuration::from_millis(10));
        semaphore
            .release(3)
            .expect("release should wake all waiters");

        for waiter in waiters {
            waiter.join().expect("waiter should finish");
        }

        assert_eq!(completed.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn linux_semaphore_multi_threaded_permit_accounting_stays_balanced() {
        let semaphore = Arc::new(LinuxSemaphore::new(2, 2).expect("valid semaphore"));
        let in_critical = Arc::new(AtomicU32::new(0));
        let max_seen = Arc::new(AtomicU32::new(0));
        let mut threads = self::std::vec::Vec::new();

        for _ in 0..4 {
            let semaphore = Arc::clone(&semaphore);
            let in_critical = Arc::clone(&in_critical);
            let max_seen = Arc::clone(&max_seen);
            threads.push(thread::spawn(move || {
                for _ in 0..100 {
                    semaphore.acquire().expect("acquire should succeed");
                    let current = in_critical.fetch_add(1, Ordering::AcqRel) + 1;
                    update_max(&max_seen, current);
                    assert!(current <= 2, "permit count exceeded semaphore capacity");
                    in_critical.fetch_sub(1, Ordering::AcqRel);
                    semaphore.release(1).expect("release should succeed");
                }
            }));
        }

        for thread in threads {
            thread.join().expect("worker should finish");
        }

        assert!(max_seen.load(Ordering::Acquire) <= 2);
        assert_eq!(in_critical.load(Ordering::Acquire), 0);
        assert!(
            semaphore
                .try_acquire()
                .expect("permit should remain available")
        );
        assert!(
            semaphore
                .try_acquire()
                .expect("second permit should remain available")
        );
        assert!(
            !semaphore
                .try_acquire()
                .expect("no third permit should exist")
        );
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
