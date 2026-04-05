//! Internal CYW43439 host-transport descriptors.

#[path = "bluetooth/bluetooth.rs"]
pub mod bluetooth;

#[path = "wlan/wlan.rs"]
pub mod wlan;

pub use bluetooth::{
    Cyw43439BluetoothTransport,
    Cyw43439BluetoothTransportClockProfile,
};
pub use wlan::{
    Cyw43439WlanTransport,
    Cyw43439WlanTransportClockProfile,
};

/// How the selected host reaches the two logical radios on one CYW43439 combo chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cyw43439TransportTopology {
    /// The WLAN and Bluetooth facets have distinct host-transport lanes.
    SplitHostTransports,
    /// The current board/backend exposes one provisional shared bus for both facets.
    ///
    /// This is intentionally named as a board-specific escape hatch instead of pretending it is
    /// canonical CYW43439 law.
    SharedBoardTransport,
}
