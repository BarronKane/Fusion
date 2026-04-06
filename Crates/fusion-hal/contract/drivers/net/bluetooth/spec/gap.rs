//! Canonical Bluetooth GAP frames.

pub use super::super::{
    BluetoothAdvertisingParameters,
    BluetoothConnectionDescriptor,
    BluetoothConnectionParameters,
    BluetoothPairingParameters,
    BluetoothScanParameters,
    BluetoothScanReport,
};

/// One canonical GAP frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BluetoothGapFrame<'a> {
    AdvertisingParameters(BluetoothAdvertisingParameters),
    AdvertisingData(&'a [u8]),
    ScanResponseData(&'a [u8]),
    ScanParameters(BluetoothScanParameters),
    ScanReport(BluetoothScanReport<'a>),
    ConnectionParameters(BluetoothConnectionParameters),
    ConnectionDescriptor(BluetoothConnectionDescriptor),
    PairingParameters(BluetoothPairingParameters),
}
