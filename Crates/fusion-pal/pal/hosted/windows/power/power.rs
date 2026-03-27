//! Windows fusion-pal power backend.

use crate::contract::hardware::power::UnsupportedPower;

/// Selected Windows power provider type.
pub type PlatformPower = UnsupportedPower;

/// Returns the selected Windows power provider.
#[must_use]
pub const fn system_power() -> PlatformPower {
    PlatformPower::new()
}
