//! iOS fusion-pal user-space context backend.

use crate::pal::context::{UnsupportedContext, UnsupportedSavedContext};

/// Selected iOS context provider type.
pub type PlatformContext = UnsupportedContext;
/// Selected iOS saved-context type.
pub type PlatformSavedContext = UnsupportedSavedContext;

/// Returns the selected iOS context provider.
#[must_use]
pub const fn system_context() -> PlatformContext {
    PlatformContext::new()
}
