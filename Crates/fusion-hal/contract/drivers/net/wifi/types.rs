//! Shared generic Wi-Fi identifier, descriptor, and request vocabulary.

use super::super::NetVendorIdentity;
use super::caps::WifiAccessPointCaps;
use super::caps::WifiBandCaps;
use super::caps::WifiChannelWidthCaps;
use super::caps::WifiDataCaps;
use super::caps::WifiMeshCaps;
use super::caps::WifiMloCaps;
use super::caps::WifiMonitorCaps;
use super::caps::WifiP2pCaps;
use super::caps::WifiRoleCaps;
use super::caps::WifiScanCaps;
use super::caps::WifiSecurityCaps;
use super::caps::WifiStandardFamilyCaps;
use super::caps::WifiStationCaps;

/// Stable surfaced Wi-Fi adapter identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiAdapterId(pub u16);

/// Stable Wi-Fi link identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiLinkId(pub u16);

/// Stable Wi-Fi scan session identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiScanSessionId(pub u16);

/// Stable hosted access-point identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiAccessPointId(pub u8);

/// Stable monitor session identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiMonitorSessionId(pub u8);

/// Stable mesh-session identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiMeshId(pub u16);

/// Canonical Wi-Fi MAC address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiMacAddress {
    /// Address bytes in canonical network order.
    pub bytes: [u8; 6],
}

/// Regulatory-domain code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiRegulatoryDomain {
    /// Two-character regulatory domain code.
    pub code: [u8; 2],
}

/// Stable owned SSID representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiSsid {
    bytes: [u8; 32],
    len: u8,
}

impl WifiSsid {
    /// Creates one SSID from fixed storage plus one explicit visible length.
    #[must_use]
    pub const fn new(bytes: [u8; 32], len: u8) -> Self {
        Self { bytes, len }
    }

    /// Returns the visible SSID bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }

    /// Returns the SSID length in octets.
    #[must_use]
    pub const fn len(self) -> u8 {
        self.len
    }

    /// Returns whether the SSID is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }
}

/// Wi-Fi standard family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WifiStandardFamily {
    Legacy,
    Ht,
    Vht,
    He,
    Eht,
}

/// Wi-Fi operating band.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WifiBand {
    Ghz2_4,
    Ghz5,
    Ghz6,
    Ghz60,
}

/// Wi-Fi channel width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WifiChannelWidth {
    Width20Mhz,
    Width40Mhz,
    Width80Mhz,
    Width160Mhz,
    Width320Mhz,
}

/// One currently selected or discovered Wi-Fi channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiChannelDescriptor {
    pub band: WifiBand,
    pub primary_channel: u16,
    pub width: WifiChannelWidth,
    pub center_frequency_mhz: u16,
    pub dfs_required: bool,
    pub passive_only: bool,
}

/// Supported Wi-Fi authentication families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WifiAuthenticationMode {
    Open,
    Wep,
    WpaPersonal,
    Wpa2Personal,
    Wpa3Personal,
    Wpa2Enterprise,
    Wpa3Enterprise,
    Owe,
}

/// Supported Wi-Fi cipher suites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WifiCipherSuite {
    None,
    Wep40,
    Wep104,
    Tkip,
    Ccmp128,
    Gcmp128,
    Gcmp256,
}

/// Full truthful support snapshot for one surfaced Wi-Fi adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiAdapterSupport {
    pub standards: WifiStandardFamilyCaps,
    pub roles: WifiRoleCaps,
    pub bands: WifiBandCaps,
    pub channel_widths: WifiChannelWidthCaps,
    pub security: WifiSecurityCaps,
    pub scan: WifiScanCaps,
    pub station: WifiStationCaps,
    pub access_point: WifiAccessPointCaps,
    pub data: WifiDataCaps,
    pub monitor: WifiMonitorCaps,
    pub p2p: WifiP2pCaps,
    pub mesh: WifiMeshCaps,
    pub mlo: WifiMloCaps,
    pub max_scan_results: u16,
    pub max_links: u16,
    pub max_access_points: u8,
    pub max_associated_clients: u16,
    pub max_mesh_peers: u16,
    pub max_tx_queues: u8,
    pub max_spatial_streams: u8,
}

