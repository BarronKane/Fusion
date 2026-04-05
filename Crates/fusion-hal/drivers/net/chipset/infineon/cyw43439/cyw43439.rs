//! Infineon CYW43439 combo-chip driver family.

#![cfg_attr(not(feature = "std"), no_std)]

mod core;

#[path = "bluetooth/bluetooth.rs"]
pub mod bluetooth;

#[path = "firmware/firmware.rs"]
pub mod firmware;

#[path = "wifi/wifi.rs"]
pub mod wifi;

#[path = "interface/interface.rs"]
pub mod interface;

#[path = "transport/transport.rs"]
pub mod transport;

#[cfg(any(target_os = "none", feature = "fdxe-module"))]
mod fdxe;
