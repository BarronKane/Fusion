//! Public hardware export for the selected platform backend.
//!
//! This module forwards the chosen private backend's hardware implementation together with
//! the backend-neutral fusion-pal HAL contract and capability types.

/// Concrete hardware provider type and constructor for the selected platform.
pub use super::platform::hal::{PlatformHardware, system_hardware};
/// Backend-neutral fusion-pal hardware vocabulary and traits.
pub use crate::pal::hal::*;
