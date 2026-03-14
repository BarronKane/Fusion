//! Linux fusion-pal synchronization backend.
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
    MutexCaps, MutexSupport, OnceBeginResult, OnceCaps, OnceState, OnceSupport,
    PriorityInheritanceSupport, ProcessScopeSupport, RawMutex, RawOnce, RawRwLock, RawSemaphore,
    RecursionSupport, RobustnessSupport, RwLockCaps, RwLockFairnessSupport, RwLockSupport,
    SemaphoreCaps, SemaphoreSupport, SyncBase, SyncError, SyncErrorKind, SyncFallbackKind,
    SyncImplementationKind, SyncSupport, TimeoutCaps, WaitCaps, WaitOutcome, WaitPrimitive,
    WaitSupport,
};

const UNLOCKED: u32 = 0;
const LOCKED: u32 = 1;
const CONTENDED: u32 = 2;
const ONCE_UNINITIALIZED: u32 = 0;
const ONCE_RUNNING: u32 = 1;
const ONCE_COMPLETE: u32 = 2;

const LINUX_WAIT_SUPPORT: WaitSupport = WaitSupport {
    caps: WaitCaps::WAIT_WHILE_EQUAL
        .union(WaitCaps::WAKE_ONE)
        .union(WaitCaps::WAKE_ALL)
        .union(WaitCaps::SPURIOUS_WAKE),
    timeout: TimeoutCaps::RELATIVE.union(TimeoutCaps::RELATIVE_MONOTONIC),
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Native,
    fallback: SyncFallbackKind::None,
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
    fallback: SyncFallbackKind::None,
};

const LINUX_SEMAPHORE_SUPPORT: SemaphoreSupport = SemaphoreSupport {
    caps: SemaphoreCaps::TRY_ACQUIRE
        .union(SemaphoreCaps::BLOCKING)
        .union(SemaphoreCaps::RELEASE_MANY),
    timeout: TimeoutCaps::empty(),
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Emulated,
    fallback: SyncFallbackKind::None,
};

const LINUX_ONCE_SUPPORT: OnceSupport = OnceSupport {
    caps: OnceCaps::WAITING
        .union(OnceCaps::STATIC_INIT)
        .union(OnceCaps::RESET_ON_FAILURE),
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Emulated,
    fallback: SyncFallbackKind::None,
};

const LINUX_RWLOCK_SUPPORT: RwLockSupport = RwLockSupport {
    caps: RwLockCaps::TRY_READ
        .union(RwLockCaps::TRY_WRITE)
        .union(RwLockCaps::BLOCKING_READ)
        .union(RwLockCaps::BLOCKING_WRITE)
        .union(RwLockCaps::STATIC_INIT),
    timeout: TimeoutCaps::empty(),
    fairness: RwLockFairnessSupport::WriterPreferred,
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Emulated,
    fallback: SyncFallbackKind::None,
};

/// Linux synchronization provider handle.
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxSync;

/// Selected raw mutex type for Linux builds.
pub type PlatformRawMutex = LinuxRawMutex;

/// Selected semaphore type for Linux builds.
pub type PlatformSemaphore = LinuxSemaphore;

/// Selected raw once type for Linux builds.
pub type PlatformRawOnce = LinuxRawOnce;

/// Selected raw rwlock type for Linux builds.
pub type PlatformRawRwLock = LinuxRawRwLock;

/// Target-selected synchronization provider alias for Linux builds.
pub type PlatformSync = LinuxSync;

/// Backend truth for the selected raw mutex implementation on Linux.
pub const PLATFORM_RAW_MUTEX_IMPLEMENTATION: SyncImplementationKind =
    SyncImplementationKind::Native;

/// Backend truth for the selected raw once implementation on Linux.
pub const PLATFORM_RAW_ONCE_IMPLEMENTATION: SyncImplementationKind =
    SyncImplementationKind::Emulated;

/// Backend truth for the selected raw rwlock implementation on Linux.
pub const PLATFORM_RAW_RWLOCK_IMPLEMENTATION: SyncImplementationKind =
    SyncImplementationKind::Emulated;

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
            once: LINUX_ONCE_SUPPORT,
            semaphore: LINUX_SEMAPHORE_SUPPORT,
            rwlock: LINUX_RWLOCK_SUPPORT,
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

/// Linux raw once primitive backed by a futex word.
#[derive(Debug)]
pub struct LinuxRawOnce {
    state: AtomicU32,
}

