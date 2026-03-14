//! Public user-space context export for the selected platform backend.

/// Concrete context provider type and constructor for the selected platform.
pub use super::platform::context::{PlatformContext, PlatformSavedContext, system_context};
/// Backend-neutral fusion-pal context vocabulary and traits.
pub use crate::pal::context::*;
