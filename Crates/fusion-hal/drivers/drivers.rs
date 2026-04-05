//! DriverContract implementations layered over PAL substrate truth.

#[path = "bus/bus.rs"]
pub mod bus;

#[path = "net/net.rs"]
pub mod net;

#[path = "peripheral/peripheral.rs"]
pub mod peripheral;
