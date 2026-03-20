//! Public power export for the selected platform backend.

/// Concrete power provider type and constructor for the selected platform.
pub use super::platform::power::{PlatformPower, system_power};
/// Backend-neutral fusion-pal power vocabulary and traits.
pub use crate::pal::power::*;
