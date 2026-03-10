//! Public memory export for the selected platform backend.
//!
//! This module forwards the chosen private backend's memory implementation together with
//! the backend-neutral PAL memory contract types.

/// Concrete memory provider type and constructor for the selected platform.
pub use super::platform::mem::{PlatformMem, system_mem};
/// Backend-neutral PAL memory vocabulary and traits.
pub use crate::pal::mem::*;
