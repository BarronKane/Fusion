//! macOS PAL synchronization backend stub.

use crate::pal::sync::{
    SyncImplementationKind, UnsupportedRawMutex, UnsupportedSemaphore, UnsupportedSync,
};

/// Selected raw mutex type for macOS builds.
pub type PlatformRawMutex = UnsupportedRawMutex;

/// Selected semaphore type for macOS builds.
pub type PlatformSemaphore = UnsupportedSemaphore;

/// Target-selected synchronization provider alias for macOS builds.
pub type PlatformSync = UnsupportedSync;

/// Backend truth for the selected raw mutex implementation on macOS.
pub const PLATFORM_RAW_MUTEX_IMPLEMENTATION: SyncImplementationKind =
    SyncImplementationKind::Unsupported;

/// Returns the process-wide macOS synchronization provider handle.
#[must_use]
pub const fn system_sync() -> PlatformSync {
    PlatformSync::new()
}
