//! Canonical Wi-Fi control-plane frames.

pub use super::super::{
    WifiAccessPointId,
    WifiAssociatedClient,
    WifiConnectionDescriptor,
    WifiLinkId,
    WifiMonitorParameters,
    WifiScanParameters,
    WifiSecurityParameters,
};

/// One canonical Wi-Fi control frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WifiControlFrame<'a> {
    ScanParameters(WifiScanParameters),
    SecurityParameters(WifiSecurityParameters<'a>),
    ConnectionDescriptor(WifiConnectionDescriptor),
    AssociatedClient(WifiAssociatedClient),
    LinkDown {
        link: WifiLinkId,
        reason_code: Option<u16>,
    },
    MonitorParameters(WifiMonitorParameters),
    AccessPointStarted(WifiAccessPointId),
    AccessPointStopped(WifiAccessPointId),
}
