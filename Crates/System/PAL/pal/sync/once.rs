//! Backend-neutral PAL contract for one-time initialization primitives.

use super::{OnceSupport, SyncError};

/// Current state of a raw once primitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OnceState {
    /// Initialization has not started yet.
    Uninitialized,
    /// One thread is currently running the initializer.
    Running,
    /// Initialization completed successfully.
    Complete,
}

/// Result of attempting to enter a raw once primitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OnceBeginResult {
    /// The caller won the right to run the initializer.
    Initialize,
    /// Another thread is currently running the initializer.
    InProgress,
    /// Initialization has already completed.
    Complete,
}

/// Low-level once-initialization contract implemented by selected platform backends.
///
/// This primitive intentionally does not expose poisoning semantics. A failed initializer
/// should reset the once state so another caller can retry or report the failure upward.
pub trait RawOnce: Send + Sync {
    /// Reports the support surface of this once instance.
    fn support(&self) -> OnceSupport;

    /// Returns the current once state.
    fn state(&self) -> OnceState;

    /// Attempts to begin one-time initialization.
    ///
    /// A return value of [`OnceBeginResult::InProgress`] indicates that another thread is
    /// currently running the initializer. Callers are expected to follow that result with
    /// [`wait`] or rely on a higher-level wrapper that does so, rather than busy-spinning
    /// on `begin`.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot perform the state transition honestly.
    fn begin(&self) -> Result<OnceBeginResult, SyncError>;

    /// Waits until the running initializer completes or resets.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot perform the wait honestly.
    fn wait(&self) -> Result<(), SyncError>;

    /// Marks the once as successfully initialized.
    ///
    /// # Safety
    ///
    /// The caller must currently own the initialization right returned by [`begin`] and must
    /// call exactly one of `complete_unchecked` or `reset_unchecked`.
    unsafe fn complete_unchecked(&self);

    /// Resets the once after failed or abandoned initialization.
    ///
    /// # Safety
    ///
    /// The caller must currently own the initialization right returned by [`begin`] and must
    /// call exactly one of `complete_unchecked` or `reset_unchecked`.
    unsafe fn reset_unchecked(&self);
}
