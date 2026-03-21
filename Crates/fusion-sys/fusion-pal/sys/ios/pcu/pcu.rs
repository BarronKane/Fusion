//! iOS fusion-pal programmable-IO backend.

use crate::pcu::UnsupportedPcu;

/// Selected iOS programmable-IO provider type.
pub type PlatformPcu = UnsupportedPcu;

/// Returns the selected iOS programmable-IO provider.
#[must_use]
pub const fn system_pcu() -> PlatformPcu {
    PlatformPcu::new()
}
