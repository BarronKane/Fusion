//! Unsupported generic Wi-Fi placeholders.

use super::WifiAccessPointControlContract;
use super::WifiAccessPointId;
use super::WifiAdapterDescriptor;
use super::WifiAdapterId;
use super::WifiAdapterSupport;
use super::WifiApConfiguration;
use super::WifiAssociatedClient;
use super::WifiBandCaps;
use super::WifiBaseContract;
use super::WifiChannelDescriptor;
use super::WifiConnectParameters;
use super::WifiControlContract;
use super::WifiDataControlContract;
use super::WifiError;
use super::WifiImplementationKind;
use super::WifiLinkId;
use super::WifiMeshConfiguration;
use super::WifiMeshControlContract;
use super::WifiMeshId;
use super::WifiMonitorControlContract;
use super::WifiMonitorParameters;
use super::WifiMonitorSessionId;
use super::WifiOwnedAdapterContract;
use super::WifiP2pControlContract;
use super::WifiRadioControlContract;
use super::WifiReceivedFrame;
use super::WifiRegulatoryDomain;
use super::WifiRoleCaps;
use super::WifiScanCaps;
use super::WifiScanControlContract;
use super::WifiScanParameters;
use super::WifiScanReport;
use super::WifiScanSessionId;
use super::WifiSecurityCaps;
use super::WifiSecurityControlContract;
use super::WifiStationControlContract;
use super::WifiStationCaps;
use super::WifiSupport;
use super::WifiTransmitFrame;
use super::WifiProviderCaps;
use super::WifiStandardFamilyCaps;
use super::WifiConnectionDescriptor;
use super::WifiMonitorCaps;
use super::WifiDataCaps;
use super::WifiP2pCaps;
use super::WifiMeshCaps;
use super::WifiMloCaps;
use super::WifiChannelWidthCaps;

const UNSUPPORTED_CHANNELS: [WifiChannelDescriptor; 0] = [];
const UNSUPPORTED_ADAPTERS: [WifiAdapterDescriptor; 0] = [];

const UNSUPPORTED_ADAPTER_SUPPORT: WifiAdapterSupport = WifiAdapterSupport {
    standards: WifiStandardFamilyCaps::empty(),
    roles: WifiRoleCaps::empty(),
    bands: WifiBandCaps::empty(),
    channel_widths: WifiChannelWidthCaps::empty(),
    security: WifiSecurityCaps::empty(),
    scan: WifiScanCaps::empty(),
    station: WifiStationCaps::empty(),
    access_point: super::WifiAccessPointCaps::empty(),
    data: WifiDataCaps::empty(),
    monitor: WifiMonitorCaps::empty(),
    p2p: WifiP2pCaps::empty(),
    mesh: WifiMeshCaps::empty(),
    mlo: WifiMloCaps::empty(),
    max_scan_results: 0,
    max_links: 0,
    max_access_points: 0,
    max_associated_clients: 0,
    max_mesh_peers: 0,
    max_tx_queues: 0,
    max_spatial_streams: 0,
};

const UNSUPPORTED_ADAPTER: WifiAdapterDescriptor = WifiAdapterDescriptor {
    id: WifiAdapterId(0),
    name: "unsupported",
    vendor_identity: None,
    shared_chipset: false,
    mac_address: None,
    regulatory_domain: None,
    channels: &UNSUPPORTED_CHANNELS,
    support: UNSUPPORTED_ADAPTER_SUPPORT,
};

/// Unsupported generic Wi-Fi provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedWifi;

/// Unsupported generic opened Wi-Fi adapter.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedWifiAdapter;

impl WifiBaseContract for UnsupportedWifi {
    fn support(&self) -> WifiSupport {
        WifiSupport {
            caps: WifiProviderCaps::empty(),
            implementation: WifiImplementationKind::Unsupported,
            adapter_count: 0,
        }
    }

    fn adapters(&self) -> &'static [WifiAdapterDescriptor] {
        &UNSUPPORTED_ADAPTERS
    }
}

impl WifiControlContract for UnsupportedWifi {
    type Adapter = UnsupportedWifiAdapter;

    fn open_adapter(&mut self, _adapter: WifiAdapterId) -> Result<Self::Adapter, WifiError> {
        Err(WifiError::unsupported())
    }
}

impl WifiOwnedAdapterContract for UnsupportedWifiAdapter {
    fn descriptor(&self) -> &'static WifiAdapterDescriptor {
        &UNSUPPORTED_ADAPTER
    }

    fn capabilities(&self) -> WifiAdapterSupport {
        UNSUPPORTED_ADAPTER_SUPPORT
    }
}

impl WifiRadioControlContract for UnsupportedWifiAdapter {
    fn set_powered(&mut self, _powered: bool) -> Result<(), WifiError> {
        Err(WifiError::unsupported())
    }

    fn is_powered(&self) -> Result<bool, WifiError> {
        Err(WifiError::unsupported())
    }

