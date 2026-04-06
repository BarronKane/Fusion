//! DriverContract-facing Bluetooth contract vocabulary.

mod advertising;
mod att;
mod base;
mod caps;
mod connection;
mod error;
mod frames;
mod gatt;
mod l2cap;
mod radio;
mod scanning;
mod security;
mod spec;
mod types;
mod unsupported;

pub use advertising::*;
pub use att::*;
pub use base::*;
pub use caps::*;
pub use connection::*;
pub use error::*;
pub use frames::*;
pub use gatt::*;
pub use l2cap::*;
pub use radio::*;
pub use scanning::*;
pub use security::*;
pub use spec::*;
pub use types::*;
pub use unsupported::*;

/// Full control surface for one opened Bluetooth adapter.
pub trait BluetoothAdapterContract:
    BluetoothOwnedAdapterContract
    + BluetoothCanonicalFrameControlContract
    + BluetoothRadioControlContract
    + BluetoothScanningControlContract
    + BluetoothAdvertisingControlContract
    + BluetoothConnectionControlContract
    + BluetoothL2capControlContract
    + BluetoothSecurityControlContract
    + BluetoothAttClientContract
    + BluetoothGattClientContract
    + BluetoothGattServerContract
{
}

impl<T> BluetoothAdapterContract for T where
    T: BluetoothOwnedAdapterContract
        + BluetoothCanonicalFrameControlContract
        + BluetoothRadioControlContract
        + BluetoothScanningControlContract
        + BluetoothAdvertisingControlContract
        + BluetoothConnectionControlContract
        + BluetoothL2capControlContract
        + BluetoothSecurityControlContract
        + BluetoothAttClientContract
        + BluetoothGattClientContract
        + BluetoothGattServerContract
{
}
