//! Cortex-M programmable-IO wrappers, planning vocabulary, and deterministic PIO IR.

mod plan;
mod system;

pub use fusion_pal::sys::soc::cortex_m::hal::soc::pio::*;
#[doc(hidden)]
pub use fusion_pal::sys::soc::cortex_m::hal::soc::pio::{
    PioError as PcuError,
    PioErrorKind as PcuErrorKind,
};

pub use plan::*;
pub use system::{
    PcuSystem as PioSystem,
    system_pio,
};
