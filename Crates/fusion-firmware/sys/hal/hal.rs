//! Selected firmware and dynamic hardware-discovery facade.

pub use crate::pal::hal::*;

#[path = "drivers/drivers.rs"]
pub mod drivers;
#[path = "runtime/runtime.rs"]
pub mod runtime;
