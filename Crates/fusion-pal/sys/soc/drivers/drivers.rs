//! Selected SoC-declared driver exports.
//!
//! This namespace is intentionally narrow:
//! - only SoC-declared driver exports live here
//! - only for the selected SoC path
//! - absent drivers should stay absent rather than pretending to exist

#[cfg(all(target_os = "none", feature = "soc-rp2350"))]
pub use crate::pal::soc::cortex_m::rp2350::drivers::bus;

#[cfg(all(target_os = "none", feature = "soc-rp2350"))]
pub use crate::pal::soc::cortex_m::rp2350::drivers::net;
