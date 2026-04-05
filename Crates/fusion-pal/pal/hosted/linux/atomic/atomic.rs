//! Linux fusion-pal atomic backend.
//!
//! Linux gives Fusion one straightforward story here: ordinary 32-bit atomic operations are
//! native through the target's Rust atomic surface, and wait/wake over one 32-bit word maps
//! directly onto process-local futex operations.

use core::convert::TryFrom;
use core::sync::atomic::{
    AtomicU32,
    Ordering,
};
use core::time::Duration;

use rustix::io::Errno;
use rustix::thread::futex;

use crate::contract::pal::runtime::atomic::{
    AtomicBaseContract,
    AtomicCompareExchangeOutcome32,
    AtomicError,
    AtomicErrorKind,
    AtomicFallbackKind,
    AtomicImplementationKind,
    AtomicScopeSupport,
    AtomicSupport,
    AtomicTimeoutCaps,
    AtomicWaitOutcome,
    AtomicWaitWord32Caps,
    AtomicWaitWord32Support,
    AtomicWord32Contract,
    AtomicWord32Caps,
    AtomicWord32Support,
};

const LINUX_WORD32_SUPPORT: AtomicWord32Support = AtomicWord32Support {
    caps: AtomicWord32Caps::LOAD
        .union(AtomicWord32Caps::STORE)
        .union(AtomicWord32Caps::SWAP)
        .union(AtomicWord32Caps::COMPARE_EXCHANGE)
        .union(AtomicWord32Caps::FETCH_ADD)
        .union(AtomicWord32Caps::FETCH_SUB)
        .union(AtomicWord32Caps::FETCH_AND)
        .union(AtomicWord32Caps::FETCH_OR)
        .union(AtomicWord32Caps::FETCH_XOR)
        .union(AtomicWord32Caps::STATIC_INIT),
    implementation: AtomicImplementationKind::Native,
    fallback: AtomicFallbackKind::None,
};

const LINUX_WAIT_WORD32_SUPPORT: AtomicWaitWord32Support = AtomicWaitWord32Support {
    caps: AtomicWaitWord32Caps::WAIT_WHILE_EQUAL
        .union(AtomicWaitWord32Caps::WAKE_ONE)
        .union(AtomicWaitWord32Caps::WAKE_ALL)
        .union(AtomicWaitWord32Caps::SPURIOUS_WAKE),
    timeout: AtomicTimeoutCaps::RELATIVE.union(AtomicTimeoutCaps::RELATIVE_MONOTONIC),
    scope: AtomicScopeSupport::LocalOnly,
    implementation: AtomicImplementationKind::Native,
    fallback: AtomicFallbackKind::None,
};

/// Linux atomic provider handle.
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxAtomic;

/// Linux 32-bit atomic word backed by the target's native atomic surface.
#[derive(Debug)]
pub struct LinuxAtomicWord32 {
    inner: AtomicU32,
}

/// Target-selected atomic provider alias for Linux builds.
pub type PlatformAtomic = LinuxAtomic;

/// Selected 32-bit atomic word type for Linux builds.
pub type PlatformAtomicWord32 = LinuxAtomicWord32;

/// Backend truth for the selected 32-bit atomic-word implementation on Linux.
pub const PLATFORM_ATOMIC_WORD32_IMPLEMENTATION: AtomicImplementationKind =
    AtomicImplementationKind::Native;

/// Backend truth for the selected 32-bit atomic wait/wake implementation on Linux.
pub const PLATFORM_ATOMIC_WAIT_WORD32_IMPLEMENTATION: AtomicImplementationKind =
    AtomicImplementationKind::Native;

/// Returns the process-wide Linux atomic provider handle.
#[must_use]
pub const fn system_atomic() -> PlatformAtomic {
    PlatformAtomic::new()
}

impl LinuxAtomic {
    /// Creates a new Linux atomic provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl AtomicBaseContract for LinuxAtomic {
    type Word32 = LinuxAtomicWord32;

    fn support(&self) -> AtomicSupport {
        AtomicSupport {
            word32: LINUX_WORD32_SUPPORT,
            wait_word32: LINUX_WAIT_WORD32_SUPPORT,
        }
    }

