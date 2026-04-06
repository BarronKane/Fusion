//! Canonical Wi-Fi event frames.

pub use super::super::{
    WifiConnectionDescriptor,
    WifiLinkId,
    WifiScanReport,
};

/// One canonical Wi-Fi event frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WifiEventFrame<'a> {
    ScanReport(WifiScanReport<'a>),
    LinkUp(WifiConnectionDescriptor),
    LinkDown {
        link: WifiLinkId,
        reason_code: Option<u16>,
    },
    RegulatoryUpdate(&'a [u8]),
    Vendor(&'a [u8]),
}
