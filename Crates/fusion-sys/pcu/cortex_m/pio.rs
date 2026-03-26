//! Cortex-M programmable-IO wrappers, planning vocabulary, and deterministic PIO IR.

#[path = "pio/plan.rs"]
mod plan_impl;
#[path = "pio/system.rs"]
mod system_impl;

pub use fusion_pal::sys::cortex_m::hal::soc::pio::*;
#[doc(hidden)]
pub use fusion_pal::sys::cortex_m::hal::soc::pio::{
    PioError as PcuError,
    PioErrorKind as PcuErrorKind,
};
pub use plan_impl::*;
pub use system_impl::{PcuSystem as PioSystem, system_pio};
