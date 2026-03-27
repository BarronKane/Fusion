//! Hardware-facing PAL contract grouping.

pub mod gpio;
#[path = "mem/mem.rs"]
pub mod mem;
pub mod pcu;
#[path = "power/power.rs"]
pub mod power;

pub use gpio::GpioContract;
