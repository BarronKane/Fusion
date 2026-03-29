//! Driver-facing contracts layered on top of platform truth.

#[path = "gpio/gpio.rs"]
pub mod gpio;

#[path = "pcu/pcu.rs"]
pub mod pcu;

#[path = "peripheral/peripheral.rs"]
pub mod peripheral;