    fn new_word32(&self, initial: u32) -> Result<Self::Word32, AtomicError> {
        Ok(LinuxAtomicWord32::new(initial))
    }
}

impl LinuxAtomicWord32 {
    /// Creates a new Linux 32-bit atomic word.
    #[must_use]
    pub const fn new(initial: u32) -> Self {
        Self {
            inner: AtomicU32::new(initial),
        }
    }
}

impl Default for LinuxAtomicWord32 {
    fn default() -> Self {
        Self::new(0)
    }
}

impl AtomicWord32Contract for LinuxAtomicWord32 {
    fn support(&self) -> AtomicWord32Support {
        LINUX_WORD32_SUPPORT
    }

    fn wait_support(&self) -> AtomicWaitWord32Support {
        LINUX_WAIT_WORD32_SUPPORT
    }

    fn load(&self, ordering: Ordering) -> Result<u32, AtomicError> {
        validate_load_ordering(ordering)?;
        Ok(self.inner.load(ordering))
    }

    fn store(&self, value: u32, ordering: Ordering) -> Result<(), AtomicError> {
        validate_store_ordering(ordering)?;
        self.inner.store(value, ordering);
        Ok(())
    }

    fn swap(&self, value: u32, ordering: Ordering) -> Result<u32, AtomicError> {
        validate_rmw_ordering(ordering)?;
        Ok(self.inner.swap(value, ordering))
    }

    fn compare_exchange(
        &self,
        current: u32,
        new: u32,
        success: Ordering,
        failure: Ordering,
    ) -> Result<AtomicCompareExchangeOutcome32, AtomicError> {
        validate_compare_exchange_orderings(success, failure)?;
        Ok(
            match self.inner.compare_exchange(current, new, success, failure) {
                Ok(_) => AtomicCompareExchangeOutcome32::Exchanged,
                Err(observed) => AtomicCompareExchangeOutcome32::Mismatch(observed),
            },
        )
    }

    fn fetch_add(&self, value: u32, ordering: Ordering) -> Result<u32, AtomicError> {
        validate_rmw_ordering(ordering)?;
        Ok(self.inner.fetch_add(value, ordering))
    }

    fn fetch_sub(&self, value: u32, ordering: Ordering) -> Result<u32, AtomicError> {
        validate_rmw_ordering(ordering)?;
        Ok(self.inner.fetch_sub(value, ordering))
    }

    fn fetch_and(&self, value: u32, ordering: Ordering) -> Result<u32, AtomicError> {
        validate_rmw_ordering(ordering)?;
        Ok(self.inner.fetch_and(value, ordering))
    }

    fn fetch_or(&self, value: u32, ordering: Ordering) -> Result<u32, AtomicError> {
        validate_rmw_ordering(ordering)?;
        Ok(self.inner.fetch_or(value, ordering))
    }

    fn fetch_xor(&self, value: u32, ordering: Ordering) -> Result<u32, AtomicError> {
        validate_rmw_ordering(ordering)?;
        Ok(self.inner.fetch_xor(value, ordering))
    }

    fn wait_while_equal(
        &self,
        expected: u32,
        timeout: Option<Duration>,
    ) -> Result<AtomicWaitOutcome, AtomicError> {
        futex_wait_private(&self.inner, expected, timeout)
    }

    fn wake_one(&self) -> Result<usize, AtomicError> {
        futex::wake(&self.inner, futex::Flags::PRIVATE, 1).map_err(map_errno)
    }

    fn wake_all(&self) -> Result<usize, AtomicError> {
        futex::wake(&self.inner, futex::Flags::PRIVATE, u32::MAX).map_err(map_errno)
    }
}

fn futex_wait_private(
    word: &AtomicU32,
    expected: u32,
    timeout: Option<Duration>,
) -> Result<AtomicWaitOutcome, AtomicError> {
    let timeout_storage = duration_to_timespec(timeout)?;
    match futex::wait(
        word,
        futex::Flags::PRIVATE,
        expected,
        timeout_storage.as_ref(),
    ) {
        Ok(()) => Ok(AtomicWaitOutcome::Woken),
        Err(Errno::AGAIN) => Ok(AtomicWaitOutcome::Mismatch),
        Err(Errno::TIMEDOUT) => Ok(AtomicWaitOutcome::TimedOut),
        Err(Errno::INTR) => Ok(AtomicWaitOutcome::Interrupted),
        Err(errno) => Err(map_errno(errno)),
    }
}

