//! Driver-facing Bluetooth contract vocabulary.

mod advertising;
mod att;
mod base;
mod caps;
mod connection;
mod error;
mod gatt;
mod l2cap;
mod radio;
mod scanning;
mod security;
mod types;
mod unsupported;

pub use advertising::*;
pub use att::*;
pub use base::*;
pub use caps::*;
pub use connection::*;
pub use error::*;
pub use gatt::*;
pub use l2cap::*;
pub use radio::*;
pub use scanning::*;
pub use security::*;
pub use types::*;
pub use unsupported::*;

/// Full control surface for one opened Bluetooth adapter.
pub trait BluetoothAdapter:
    BluetoothOwnedAdapter
    + BluetoothRadioControl
    + BluetoothScanningControl
    + BluetoothAdvertisingControl
    + BluetoothConnectionControl
    + BluetoothL2capControl
    + BluetoothSecurityControl
    + BluetoothAttClient
    + BluetoothGattClient
    + BluetoothGattServer
{
}

impl<T> BluetoothAdapter for T where
    T: BluetoothOwnedAdapter
        + BluetoothRadioControl
        + BluetoothScanningControl
        + BluetoothAdvertisingControl
        + BluetoothConnectionControl
        + BluetoothL2capControl
        + BluetoothSecurityControl
        + BluetoothAttClient
        + BluetoothGattClient
        + BluetoothGattServer
{
}
