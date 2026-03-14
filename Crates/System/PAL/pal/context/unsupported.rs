//! Backend-neutral unsupported context implementation.

use super::{
    ContextBase, ContextError, ContextStackLayout, ContextSupport, ContextSwitch, RawContextEntry,
};

/// Unsupported context provider placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedContext;

/// Unsupported saved-context placeholder.
#[derive(Debug, Default, PartialEq, Eq, Hash)]
pub struct UnsupportedSavedContext;

impl UnsupportedContext {
    /// Creates a new unsupported context provider placeholder.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ContextBase for UnsupportedContext {
    type Context = UnsupportedSavedContext;

    fn support(&self) -> ContextSupport {
        ContextSupport::unsupported()
    }
}

// SAFETY: this backend never creates or switches contexts successfully.
unsafe impl ContextSwitch for UnsupportedContext {
    unsafe fn make(
        &self,
        _stack: ContextStackLayout,
        _entry: RawContextEntry,
        _arg: *mut (),
    ) -> Result<Self::Context, ContextError> {
        Err(ContextError::unsupported())
    }

    unsafe fn swap(
        &self,
        _from: &mut Self::Context,
        _to: &Self::Context,
    ) -> Result<(), ContextError> {
        Err(ContextError::unsupported())
    }
}
