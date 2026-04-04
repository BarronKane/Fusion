//! Selected SoC facade wiring for the fusion-pal.

#[path = "drivers/drivers.rs"]
/// Selected SoC driver exports surfaced through the PAL system facade.
pub mod drivers;

pub use crate::pal::soc::*;