/// Static descriptor for one surfaced Wi-Fi adapter/controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiAdapterDescriptor {
    pub id: WifiAdapterId,
    pub name: &'static str,
    pub vendor_identity: Option<NetVendorIdentity>,
    pub mac_address: Option<WifiMacAddress>,
    pub regulatory_domain: Option<WifiRegulatoryDomain>,
    pub channels: &'static [WifiChannelDescriptor],
    pub support: WifiAdapterSupport,
}

/// Wi-Fi scan parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiScanParameters {
    pub passive: bool,
    pub bands: WifiBandCaps,
    pub ssid_filter: Option<WifiSsid>,
    pub bssid_filter: Option<WifiMacAddress>,
    pub channel_filter: Option<WifiChannelDescriptor>,
    pub dwell_time_ms: Option<u16>,
    pub max_results: Option<u16>,
}

/// One surfaced Wi-Fi scan report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiScanReport<'a> {
    pub ssid: WifiSsid,
    pub bssid: WifiMacAddress,
    pub band: WifiBand,
    pub channel: WifiChannelDescriptor,
    pub standards: WifiStandardFamilyCaps,
    pub security: WifiSecurityCaps,
    pub rssi_dbm: i8,
    pub information_elements: &'a [u8],
}

/// Security parameters for station or hosted-AP operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiSecurityParameters<'a> {
    pub authentication: WifiAuthenticationMode,
    pub pairwise_cipher: WifiCipherSuite,
    pub group_cipher: WifiCipherSuite,
    pub passphrase: Option<&'a [u8]>,
    pub identity: Option<&'a [u8]>,
    pub anonymous_identity: Option<&'a [u8]>,
    pub password: Option<&'a [u8]>,
    pub pmf_required: bool,
}

/// Station-mode connection parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiConnectParameters<'a> {
    pub ssid: WifiSsid,
    pub bssid: Option<WifiMacAddress>,
    pub security: WifiSecurityParameters<'a>,
    pub preferred_channel: Option<WifiChannelDescriptor>,
    pub powersave_enabled: bool,
}

/// One surfaced station connection descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiConnectionDescriptor {
    pub id: WifiLinkId,
    pub ssid: WifiSsid,
    pub bssid: WifiMacAddress,
    pub station_address: Option<WifiMacAddress>,
    pub band: WifiBand,
    pub channel: WifiChannelDescriptor,
    pub standards: WifiStandardFamilyCaps,
    pub authenticated: bool,
    pub associated: bool,
    pub encrypted: bool,
    pub rssi_dbm: Option<i8>,
}

/// Hosted access-point configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiApConfiguration<'a> {
    pub ssid: WifiSsid,
    pub band: WifiBand,
    pub channel: WifiChannelDescriptor,
    pub hidden: bool,
    pub security: WifiSecurityParameters<'a>,
    pub max_clients: Option<u16>,
}

/// One associated hosted-AP client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiAssociatedClient {
    pub address: WifiMacAddress,
    pub authenticated: bool,
    pub authorized: bool,
    pub powersave: bool,
    pub rssi_dbm: Option<i8>,
}

/// One received Wi-Fi frame or MSDU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiReceivedFrame<'a> {
    pub bytes: &'a [u8],
    pub source: Option<WifiMacAddress>,
    pub destination: Option<WifiMacAddress>,
    pub rssi_dbm: Option<i8>,
}

/// Monitor-mode configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiMonitorParameters {
    pub band: Option<WifiBand>,
    pub channel: Option<WifiChannelDescriptor>,
    pub require_fcs_status: bool,
}

/// Mesh-join configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiMeshConfiguration<'a> {
    pub mesh_id: &'a [u8],
    pub security: Option<WifiSecurityParameters<'a>>,
    pub channel: Option<WifiChannelDescriptor>,
}
