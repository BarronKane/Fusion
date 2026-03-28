//! Windows fusion-pal hardware backend stub.

use crate::contract::pal::UnsupportedHardware;

/// Selected hardware provider type for Windows builds.
pub type PlatformHardware = UnsupportedHardware;

/// Returns the selected Windows hardware provider.
#[must_use]
pub const fn system_hardware() -> PlatformHardware {
    PlatformHardware::new()
}
