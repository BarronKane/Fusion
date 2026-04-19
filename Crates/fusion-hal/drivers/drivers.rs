//! DriverContract implementations layered over PAL substrate truth.

#[path = "acpi/acpi.rs"]
pub mod acpi;

#[path = "bus/bus.rs"]
pub mod bus;

#[path = "display/display.rs"]
pub mod display;

#[path = "net/net.rs"]
pub mod net;

#[path = "peripheral/peripheral.rs"]
pub mod peripheral;
