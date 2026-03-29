//! macOS fusion-pal programmable-IO backend.

use crate::contract::drivers::pcu::UnsupportedPcu;

/// Selected macOS programmable-IO provider type.
pub type PlatformPcu = UnsupportedPcu;

/// Returns the selected macOS programmable-IO provider.
#[must_use]
pub const fn system_pcu() -> PlatformPcu {
    PlatformPcu::new()
}
