//! Public synchronization export for the selected platform backend.
//!
//! This module forwards the chosen private backend's synchronization implementation
//! together with the backend-neutral fusion-pal sync contract and capability types.

/// Concrete synchronization provider type and constructor for the selected platform.
pub use super::platform::sync::{
    PLATFORM_RAW_MUTEX_IMPLEMENTATION, PLATFORM_RAW_ONCE_IMPLEMENTATION,
    PLATFORM_RAW_RWLOCK_IMPLEMENTATION, PlatformRawMutex, PlatformRawOnce, PlatformRawRwLock,
    PlatformSemaphore, PlatformSync, system_sync,
};
/// Backend-neutral fusion-pal synchronization vocabulary and traits.
pub use crate::pal::sync::*;
