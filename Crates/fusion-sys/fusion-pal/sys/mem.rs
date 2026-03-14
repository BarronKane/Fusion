//! Public memory export for the selected platform backend.
//!
//! This module forwards the chosen private backend's memory implementation together with
//! the backend-neutral fusion-pal memory contract, inventory, and topology types.

/// Concrete memory provider type and constructor for the selected platform.
pub use super::platform::mem::{PlatformMem, system_mem};
/// Backend-neutral fusion-pal memory vocabulary and traits.
pub use crate::pal::mem::*;
