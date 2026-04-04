//! One-time initialization primitives built on top of the selected fusion-pal raw once backend.

use core::cell::UnsafeCell;
use core::convert::Infallible;
use core::mem::MaybeUninit;

use fusion_pal::sys::sync::{
    PlatformRawOnce,
    RawOnce,
};

use super::{
    OnceBeginResult,
    OnceState,
    OnceSupport,
    SyncError,
};

/// Error returned when one-time initialization fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnceInitError<E> {
    /// The underlying synchronization primitive failed.
    Sync(SyncError),
    /// The user-provided initializer returned an error.
    Init(E),
}

/// Small once primitive with no poisoning and explicit reset-on-failure behavior.
#[derive(Debug)]
pub struct Once {
    raw: PlatformRawOnce,
}

impl Once {
    /// Creates a new once primitive.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            raw: PlatformRawOnce::new(),
        }
    }

    /// Reports the support surface of the selected raw once backend.
    #[must_use]
    pub fn support(&self) -> OnceSupport {
        self.raw.support()
    }

    /// Returns the current once state.
    #[must_use]
    pub fn state(&self) -> OnceState {
        self.raw.state()
    }

    /// Returns whether initialization has completed successfully.
    #[must_use]
    pub fn is_completed(&self) -> bool {
        self.state() == OnceState::Complete
    }

    /// Runs `init` at most once across all callers.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying raw once primitive cannot coordinate honestly.
    pub fn call_once<F>(&self, init: F) -> Result<(), SyncError>
    where
        F: FnOnce(),
    {
        match self.call_once_try::<_, Infallible>(|| {
            init();
            Ok(())
        }) {
            Ok(()) => Ok(()),
            Err(OnceInitError::Sync(error)) => Err(error),
            Err(OnceInitError::Init(never)) => match never {},
        }
    }

    /// Runs `init` at most once across all callers, resetting on user error.
    ///
    /// # Errors
    ///
    /// Returns `OnceInitError::Sync` if synchronization fails, or `OnceInitError::Init`
    /// if the initializer itself reports a failure.
    pub fn call_once_try<F, E>(&self, init: F) -> Result<(), OnceInitError<E>>
    where
        F: FnOnce() -> Result<(), E>,
    {
        loop {
            match self.raw.begin().map_err(OnceInitError::Sync)? {
                OnceBeginResult::Complete => return Ok(()),
                OnceBeginResult::InProgress => {
                    self.raw.wait().map_err(OnceInitError::Sync)?;
                }
                OnceBeginResult::Initialize => {
                    let mut guard = OnceInitGuard::new(&self.raw);
                    init().map_err(OnceInitError::Init)?;
                    guard.complete();
                    return Ok(());
                }
            }
        }
    }
}

impl Default for Once {
    fn default() -> Self {
        Self::new()
    }
}

struct OnceInitGuard<'a> {
    raw: &'a PlatformRawOnce,
    completed: bool,
}

impl<'a> OnceInitGuard<'a> {
    const fn new(raw: &'a PlatformRawOnce) -> Self {
        Self {
            raw,
            completed: false,
        }
    }

    fn complete(&mut self) {
        // SAFETY: this guard exists only for the thread that successfully entered the
        // once primitive and has not yet completed or reset it.
        unsafe { self.raw.complete_unchecked() };
        self.completed = true;
    }
}

impl Drop for OnceInitGuard<'_> {
    fn drop(&mut self) {
        if !self.completed {
            // SAFETY: this guard only exists on the winning initializer path, so reset is
            // only issued by the thread that currently owns initialization.
            unsafe { self.raw.reset_unchecked() };
        }
    }
}

/// Single-assignment cell initialized via [`Once`].
pub struct OnceLock<T> {
    once: Once,
    value: UnsafeCell<MaybeUninit<T>>,
}

impl<T> OnceLock<T> {
    /// Creates a new empty once-initialized cell.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            once: Once::new(),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    /// Returns the initialized value if one is present.
    #[must_use]
    pub fn get(&self) -> Option<&T> {
        if self.once.is_completed() {
            // SAFETY: completion only happens after the initializer writes the value, and
            // completion is published with release semantics through the raw once primitive.
            Some(unsafe { (*self.value.get()).assume_init_ref() })
        } else {
            None
        }
    }

