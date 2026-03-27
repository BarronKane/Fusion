//! Linux fusion-pal power backend.

use crate::contract::hardware::power::UnsupportedPower;

/// Selected Linux power provider type.
pub type PlatformPower = UnsupportedPower;

/// Returns the selected Linux power provider.
#[must_use]
pub const fn system_power() -> PlatformPower {
    PlatformPower::new()
}
