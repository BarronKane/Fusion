//! Public coprocessor export for the selected platform backend.

/// Concrete coprocessor provider type and constructor for the selected platform.
pub use super::platform::pcu::{PlatformPcu, system_pcu};
/// Backend-neutral fusion-pal coprocessor vocabulary and traits.
pub use crate::pcu::*;
