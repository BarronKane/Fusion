//! Canonical Wi-Fi data frames.

pub use super::super::{
    WifiMacAddress,
    WifiReceivedFrame,
    WifiTransmitFrame,
};

/// One canonical Wi-Fi data frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiDataFrame<'a> {
    pub bytes: &'a [u8],
    pub source: Option<WifiMacAddress>,
    pub destination: Option<WifiMacAddress>,
    pub rssi_dbm: Option<i8>,
}

impl<'a> From<WifiReceivedFrame<'a>> for WifiDataFrame<'a> {
    fn from(value: WifiReceivedFrame<'a>) -> Self {
        Self {
            bytes: value.bytes,
            source: value.source,
            destination: value.destination,
            rssi_dbm: value.rssi_dbm,
        }
    }
}

impl<'a> From<WifiTransmitFrame<'a>> for WifiDataFrame<'a> {
    fn from(value: WifiTransmitFrame<'a>) -> Self {
        Self {
            bytes: value.bytes,
            source: value.source,
            destination: value.destination,
            rssi_dbm: None,
        }
    }
}
