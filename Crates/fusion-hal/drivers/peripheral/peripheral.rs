//! Simple compound hardware peripherals layered over truthful low-level contracts.

#[path = "interface/interface.rs"]
pub mod interface;

mod audio_jack;
mod button;
mod buzzer;
mod led;
mod led_pair;
mod oled;
mod seven_segment;
mod shift_register_74hc595;
mod speaker;

pub use audio_jack::*;
pub use button::*;
pub use buzzer::*;
pub use interface::*;
pub use led::*;
pub use led_pair::*;
pub use oled::*;
pub use seven_segment::*;
pub use shift_register_74hc595::*;
pub use speaker::*;
