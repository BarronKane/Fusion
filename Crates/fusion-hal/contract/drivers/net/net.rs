//! Network-facing driver contracts layered on top of platform truth.

mod identity;

pub use identity::*;

#[path = "bluetooth/bluetooth.rs"]
pub mod bluetooth;

#[path = "wifi/wifi.rs"]
pub mod wifi;
