//! iOS fusion-pal hardware backend stub.

use crate::contract::pal::UnsupportedHardware;

/// Selected hardware provider type for iOS builds.
pub type PlatformHardware = UnsupportedHardware;

/// Returns the selected iOS hardware provider.
#[must_use]
pub const fn system_hardware() -> PlatformHardware {
    PlatformHardware::new()
}