impl LinuxRawOnce {
    /// Creates a new uninitialized Linux raw once primitive.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: AtomicU32::new(ONCE_UNINITIALIZED),
        }
    }
}

impl RawOnce for LinuxRawOnce {
    fn support(&self) -> OnceSupport {
        LINUX_ONCE_SUPPORT
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
        loop {
            match self.state.load(Ordering::Acquire) {
                ONCE_COMPLETE | ONCE_UNINITIALIZED => return Ok(()),
                ONCE_RUNNING => match futex_wait_private(&self.state, ONCE_RUNNING, None)? {
                    WaitOutcome::Woken
                    | WaitOutcome::Mismatch
                    | WaitOutcome::Interrupted
                    | WaitOutcome::TimedOut => {}
                },
                _ => return Err(SyncError::invalid()),
            }
        }
    }

    unsafe fn complete_unchecked(&self) {
        self.state.store(ONCE_COMPLETE, Ordering::Release);
        let _ = futex::wake(&self.state, futex::Flags::PRIVATE, u32::MAX);
    }

    unsafe fn reset_unchecked(&self) {
        self.state.store(ONCE_UNINITIALIZED, Ordering::Release);
        let _ = futex::wake(&self.state, futex::Flags::PRIVATE, u32::MAX);
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
                    CONTENDED,
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
                match self.state.compare_exchange_weak(
                    LOCKED,
                    CONTENDED,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => {}
                    Err(observed) => {
                        state = observed;
                        continue;
                    }
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

/// Linux raw rwlock built from a local mutex plus futex-backed epoch waits.
#[derive(Debug)]
pub struct LinuxRawRwLock {
    gate: LinuxRawMutex,
    epoch: AtomicU32,
    readers: AtomicU32,
    waiting_writers: AtomicU32,
    writer_active: AtomicU32,
}

impl LinuxRawRwLock {
    /// Creates a new unlocked Linux raw rwlock.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            gate: LinuxRawMutex::new(),
            epoch: AtomicU32::new(0),
            readers: AtomicU32::new(0),
            waiting_writers: AtomicU32::new(0),
            writer_active: AtomicU32::new(0),
        }
    }

    fn wake_waiters(&self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
        let _ = futex::wake(&self.epoch, futex::Flags::PRIVATE, u32::MAX);
    }

    fn lock_gate(&self) -> Result<(), SyncError> {
        self.gate.lock()
    }

    unsafe fn unlock_gate_unchecked(&self) {
        // SAFETY: callers only route here immediately after a successful `lock_gate`.
        unsafe { self.gate.unlock_unchecked() };
    }

    fn lock_gate_for_release_assumed_infallible(&self) {
        // The internal gate is a LinuxRawMutex over a valid process-local futex word that
        // this type constructed itself. On this backend, treating acquisition here as
        // infallible is the contract assumption that keeps release paths compatible with
        // `unsafe fn ..._unlock_unchecked` and guard `Drop`. If a future backend cannot
        // uphold that assumption, the release protocol needs a different design rather than
        // pretending an error can be surfaced from a `Drop`-driven unlock path.
        let _ = self.gate.lock();
    }
}

struct WaitingWriterGuard<'a> {
    lock: &'a LinuxRawRwLock,
}

impl<'a> WaitingWriterGuard<'a> {
    fn new(lock: &'a LinuxRawRwLock) -> Self {
        lock.waiting_writers.fetch_add(1, Ordering::Relaxed);
        Self { lock }
    }
}

impl Drop for WaitingWriterGuard<'_> {
    fn drop(&mut self) {
        // Waiting-writer accounting is part of the same internal release path story as the
        // unlock helpers above: once the writer has joined the wait set, backing that count
        // out must not be skipped simply because the internal gate reported an impossible
        // failure on this Linux backend.
        self.lock.lock_gate_for_release_assumed_infallible();
        self.lock.waiting_writers.fetch_sub(1, Ordering::Relaxed);
        // SAFETY: `lock_gate_for_release_assumed_infallible` treats the gate as held under
        // the same backend-local invariant used by the rwlock release paths.
        unsafe { self.lock.unlock_gate_unchecked() };
    }
}

// SAFETY: `LinuxRawRwLock` serializes state transitions through an internal raw mutex and
// uses acquire/release operations for read and write ownership publication.
unsafe impl RawRwLock for LinuxRawRwLock {
    fn support(&self) -> RwLockSupport {
        LINUX_RWLOCK_SUPPORT
    }

