//! Firmware-orchestrated selected driver bindings.

#[cfg(all(target_os = "none", feature = "soc-rp2350"))]
#[path = "bus/bus.rs"]
pub mod bus;

#[cfg(all(target_os = "none", feature = "soc-rp2350"))]
#[path = "net/net.rs"]
pub mod net;
