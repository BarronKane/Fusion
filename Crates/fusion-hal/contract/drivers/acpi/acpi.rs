//! DriverContract-facing ACPI component contract vocabulary.

mod battery;
mod button;
mod core;
mod embedded_controller;
mod error;
mod fan;
mod lid;
mod power_source;
mod processor;
mod thermal;

pub use battery::*;
pub use button::*;
pub use core::*;
pub use embedded_controller::*;
pub use error::*;
pub use fan::*;
pub use lid::*;
pub use power_source::*;
pub use processor::*;
pub use thermal::*;