    fn read_lock(&self) -> Result<(), SyncError> {
        loop {
            self.lock_gate()?;
            if self.writer_active.load(Ordering::Relaxed) == 0
                && self.waiting_writers.load(Ordering::Relaxed) == 0
            {
                self.readers.fetch_add(1, Ordering::Acquire);
                // SAFETY: this thread currently holds the gate mutex.
                unsafe { self.unlock_gate_unchecked() };
                return Ok(());
            }
            let observed = self.epoch.load(Ordering::Acquire);
            // SAFETY: this thread currently holds the gate mutex.
            unsafe { self.unlock_gate_unchecked() };
            match futex_wait_private(&self.epoch, observed, None)? {
                WaitOutcome::Woken
                | WaitOutcome::Mismatch
                | WaitOutcome::Interrupted
                | WaitOutcome::TimedOut => {}
            }
        }
    }

    fn try_read_lock(&self) -> Result<bool, SyncError> {
        self.lock_gate()?;
        let acquired = if self.writer_active.load(Ordering::Relaxed) == 0
            && self.waiting_writers.load(Ordering::Relaxed) == 0
        {
            self.readers.fetch_add(1, Ordering::Acquire);
            true
        } else {
            false
        };
        // SAFETY: this thread currently holds the gate mutex.
        unsafe { self.unlock_gate_unchecked() };
        Ok(acquired)
    }

    fn write_lock(&self) -> Result<(), SyncError> {
        loop {
            self.lock_gate()?;
            if self.writer_active.load(Ordering::Relaxed) == 0
                && self.readers.load(Ordering::Relaxed) == 0
            {
                self.writer_active.store(1, Ordering::Relaxed);
                // SAFETY: this thread currently holds the gate mutex.
                unsafe { self.unlock_gate_unchecked() };
                return Ok(());
            }

            let waiting_writer = WaitingWriterGuard::new(self);
            let observed = self.epoch.load(Ordering::Acquire);
            // SAFETY: this thread currently holds the gate mutex.
            unsafe { self.unlock_gate_unchecked() };
            let wait_result = futex_wait_private(&self.epoch, observed, None);
            drop(waiting_writer);
            match wait_result? {
                WaitOutcome::Woken
                | WaitOutcome::Mismatch
                | WaitOutcome::Interrupted
                | WaitOutcome::TimedOut => {}
            }
        }
    }

    fn try_write_lock(&self) -> Result<bool, SyncError> {
        self.lock_gate()?;
        let acquired = if self.writer_active.load(Ordering::Relaxed) == 0
            && self.readers.load(Ordering::Relaxed) == 0
        {
            self.writer_active.store(1, Ordering::Relaxed);
            true
        } else {
            false
        };
        // SAFETY: this thread currently holds the gate mutex.
        unsafe { self.unlock_gate_unchecked() };
        Ok(acquired)
    }

    unsafe fn read_unlock_unchecked(&self) {
        self.lock_gate_for_release_assumed_infallible();
        let remaining = self
            .readers
            .fetch_sub(1, Ordering::Release)
            .saturating_sub(1);
        let should_wake = remaining == 0 && self.waiting_writers.load(Ordering::Relaxed) != 0;
        // SAFETY: this thread currently holds the gate mutex.
        unsafe { self.unlock_gate_unchecked() };
        if should_wake {
            self.wake_waiters();
        }
    }

