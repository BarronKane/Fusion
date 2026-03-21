//! Public programmable-IO export for the selected platform backend.

/// Concrete programmable-IO provider type and constructor for the selected platform.
pub use super::platform::pcu::{PlatformPcu, system_pcu};
/// Backend-neutral fusion-pal programmable-IO vocabulary and traits.
pub use crate::pcu::*;
