//! Canonical Wi-Fi MAC frames.

pub use super::super::{
    WifiFrameKind,
    WifiMacAddress,
};

/// One canonical Wi-Fi MAC frame view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiMacFrame<'a> {
    pub kind: WifiFrameKind,
    pub bytes: &'a [u8],
    pub source: Option<WifiMacAddress>,
    pub destination: Option<WifiMacAddress>,
    pub bssid: Option<WifiMacAddress>,
    pub sequence_control: Option<u16>,
    pub qos_control: Option<u16>,
}
