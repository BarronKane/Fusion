//! macOS PAL hardware backend stub.

use crate::pal::hal::UnsupportedHardware;

/// Selected hardware provider type for macOS builds.
pub type PlatformHardware = UnsupportedHardware;

/// Returns the selected macOS hardware provider.
#[must_use]
pub const fn system_hardware() -> PlatformHardware {
    PlatformHardware::new()
}
