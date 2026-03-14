//! Public thread export for the selected platform backend.
//!
//! This module forwards the chosen private backend's thread implementation together with
//! the backend-neutral fusion-pal thread contract and capability types.

/// Concrete thread provider type and constructor for the selected platform.
pub use super::platform::thread::{PlatformThread, PlatformThreadHandle, system_thread};
/// Backend-neutral fusion-pal thread vocabulary and traits.
pub use crate::pal::thread::*;
