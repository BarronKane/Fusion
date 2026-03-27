#![allow(clippy::doc_markdown)]

//! Selected Cortex-M SoC board wiring.

#[path = "board/board.rs"]
mod board_contract;

#[cfg(not(feature = "soc-rp2350"))]
#[path = "generic.rs"]
pub mod generic;
#[cfg(feature = "soc-rp2350")]
#[path = "rp2350.rs"]
pub mod rp2350;

#[cfg(not(feature = "soc-rp2350"))]
pub use generic as board;
#[cfg(feature = "soc-rp2350")]
pub use rp2350 as board;

#[path = "pio/pio.rs"]
pub mod pio;
