//! Linux fusion-pal programmable-IO backend.

use crate::contract::pal::pcu::UnsupportedPcu;

/// Selected Linux programmable-IO provider type.
pub type PlatformPcu = UnsupportedPcu;

/// Returns the selected Linux programmable-IO provider.
#[must_use]
pub const fn system_pcu() -> PlatformPcu {
    PlatformPcu::new()
}
