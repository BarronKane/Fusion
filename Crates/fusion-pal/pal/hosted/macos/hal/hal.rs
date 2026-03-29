//! macOS fusion-pal hardware backend stub.

use crate::contract::pal::{CachePadded64, UnsupportedHardware};

/// Selected hardware provider type for macOS builds.
pub type PlatformHardware = UnsupportedHardware;

/// Compile-time cache-padding wrapper for macOS-hosted builds.
pub type PlatformCachePadded<T> = CachePadded64<T>;

/// Compile-time cache-padding alignment exported by the selected macOS backend.
pub const PLATFORM_CACHE_LINE_ALIGN_BYTES: usize = 64;

/// Returns the selected macOS hardware provider.
#[must_use]
pub const fn system_hardware() -> PlatformHardware {
    PlatformHardware::new()
}
