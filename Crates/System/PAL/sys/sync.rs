//! Public synchronization export for the selected platform backend.
//!
//! This module forwards the chosen private backend's synchronization implementation
//! together with the backend-neutral PAL sync contract and capability types.

/// Concrete synchronization provider type and constructor for the selected platform.
pub use super::platform::sync::{
    PLATFORM_RAW_MUTEX_IMPLEMENTATION, PlatformRawMutex, PlatformSemaphore, PlatformSync,
    system_sync,
};
/// Backend-neutral PAL synchronization vocabulary and traits.
pub use crate::pal::sync::*;
