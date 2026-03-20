//! iOS fusion-pal synchronization backend stub.

use crate::pal::sync::{
    SyncImplementationKind,
    UnsupportedRawMutex,
    UnsupportedRawOnce,
    UnsupportedRawRwLock,
    UnsupportedSemaphore,
    UnsupportedSync,
};

/// Selected raw mutex type for iOS builds.
pub type PlatformRawMutex = UnsupportedRawMutex;

/// Selected semaphore type for iOS builds.
pub type PlatformSemaphore = UnsupportedSemaphore;

/// Selected raw once type for iOS builds.
pub type PlatformRawOnce = UnsupportedRawOnce;

/// Selected raw rwlock type for iOS builds.
pub type PlatformRawRwLock = UnsupportedRawRwLock;

/// Target-selected synchronization provider alias for iOS builds.
pub type PlatformSync = UnsupportedSync;

/// Backend truth for the selected raw mutex implementation on iOS.
pub const PLATFORM_RAW_MUTEX_IMPLEMENTATION: SyncImplementationKind =
    SyncImplementationKind::Unsupported;

/// Backend truth for the selected raw once implementation on iOS.
pub const PLATFORM_RAW_ONCE_IMPLEMENTATION: SyncImplementationKind =
    SyncImplementationKind::Unsupported;

/// Backend truth for the selected raw rwlock implementation on iOS.
pub const PLATFORM_RAW_RWLOCK_IMPLEMENTATION: SyncImplementationKind =
    SyncImplementationKind::Unsupported;

/// Returns the process-wide iOS synchronization provider handle.
#[must_use]
pub const fn system_sync() -> PlatformSync {
    PlatformSync::new()
}