    fn current_channel(&self) -> Result<Option<WifiChannelDescriptor>, WifiError> {
        Err(WifiError::unsupported())
    }

    fn set_channel(&mut self, _channel: WifiChannelDescriptor) -> Result<(), WifiError> {
        Err(WifiError::unsupported())
    }

    fn regulatory_domain(&self) -> Result<Option<WifiRegulatoryDomain>, WifiError> {
        Ok(None)
    }
}

impl WifiScanControlContract for UnsupportedWifiAdapter {
    fn start_scan(
        &mut self,
        _parameters: WifiScanParameters,
    ) -> Result<WifiScanSessionId, WifiError> {
        Err(WifiError::unsupported())
    }

    fn stop_scan(&mut self, _session: WifiScanSessionId) -> Result<(), WifiError> {
        Err(WifiError::unsupported())
    }

    fn next_scan_report<'a>(
        &mut self,
        _session: WifiScanSessionId,
        _information_elements: &'a mut [u8],
    ) -> Result<Option<WifiScanReport<'a>>, WifiError> {
        Err(WifiError::unsupported())
    }
}

impl WifiStationControlContract for UnsupportedWifiAdapter {
    fn connect(&mut self, _parameters: WifiConnectParameters<'_>) -> Result<WifiLinkId, WifiError> {
        Err(WifiError::unsupported())
    }

    fn disconnect(&mut self, _link: WifiLinkId) -> Result<(), WifiError> {
        Err(WifiError::unsupported())
    }

    fn connection(&self, _link: WifiLinkId) -> Result<WifiConnectionDescriptor, WifiError> {
        Err(WifiError::unsupported())
    }

    fn current_station_link(&self) -> Result<Option<WifiLinkId>, WifiError> {
        Err(WifiError::unsupported())
    }

    fn roam(
        &mut self,
        _link: WifiLinkId,
        _parameters: WifiConnectParameters<'_>,
    ) -> Result<(), WifiError> {
        Err(WifiError::unsupported())
    }
}

impl WifiSecurityControlContract for UnsupportedWifiAdapter {
    fn clear_cached_security_state(&mut self) -> Result<(), WifiError> {
        Err(WifiError::unsupported())
    }

    fn set_management_frame_protection_required(
        &mut self,
        _required: bool,
    ) -> Result<(), WifiError> {
        Err(WifiError::unsupported())
    }
}

impl WifiAccessPointControlContract for UnsupportedWifiAdapter {
    fn start_access_point(
        &mut self,
        _configuration: WifiApConfiguration<'_>,
    ) -> Result<WifiAccessPointId, WifiError> {
        Err(WifiError::unsupported())
    }

    fn stop_access_point(&mut self, _ap: WifiAccessPointId) -> Result<(), WifiError> {
        Err(WifiError::unsupported())
    }

    fn associated_clients(
        &self,
        _ap: WifiAccessPointId,
        _out: &mut [WifiAssociatedClient],
    ) -> Result<usize, WifiError> {
        Err(WifiError::unsupported())
    }
}

impl WifiDataControlContract for UnsupportedWifiAdapter {
    fn transmit(
        &mut self,
        _link: WifiLinkId,
        _frame: WifiTransmitFrame<'_>,
    ) -> Result<(), WifiError> {
        Err(WifiError::unsupported())
    }

    fn receive<'a>(
        &mut self,
        _link: WifiLinkId,
        _frame: &'a mut [u8],
    ) -> Result<Option<WifiReceivedFrame<'a>>, WifiError> {
        Err(WifiError::unsupported())
    }
}

impl WifiMonitorControlContract for UnsupportedWifiAdapter {
    fn start_monitor(
        &mut self,
        _parameters: WifiMonitorParameters,
    ) -> Result<WifiMonitorSessionId, WifiError> {
        Err(WifiError::unsupported())
    }

    fn stop_monitor(&mut self, _session: WifiMonitorSessionId) -> Result<(), WifiError> {
        Err(WifiError::unsupported())
    }

    fn next_monitor_frame<'a>(
        &mut self,
        _session: WifiMonitorSessionId,
        _frame: &'a mut [u8],
    ) -> Result<Option<WifiReceivedFrame<'a>>, WifiError> {
        Err(WifiError::unsupported())
    }
}

impl WifiP2pControlContract for UnsupportedWifiAdapter {
    fn start_p2p_discovery(&mut self) -> Result<(), WifiError> {
        Err(WifiError::unsupported())
    }

    fn stop_p2p_discovery(&mut self) -> Result<(), WifiError> {
        Err(WifiError::unsupported())
    }
}

impl WifiMeshControlContract for UnsupportedWifiAdapter {
    fn join_mesh(
        &mut self,
        _configuration: WifiMeshConfiguration<'_>,
    ) -> Result<WifiMeshId, WifiError> {
        Err(WifiError::unsupported())
    }

    fn leave_mesh(&mut self, _mesh: WifiMeshId) -> Result<(), WifiError> {
        Err(WifiError::unsupported())
    }
}
