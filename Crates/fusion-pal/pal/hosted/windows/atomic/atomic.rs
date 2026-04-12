//! Windows fusion-pal atomic backend.
//!
//! This backend uses the target's native 32-bit atomic surface together with the Windows 10
//! `WaitOnAddress` / `WakeByAddress*` primitives. The wait/wake scope is reported conservatively
//! as process-local.

use core::ffi::c_void;
use core::mem::size_of;
use core::sync::atomic::{
    AtomicU32,
    AtomicUsize,
    Ordering,
};
use core::time::Duration;

use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED,
    ERROR_INVALID_PARAMETER,
    ERROR_NOT_ENOUGH_MEMORY,
    ERROR_OUTOFMEMORY,
    ERROR_TIMEOUT,
    WIN32_ERROR,
};
use windows::Win32::System::Threading::{
    INFINITE,
    WaitOnAddress,
    WakeByAddressAll,
    WakeByAddressSingle,
};

use crate::contract::pal::runtime::atomic::{
    AtomicBaseContract,
    AtomicCompareExchangeOutcome32,
    AtomicError,
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

const WINDOWS_WORD32_SUPPORT: AtomicWord32Support = AtomicWord32Support {
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

const WINDOWS_WAIT_WORD32_SUPPORT: AtomicWaitWord32Support = AtomicWaitWord32Support {
    caps: AtomicWaitWord32Caps::WAIT_WHILE_EQUAL
        .union(AtomicWaitWord32Caps::WAKE_ONE)
        .union(AtomicWaitWord32Caps::WAKE_ALL)
        .union(AtomicWaitWord32Caps::SPURIOUS_WAKE),
    timeout: AtomicTimeoutCaps::RELATIVE,
    scope: AtomicScopeSupport::LocalOnly,
    implementation: AtomicImplementationKind::Native,
    fallback: AtomicFallbackKind::None,
};

/// Windows atomic provider handle.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsAtomic;

/// Windows 32-bit atomic word backed by `AtomicU32` plus `WaitOnAddress`.
#[derive(Debug)]
pub struct WindowsAtomicWord32 {
    inner: AtomicU32,
    waiters: AtomicUsize,
}

/// Target-selected atomic provider alias for Windows builds.
pub type PlatformAtomic = WindowsAtomic;

/// Selected 32-bit atomic word type for Windows builds.
pub type PlatformAtomicWord32 = WindowsAtomicWord32;

/// Backend truth for the selected 32-bit atomic-word implementation on Windows.
pub const PLATFORM_ATOMIC_WORD32_IMPLEMENTATION: AtomicImplementationKind =
    AtomicImplementationKind::Native;

/// Backend truth for the selected 32-bit atomic wait/wake implementation on Windows.
pub const PLATFORM_ATOMIC_WAIT_WORD32_IMPLEMENTATION: AtomicImplementationKind =
    AtomicImplementationKind::Native;

/// Returns the process-wide Windows atomic provider handle.
#[must_use]
pub const fn system_atomic() -> PlatformAtomic {
    PlatformAtomic::new()
}

impl WindowsAtomic {
    /// Creates a new Windows atomic provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl AtomicBaseContract for WindowsAtomic {
    type Word32 = WindowsAtomicWord32;

    fn support(&self) -> AtomicSupport {
        AtomicSupport {
            word32: WINDOWS_WORD32_SUPPORT,
            wait_word32: WINDOWS_WAIT_WORD32_SUPPORT,
        }
    }

    fn new_word32(&self, initial: u32) -> Result<Self::Word32, AtomicError> {
        Ok(WindowsAtomicWord32::new(initial))
    }
}

impl WindowsAtomicWord32 {
    /// Creates a new Windows 32-bit atomic word.
    #[must_use]
    pub const fn new(initial: u32) -> Self {
        Self {
            inner: AtomicU32::new(initial),
            waiters: AtomicUsize::new(0),
        }
    }
}

impl Default for WindowsAtomicWord32 {
    fn default() -> Self {
        Self::new(0)
    }
}

impl AtomicWord32Contract for WindowsAtomicWord32 {
    fn support(&self) -> AtomicWord32Support {
        WINDOWS_WORD32_SUPPORT
    }

    fn wait_support(&self) -> AtomicWaitWord32Support {
        WINDOWS_WAIT_WORD32_SUPPORT
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
        if self.inner.load(Ordering::Acquire) != expected {
            return Ok(AtomicWaitOutcome::Mismatch);
        }

        let compare = expected;
        self.waiters.fetch_add(1, Ordering::AcqRel);
        let wait_result = unsafe {
            WaitOnAddress(
                (&raw const self.inner).cast::<c_void>(),
                (&raw const compare).cast::<c_void>(),
                size_of::<u32>(),
                Some(timeout_to_wait_ms(timeout)?),
            )
        };
        self.waiters.fetch_sub(1, Ordering::AcqRel);

        match wait_result {
            Ok(()) => {
                if self.inner.load(Ordering::Acquire) != expected {
                    Ok(AtomicWaitOutcome::Mismatch)
                } else {
                    Ok(AtomicWaitOutcome::Woken)
                }
            }
            Err(error) => {
                let mapped = map_hresult(error.code().0);
                if matches!(mapped.1, Some(AtomicWaitOutcome::TimedOut)) {
                    Ok(AtomicWaitOutcome::TimedOut)
                } else {
                    Err(mapped.0)
                }
            }
        }
    }

    fn wake_one(&self) -> Result<usize, AtomicError> {
        let observed = self.waiters.load(Ordering::Acquire);
        if observed == 0 {
            return Ok(0);
        }
        unsafe { WakeByAddressSingle((&raw const self.inner).cast::<c_void>()) };
        Ok(1)
    }

    fn wake_all(&self) -> Result<usize, AtomicError> {
        let observed = self.waiters.load(Ordering::Acquire);
        if observed == 0 {
            return Ok(0);
        }
        unsafe { WakeByAddressAll((&raw const self.inner).cast::<c_void>()) };
        Ok(observed)
    }
}

fn timeout_to_wait_ms(timeout: Option<Duration>) -> Result<u32, AtomicError> {
    match timeout {
        None => Ok(INFINITE),
        Some(duration) => {
            let millis = duration.as_millis();
            let rounded = if millis == 0 && duration.subsec_nanos() != 0 {
                1
            } else {
                millis
            };
            u32::try_from(rounded).map_err(|_| AtomicError::invalid())
        }
    }
}

const fn validate_load_ordering(ordering: Ordering) -> Result<(), AtomicError> {
    match ordering {
        Ordering::Relaxed | Ordering::Acquire | Ordering::SeqCst => Ok(()),
        _ => Err(AtomicError::invalid()),
    }
}

const fn validate_store_ordering(ordering: Ordering) -> Result<(), AtomicError> {
    match ordering {
        Ordering::Relaxed | Ordering::Release | Ordering::SeqCst => Ok(()),
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

const fn map_win32_error(error: WIN32_ERROR) -> (AtomicError, Option<AtomicWaitOutcome>) {
    match error {
        ERROR_INVALID_PARAMETER => (AtomicError::invalid(), None),
        ERROR_ACCESS_DENIED => (AtomicError::permission_denied(), None),
        ERROR_TIMEOUT => (
            AtomicError::platform(ERROR_TIMEOUT.0 as i32),
            Some(AtomicWaitOutcome::TimedOut),
        ),
        ERROR_NOT_ENOUGH_MEMORY | ERROR_OUTOFMEMORY => (AtomicError::busy(), None),
        _ => (AtomicError::platform(error.0 as i32), None),
    }
}

const fn map_hresult(code: i32) -> (AtomicError, Option<AtomicWaitOutcome>) {
    let raw = code as u32;
    let facility = (raw >> 16) & 0x1fff;
    if facility == 7 {
        return map_win32_error(WIN32_ERROR(raw & 0xffff));
    }
    (AtomicError::platform(code), None)
}

#[cfg(all(test, feature = "std", target_os = "windows"))]
mod tests {
    use super::*;

    extern crate std;
    use self::std::sync::Arc;
    use self::std::thread;
    use self::std::time::Duration as StdDuration;

    #[test]
    fn windows_atomic_support_reports_native_word_and_wait() {
        let support = system_atomic().support();

        assert_eq!(
            support.word32.implementation,
            AtomicImplementationKind::Native
        );
        assert_eq!(
            support.wait_word32.implementation,
            AtomicImplementationKind::Native
        );
        assert!(support.word32.caps.contains(
            AtomicWord32Caps::LOAD
                | AtomicWord32Caps::STORE
                | AtomicWord32Caps::COMPARE_EXCHANGE
                | AtomicWord32Caps::STATIC_INIT
        ));
        assert!(support.wait_word32.caps.contains(
            AtomicWaitWord32Caps::WAIT_WHILE_EQUAL
                | AtomicWaitWord32Caps::WAKE_ONE
                | AtomicWaitWord32Caps::WAKE_ALL
        ));
    }

    #[test]
    fn windows_atomic_compare_exchange_reports_mismatch() {
        let word = WindowsAtomicWord32::new(7);

        assert_eq!(
            word.compare_exchange(3, 9, Ordering::AcqRel, Ordering::Acquire)
                .expect("compare_exchange should succeed structurally"),
            AtomicCompareExchangeOutcome32::Mismatch(7)
        );
    }

    #[test]
    fn windows_atomic_wait_wakes() {
        let word = Arc::new(WindowsAtomicWord32::new(1));
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
