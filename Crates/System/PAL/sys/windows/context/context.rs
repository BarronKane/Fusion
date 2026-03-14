//! Windows PAL user-space context backend.

use crate::pal::context::{UnsupportedContext, UnsupportedSavedContext};

/// Selected Windows context provider type.
pub type PlatformContext = UnsupportedContext;
/// Selected Windows saved-context type.
pub type PlatformSavedContext = UnsupportedSavedContext;

/// Returns the selected Windows context provider.
#[must_use]
pub const fn system_context() -> PlatformContext {
    PlatformContext::new()
}
