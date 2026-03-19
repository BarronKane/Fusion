//! Selected Cortex-M SoC board wiring.

#[path = "board/board.rs"]
mod board_contract;

#[cfg(not(any(feature = "soc-rp2350", feature = "soc-stm32h7")))]
#[path = "generic.rs"]
pub mod board;
#[cfg(feature = "soc-rp2350")]
#[path = "rp2350.rs"]
pub mod board;
#[cfg(feature = "soc-stm32h7")]
#[path = "stm32h7.rs"]
pub mod board;
