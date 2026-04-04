//! iOS fusion-pal hardware backend stub.

use crate::contract::pal::{
    CachePadded64,
    UnsupportedHardware,
};

/// Selected hardware provider type for iOS builds.
pub type PlatformHardware = UnsupportedHardware;

/// Compile-time cache-padding wrapper for iOS-hosted builds.
pub type PlatformCachePadded<T> = CachePadded64<T>;

/// Compile-time cache-padding alignment exported by the selected iOS backend.
pub const PLATFORM_CACHE_LINE_ALIGN_BYTES: usize = 64;

/// Returns the selected iOS hardware provider.
#[must_use]
pub const fn system_hardware() -> PlatformHardware {
    PlatformHardware::new()
}
