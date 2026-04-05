//! Backend-neutral user-space execution context vocabulary and unsafe fusion-pal traits.
//!
//! This surface exists for stackful cooperative execution contexts such as fibers or green
//! threads. It is intentionally narrower than any specific platform coroutine API: the fusion-pal
//! answers only what context switching can be performed honestly, what stack discipline is
//! required, and whether a saved context may migrate across carrier threads.

mod caps;
mod error;
mod unsupported;

use core::num::NonZeroUsize;
use core::ptr::NonNull;

pub use caps::*;
pub use error::*;
pub use unsupported::*;

/// Raw entry signature used to bootstrap a newly created user-space context.
pub type RawContextEntry = unsafe fn(*mut ()) -> !;

/// Concrete stack layout used to host a user-space execution context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContextStackLayout {
    /// Lowest address in the backing stack reservation.
    pub base: NonNull<u8>,
    /// Total number of bytes available to the context stack.
    pub len: NonZeroUsize,
}

/// Common capability surface for a context-switching backend.
pub trait ContextBaseContract {
    /// Opaque saved context record owned by the selected backend.
    type Context;

    /// Reports the truthful context-switching surface for this backend.
    fn support(&self) -> ContextSupport;
}

/// Unsafe context-creation and context-switch surface.
///
/// Backends implementing this trait must preserve the documented calling convention,
/// stack discipline, and migration guarantees they report through [`ContextSupport`].
///
/// # Safety
///
/// Implementors must ensure that created contexts obey the reported stack alignment,
/// migration, and TLS semantics, and that swapping between contexts never violates the
/// calling convention or aliasing rules promised by the backend.
pub unsafe trait ContextSwitch: ContextBaseContract {
    /// Creates a context on the supplied stack and arranges for it to start at `entry`.
    ///
    /// # Safety
    ///
    /// The caller must provide a valid stack region meeting the backend's alignment and
    /// guard requirements, and must ensure `entry` and `arg` remain valid for the
    /// lifetime of the created context.
    ///
    /// # Errors
    ///
    /// Returns any honest backend failure, including unsupported switching, invalid stack
    /// layout, resource exhaustion, or platform-specific context-construction failure.
    unsafe fn make(
        &self,
        stack: ContextStackLayout,
        entry: RawContextEntry,
        arg: *mut (),
    ) -> Result<Self::Context, ContextError>;

    /// Saves the current execution state into `from` and resumes the previously created
    /// target context `to`.
    ///
    /// # Safety
    ///
    /// The caller must ensure both contexts are valid for this backend, that any
    /// cross-carrier resume obeys the reported migration support, and that unwinding does
    /// not cross the context-switch boundary unless the backend explicitly documents it.
    ///
    /// # Errors
    ///
    /// Returns any honest backend swap failure, including unsupported switching, invalid
    /// saved contexts, or platform-specific swap failure.
    unsafe fn swap(&self, from: &mut Self::Context, to: &Self::Context)
    -> Result<(), ContextError>;
}
