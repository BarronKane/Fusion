//! Windows PAL synchronization backend stub.

use crate::pal::sync::{
    SyncImplementationKind, UnsupportedRawMutex, UnsupportedSemaphore, UnsupportedSync,
};

/// Selected raw mutex type for Windows builds.
pub type PlatformRawMutex = UnsupportedRawMutex;

/// Selected semaphore type for Windows builds.
pub type PlatformSemaphore = UnsupportedSemaphore;

/// Target-selected synchronization provider alias for Windows builds.
pub type PlatformSync = UnsupportedSync;

/// Backend truth for the selected raw mutex implementation on Windows.
pub const PLATFORM_RAW_MUTEX_IMPLEMENTATION: SyncImplementationKind =
    SyncImplementationKind::Unsupported;

/// Returns the process-wide Windows synchronization provider handle.
#[must_use]
pub const fn system_sync() -> PlatformSync {
    PlatformSync::new()
}
