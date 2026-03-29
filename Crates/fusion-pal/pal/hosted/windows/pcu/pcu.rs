//! Windows fusion-pal programmable-IO backend.

use crate::contract::drivers::pcu::UnsupportedPcu;

/// Selected Windows programmable-IO provider type.
pub type PlatformPcu = UnsupportedPcu;

/// Returns the selected Windows programmable-IO provider.
#[must_use]
pub const fn system_pcu() -> PlatformPcu {
    PlatformPcu::new()
}