    unsafe fn write_unlock_unchecked(&self) {
        self.lock_gate_for_release_assumed_infallible();
        self.writer_active.store(0, Ordering::Release);
        // SAFETY: this thread currently holds the gate mutex.
        unsafe { self.unlock_gate_unchecked() };
        self.wake_waiters();
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
    fn linux_raw_mutex_wakes_waiter_chain_under_contention() {
        let lock = Arc::new(LinuxRawMutex::new());
        let completed = Arc::new(AtomicU32::new(0));
        lock.lock().expect("main thread should lock mutex");

        let mut waiters = self::std::vec::Vec::new();
        for _ in 0..3 {
            let lock = Arc::clone(&lock);
            let completed = Arc::clone(&completed);
            waiters.push(thread::spawn(move || {
                lock.lock().expect("waiter should acquire mutex");
                completed.fetch_add(1, Ordering::Relaxed);
                // SAFETY: this thread currently holds the mutex.
                unsafe { lock.unlock_unchecked() };
            }));
        }

        thread::sleep(StdDuration::from_millis(10));
        // SAFETY: the main thread currently holds the mutex.
        unsafe { lock.unlock_unchecked() };

        for waiter in waiters {
            waiter.join().expect("waiter should finish");
        }

        assert_eq!(completed.load(Ordering::Relaxed), 3);
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

    #[test]
    fn linux_raw_once_initializes_only_once_across_threads() {
        let once = Arc::new(LinuxRawOnce::new());
        let runs = Arc::new(AtomicU32::new(0));
        let mut threads = self::std::vec::Vec::new();

        for _ in 0..6 {
            let once = Arc::clone(&once);
            let runs = Arc::clone(&runs);
            threads.push(thread::spawn(move || {
                loop {
                    match once.begin().expect("begin should succeed") {
                        OnceBeginResult::Initialize => {
                            runs.fetch_add(1, Ordering::AcqRel);
                            // SAFETY: this thread won initialization and completes exactly once.
                            unsafe { once.complete_unchecked() };
                            break;
                        }
                        OnceBeginResult::InProgress => {
                            once.wait().expect("wait should succeed");
                        }
                        OnceBeginResult::Complete => break,
                    }
                }
            }));
        }

        for thread in threads {
            thread.join().expect("worker should finish");
        }

        assert_eq!(runs.load(Ordering::Acquire), 1);
        assert_eq!(once.state(), OnceState::Complete);
    }

    #[test]
    fn linux_raw_once_reset_allows_retry() {
        let once = LinuxRawOnce::new();
        assert_eq!(
            once.begin().expect("begin should succeed"),
            OnceBeginResult::Initialize
        );
        // SAFETY: this thread won initialization and is intentionally resetting it.
        unsafe { once.reset_unchecked() };
        assert_eq!(once.state(), OnceState::Uninitialized);
        assert_eq!(
            once.begin().expect("second begin should succeed"),
            OnceBeginResult::Initialize
        );
        // SAFETY: this thread again owns initialization and completes it exactly once.
        unsafe { once.complete_unchecked() };
        assert_eq!(once.state(), OnceState::Complete);
    }

    #[test]
    fn linux_raw_rwlock_allows_multiple_readers() {
        let lock = Arc::new(LinuxRawRwLock::new());
        let active = Arc::new(AtomicU32::new(0));
        let max_seen = Arc::new(AtomicU32::new(0));
        let mut threads = self::std::vec::Vec::new();

        for _ in 0..4 {
            let lock = Arc::clone(&lock);
            let active = Arc::clone(&active);
            let max_seen = Arc::clone(&max_seen);
            threads.push(thread::spawn(move || {
                lock.read_lock().expect("read lock should succeed");
                let current = active.fetch_add(1, Ordering::AcqRel) + 1;
                update_max(&max_seen, current);
                thread::sleep(StdDuration::from_millis(5));
                active.fetch_sub(1, Ordering::AcqRel);
                // SAFETY: this thread currently holds a matching read lock.
                unsafe { lock.read_unlock_unchecked() };
            }));
        }

        for thread in threads {
            thread.join().expect("reader should finish");
        }

        assert!(max_seen.load(Ordering::Acquire) >= 2);
    }

    #[test]
    fn linux_raw_rwlock_writer_blocks_new_readers_when_waiting() {
        let lock = Arc::new(LinuxRawRwLock::new());
        let writer_acquired = Arc::new(AtomicU32::new(0));

        lock.read_lock().expect("initial read lock should succeed");
        let writer = {
            let lock = Arc::clone(&lock);
            let writer_acquired = Arc::clone(&writer_acquired);
            thread::spawn(move || {
                lock.write_lock().expect("writer should eventually acquire");
                writer_acquired.store(1, Ordering::Release);
                // SAFETY: this thread currently holds the write lock.
                unsafe { lock.write_unlock_unchecked() };
            })
        };

        thread::sleep(StdDuration::from_millis(10));
        assert!(
            !lock.try_read_lock().expect("try_read should evaluate"),
            "new readers should not barge ahead of a waiting writer"
        );
        // SAFETY: the current thread holds the initial read lock.
        unsafe { lock.read_unlock_unchecked() };

        writer.join().expect("writer should finish");
        assert_eq!(writer_acquired.load(Ordering::Acquire), 1);
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