fn duration_to_timespec(timeout: Option<Duration>) -> Result<Option<futex::Timespec>, AtomicError> {
    timeout
        .map(|duration| {
            let secs = i64::try_from(duration.as_secs()).map_err(|_| AtomicError::overflow())?;
            let nsecs = i64::from(duration.subsec_nanos());
            Ok(futex::Timespec {
                tv_sec: secs,
                tv_nsec: nsecs,
            })
        })
        .transpose()
}

const fn map_errno(errno: Errno) -> AtomicError {
    match errno {
        Errno::INVAL => AtomicError::invalid(),
        Errno::AGAIN | Errno::BUSY => AtomicError::busy(),
        Errno::PERM | Errno::ACCESS => AtomicError::permission_denied(),
        _ => AtomicError {
            kind: AtomicErrorKind::Platform(errno.raw_os_error()),
        },
    }
}

const fn validate_load_ordering(ordering: Ordering) -> Result<(), AtomicError> {
    match ordering {
        Ordering::Relaxed | Ordering::Acquire | Ordering::SeqCst => Ok(()),
        Ordering::Release | Ordering::AcqRel => Err(AtomicError::invalid()),
        _ => Err(AtomicError::invalid()),
    }
}

const fn validate_store_ordering(ordering: Ordering) -> Result<(), AtomicError> {
    match ordering {
        Ordering::Relaxed | Ordering::Release | Ordering::SeqCst => Ok(()),
        Ordering::Acquire | Ordering::AcqRel => Err(AtomicError::invalid()),
        _ => Err(AtomicError::invalid()),
    }
}

const fn validate_rmw_ordering(ordering: Ordering) -> Result<(), AtomicError> {
    match ordering {
        Ordering::Relaxed
        | Ordering::Acquire
        | Ordering::Release
        | Ordering::AcqRel
        | Ordering::SeqCst => Ok(()),
        _ => Err(AtomicError::invalid()),
    }
}

const fn validate_compare_exchange_orderings(
    success: Ordering,
    failure: Ordering,
) -> Result<(), AtomicError> {
    if validate_rmw_ordering(success).is_err() {
        return Err(AtomicError::invalid());
    }

    match (success, failure) {
        (Ordering::Relaxed, Ordering::Relaxed)
        | (Ordering::Acquire, Ordering::Relaxed | Ordering::Acquire)
        | (Ordering::Release, Ordering::Relaxed)
        | (Ordering::AcqRel, Ordering::Relaxed | Ordering::Acquire)
        | (Ordering::SeqCst, Ordering::Relaxed | Ordering::Acquire | Ordering::SeqCst) => Ok(()),
        _ => Err(AtomicError::invalid()),
    }
}

#[cfg(all(test, feature = "std", not(target_os = "none")))]
mod tests {
    use super::*;
    extern crate std;
    use self::std::sync::Arc;
    use self::std::thread;
    use self::std::time::Duration as StdDuration;

    #[test]
    fn linux_atomic_word32_compare_exchange_reports_mismatch() {
        let word = LinuxAtomicWord32::new(7);
        assert_eq!(
            word.compare_exchange(3, 9, Ordering::AcqRel, Ordering::Acquire)
                .expect("compare_exchange should succeed structurally"),
            AtomicCompareExchangeOutcome32::Mismatch(7)
        );
        assert_eq!(word.load(Ordering::Acquire).expect("load should work"), 7);
    }

    #[test]
    fn linux_atomic_word32_wait_wakes() {
        let word = Arc::new(LinuxAtomicWord32::new(1));
        let waiter = {
            let word = Arc::clone(&word);
            thread::spawn(move || {
                word.wait_while_equal(1, Some(StdDuration::from_secs(1)))
                    .expect("wait should succeed")
            })
        };

        thread::sleep(StdDuration::from_millis(10));
        word.store(2, Ordering::Release)
            .expect("store should succeed");
        word.wake_all().expect("wake should succeed");

        let outcome = waiter.join().expect("waiter should finish");
        assert!(matches!(
            outcome,
            AtomicWaitOutcome::Woken | AtomicWaitOutcome::Mismatch
        ));
    }
}