    /// Initializes the cell if needed and returns a shared reference to the value.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying once primitive cannot coordinate honestly.
    pub fn get_or_init<F>(&self, init: F) -> Result<&T, SyncError>
    where
        F: FnOnce() -> T,
    {
        match self.get_or_try_init::<_, Infallible>(|| Ok(init())) {
            Ok(value) => Ok(value),
            Err(OnceInitError::Sync(error)) => Err(error),
            Err(OnceInitError::Init(never)) => match never {},
        }
    }

    /// Initializes the cell if needed and returns a shared reference to the value.
    ///
    /// # Errors
    ///
    /// Returns `OnceInitError::Sync` if synchronization fails, or `OnceInitError::Init`
    /// if the initializer itself reports a failure.
    pub fn get_or_try_init<F, E>(&self, init: F) -> Result<&T, OnceInitError<E>>
    where
        F: FnOnce() -> Result<T, E>,
    {
        if let Some(value) = self.get() {
            return Ok(value);
        }

        let value_ptr = self.value.get();
        self.once.call_once_try(|| {
            let value = init()?;
            // SAFETY: the once primitive guarantees exclusive initialization for the
            // winning thread, and we only write before marking completion.
            unsafe { (*value_ptr).write(value) };
            Ok(())
        })?;

        self.get()
            .map_or_else(|| Err(OnceInitError::Sync(SyncError::invalid())), Ok)
    }
}

impl<T> Default for OnceLock<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Drop for OnceLock<T> {
    fn drop(&mut self) {
        if self.once.is_completed() {
            // SAFETY: completed state guarantees that the value was initialized exactly
            // once, and `&mut self` guarantees exclusive drop access.
            unsafe { self.value.get_mut().assume_init_drop() };
        }
    }
}

unsafe impl<T: Send> Send for OnceLock<T> {}
unsafe impl<T: Send + Sync> Sync for OnceLock<T> {}

#[cfg(all(test, feature = "std", not(target_os = "none")))]
mod tests {
    use super::*;
    use core::sync::atomic::{
        AtomicU32,
        Ordering,
    };
    extern crate std;
    use self::std::panic::{
        AssertUnwindSafe,
        catch_unwind,
    };
    use self::std::sync::Arc;
    use self::std::thread;

    #[test]
    fn once_runs_initializer_only_once_across_threads() {
        let once = Arc::new(Once::new());
        let runs = Arc::new(AtomicU32::new(0));
        let mut threads = self::std::vec::Vec::new();

        for _ in 0..6 {
            let once = Arc::clone(&once);
            let runs = Arc::clone(&runs);
            threads.push(thread::spawn(move || {
                once.call_once(|| {
                    runs.fetch_add(1, Ordering::AcqRel);
                })
                .expect("once should coordinate");
            }));
        }

        for thread in threads {
            thread.join().expect("worker should finish");
        }

        assert_eq!(runs.load(Ordering::Acquire), 1);
        assert!(once.is_completed());
    }

    #[test]
    fn once_resets_after_initializer_panic() {
        let once = Once::new();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _ = once.call_once(|| {
                panic!("initializer panic should reset once state");
            });
        }));
        assert!(result.is_err());
        assert_eq!(once.state(), OnceState::Uninitialized);

        once.call_once(|| {})
            .expect("once should remain reusable after panic reset");
        assert!(once.is_completed());
    }

    #[test]
    fn once_lock_retries_after_initializer_error() {
        let cell = OnceLock::new();
        let attempts = AtomicU32::new(0);

        let first = cell.get_or_try_init(|| {
            attempts.fetch_add(1, Ordering::AcqRel);
            Err::<u32, _>(7_u32)
        });
        assert_eq!(first, Err(OnceInitError::Init(7_u32)));
        assert!(cell.get().is_none());

        let value = cell
            .get_or_try_init(|| {
                attempts.fetch_add(1, Ordering::AcqRel);
                Ok::<u32, u32>(42)
            })
            .expect("second init should succeed");

        assert_eq!(*value, 42);
        assert_eq!(attempts.load(Ordering::Acquire), 2);
    }
}
