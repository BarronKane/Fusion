//! macOS fusion-pal hardware backend stub.

use crate::contract::hal::UnsupportedHardware;

/// Selected hardware provider type for macOS builds.
pub type PlatformHardware = UnsupportedHardware;

/// Returns the selected macOS hardware provider.
#[must_use]
pub const fn system_hardware() -> PlatformHardware {
    PlatformHardware::new()
}
