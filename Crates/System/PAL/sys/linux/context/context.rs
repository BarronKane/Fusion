//! Linux PAL user-space context backend.
//!
//! The concrete Linux context-switch implementation is intentionally deferred until the
//! ISA-level contract is fully settled. The selected Linux backend therefore reports the
//! honest unsupported surface for now instead of pretending assembly exists because the
//! file tree said so.

use crate::pal::context::{UnsupportedContext, UnsupportedSavedContext};

/// Selected Linux context provider type.
pub type PlatformContext = UnsupportedContext;
/// Selected Linux saved-context type.
pub type PlatformSavedContext = UnsupportedSavedContext;

/// Returns the selected Linux context provider.
#[must_use]
pub const fn system_context() -> PlatformContext {
    PlatformContext::new()
}
