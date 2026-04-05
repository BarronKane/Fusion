//! Infineon CYW43439 Wi-Fi driver implementation.

use core::marker::PhantomData;

use crate::contract::drivers::driver::{
    ActiveDriver,
    DriverActivation,
    DriverActivationContext,
    DriverBindingSource,
    DriverClass,
    DriverContract,
    DriverContractKey,
    DriverDiscoveryContext,
    DriverError,
    DriverMetadata,
    DriverRegistration,
    RegisteredDriver,
};
use crate::contract::drivers::net::NetVendorIdentity;
use crate::contract::drivers::net::wifi::{
    WifiAccessPointControlContract,
    WifiAccessPointId,
    WifiAdapterDescriptor,
    WifiAdapterId,
    WifiApConfiguration,
    WifiAssociatedClient,
    WifiBaseContract,
    WifiConnectParameters,
    WifiConnectionDescriptor,
    WifiControlContract,
    WifiDataControlContract,
    WifiError,
    WifiLinkId,
    WifiMeshConfiguration,
    WifiMeshControlContract,
    WifiMeshId,
    WifiMonitorControlContract,
    WifiMonitorParameters,
    WifiMonitorSessionId,
    WifiOwnedAdapterContract,
    WifiP2pControlContract,
    WifiRadioControlContract,
    WifiReceivedFrame,
    WifiScanControlContract,
    WifiScanParameters,
    WifiScanReport,
    WifiScanSessionId,
    WifiSecurityControlContract,
    WifiStationControlContract,
    WifiSupport,
};
use crate::drivers::net::chipset::infineon::cyw43439::{
    core::{
        Cyw43439Chipset,
    },
    interface::{
        backend::UnsupportedBackend,
        contract::{
            Cyw43439ControllerCaps,
            Cyw43439HardwareContract,
            Cyw43439Radio,
        },
    },
};

pub use crate::drivers::net::chipset::infineon::cyw43439::core::Cyw43439DriverContext;

pub(crate) const CYW43439_WIFI_VENDOR_IDENTITY: NetVendorIdentity = NetVendorIdentity {
    vendor: "Infineon",
    family: Some("AIROC"),
    package: Some("CYW43439"),
    product: "Wi-Fi + Bluetooth combo",
    advertised_interface: "2.4 GHz 802.11 b/g/n",
};

const CYW43439_WIFI_DRIVER_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("net.wifi")];
const CYW43439_WIFI_BINDING_SOURCES: [DriverBindingSource; 4] = [
    DriverBindingSource::StaticSoc,
    DriverBindingSource::BoardManifest,
    DriverBindingSource::Devicetree,
    DriverBindingSource::Manual,
];
const CYW43439_WIFI_DRIVER_METADATA: DriverMetadata = DriverMetadata {
    key: "net.wifi.infineon.cyw43439",
    class: DriverClass::Network,
    identity: CYW43439_WIFI_VENDOR_IDENTITY,
    contracts: &CYW43439_WIFI_DRIVER_CONTRACTS,
    binding_sources: &CYW43439_WIFI_BINDING_SOURCES,
    description: "Infineon AIROC CYW43439 Wi-Fi controller driver",
};

/// Discoverable binding surfaced by the CYW43439 Wi-Fi driver family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cyw43439Binding {
    pub adapter: WifiAdapterId,
}

/// Registerable CYW43439 Wi-Fi driver family marker.
#[derive(Debug, Clone, Copy, Default)]
pub struct Cyw43439Driver<H: Cyw43439HardwareContract = UnsupportedBackend> {
    marker: PhantomData<fn() -> H>,
}

fn cyw43439_wifi_driver_metadata() -> &'static DriverMetadata {
    &CYW43439_WIFI_DRIVER_METADATA
}

fn enumerate_cyw43439_wifi_bindings<H>(
    _registered: &RegisteredDriver<Cyw43439Driver<H>>,
    context: &mut DriverDiscoveryContext<'_>,
    out: &mut [Cyw43439Binding],
) -> Result<usize, DriverError>
where
    H: Cyw43439HardwareContract + 'static,
{
    let context = context.downcast_mut::<Cyw43439DriverContext<H>>()?;
    let chipset = context.chipset().ok_or_else(DriverError::state_conflict)?;
    let adapters = chipset.wifi_adapters();
    if adapters.is_empty() {
        return Ok(0);
    }
    if out.len() < adapters.len() {
        return Err(DriverError::resource_exhausted());
    }

    for (binding, descriptor) in out.iter_mut().zip(adapters.iter()) {
        *binding = Cyw43439Binding {
            adapter: descriptor.id,
        };
    }

    Ok(adapters.len())
}

fn activate_cyw43439_wifi_binding<H>(
    _registered: &RegisteredDriver<Cyw43439Driver<H>>,
    context: &mut DriverActivationContext<'_>,
    binding: Cyw43439Binding,
) -> Result<ActiveDriver<Cyw43439Driver<H>>, DriverError>
where
    H: Cyw43439HardwareContract + 'static,
{
    let context = context.downcast_mut::<Cyw43439DriverContext<H>>()?;
    let chipset = context
        .take_chipset()
        .ok_or_else(DriverError::state_conflict)?;

    if !chipset
        .wifi_adapters()
        .iter()
        .any(|descriptor| descriptor.id == binding.adapter)
    {
        context.replace_chipset(chipset);
        return Err(DriverError::invalid());
    }

    Ok(ActiveDriver::new(binding, CYW43439::new(chipset)))
}

/// Universal CYW43439 Wi-Fi driver composed over one shared CYW43439 chipset substrate.
#[derive(Debug)]
pub struct CYW43439<H: Cyw43439HardwareContract = UnsupportedBackend> {
    chipset: Option<Cyw43439Chipset<H>>,
}

/// Opened CYW43439 Wi-Fi adapter managed by the universal CYW43439 driver.
#[derive(Debug)]
pub struct Cyw43439Adapter<H: Cyw43439HardwareContract = UnsupportedBackend> {
    descriptor: &'static WifiAdapterDescriptor,
    chipset: Cyw43439Chipset<H>,
}

impl CYW43439 {
    /// Returns the canonical marketed identity for this chip's Wi-Fi surface.
    #[must_use]
    pub const fn vendor_identity() -> NetVendorIdentity {
        CYW43439_WIFI_VENDOR_IDENTITY
    }
}

impl<H> CYW43439<H>
where
    H: Cyw43439HardwareContract,
{
    /// Creates one universal CYW43439 Wi-Fi provider over one hardware-facing substrate.
    #[must_use]
    pub(crate) fn new(chipset: Cyw43439Chipset<H>) -> Self {
        Self {
            chipset: Some(chipset),
        }
    }

    fn chipset(&self) -> Option<&Cyw43439Chipset<H>> {
        self.chipset.as_ref()
    }
}

impl Default for CYW43439<UnsupportedBackend> {
    fn default() -> Self {
        Self::new(Cyw43439Chipset::new(UnsupportedBackend))
    }
}

impl<H> DriverContract for Cyw43439Driver<H>
where
    H: Cyw43439HardwareContract + 'static,
{
    type Binding = Cyw43439Binding;
    type Instance = CYW43439<H>;

    fn registration() -> DriverRegistration<Self> {
        DriverRegistration::new(
            cyw43439_wifi_driver_metadata,
            DriverActivation::new(
                enumerate_cyw43439_wifi_bindings::<H>,
                activate_cyw43439_wifi_binding::<H>,
            ),
        )
    }
}

impl<H> WifiBaseContract for CYW43439<H>
where
    H: Cyw43439HardwareContract,
{
    fn support(&self) -> WifiSupport {
        self.chipset()
            .map_or_else(WifiSupport::unsupported, Cyw43439Chipset::wifi_support)
    }

    fn adapters(&self) -> &'static [WifiAdapterDescriptor] {
        self.chipset().map_or(&[], Cyw43439Chipset::wifi_adapters)
    }
}

impl<H> WifiControlContract for CYW43439<H>
where
    H: Cyw43439HardwareContract,
{
    type Adapter = Cyw43439Adapter<H>;

    fn open_adapter(&mut self, adapter: WifiAdapterId) -> Result<Self::Adapter, WifiError> {
        let mut chipset = self.chipset.take().ok_or_else(WifiError::state_conflict)?;
        let Some(descriptor) = chipset
            .wifi_adapters()
            .iter()
            .find(|descriptor| descriptor.id == adapter)
        else {
            self.chipset = Some(chipset);
            return Err(WifiError::invalid());
        };

        if let Err(error) = chipset.claim_wifi() {
            self.chipset = Some(chipset);
            return Err(error);
        }

        Ok(Cyw43439Adapter {
            descriptor,
            chipset,
        })
    }
}

impl<H> Drop for Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn drop(&mut self) {
        self.chipset.release_wifi();
    }
}

impl<H> Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn unsupported<T>() -> Result<T, WifiError> {
        Err(WifiError::unsupported())
    }

    /// Returns the truthful controller-plumbing capability surface for this adapter binding.
    #[must_use]
    pub fn controller_caps(&self) -> Cyw43439ControllerCaps {
        self.chipset.controller_caps(Cyw43439Radio::Wifi)
    }

    /// Asserts or deasserts the controller reset line.
    pub fn set_controller_reset(&mut self, asserted: bool) -> Result<(), WifiError> {
        self.chipset.set_controller_reset_wifi(asserted)
    }

    /// Asserts or deasserts the controller wake line.
    pub fn set_controller_wake(&mut self, awake: bool) -> Result<(), WifiError> {
        self.chipset.set_controller_wake_wifi(awake)
    }

    /// Waits for one controller interrupt indication.
    pub fn wait_for_controller_irq(&mut self, timeout_ms: Option<u32>) -> Result<bool, WifiError> {
        self.chipset.wait_for_controller_irq_wifi(timeout_ms)
    }

    /// Acknowledges one pending controller interrupt indication.
    pub fn acknowledge_controller_irq(&mut self) -> Result<(), WifiError> {
        self.chipset.acknowledge_controller_irq_wifi()
    }

    /// Writes one raw controller transport frame.
    pub fn write_controller_transport(&mut self, payload: &[u8]) -> Result<(), WifiError> {
        self.chipset.write_controller_transport_wifi(payload)
    }

    /// Reads one raw controller transport frame into caller-owned storage.
    pub fn read_controller_transport(&mut self, out: &mut [u8]) -> Result<usize, WifiError> {
        self.chipset.read_controller_transport_wifi(out)
    }

    /// Returns one optional controller firmware image.
    pub fn firmware_image(&self) -> Result<Option<&'static [u8]>, WifiError> {
        self.chipset.firmware_image_wifi()
    }

    /// Returns one optional controller NVRAM/config image.
    pub fn nvram_image(&self) -> Result<Option<&'static [u8]>, WifiError> {
        self.chipset.nvram_image_wifi()
    }

    /// Sleeps for one board-truthful delay interval.
    pub fn delay_ms(&self, milliseconds: u32) {
        self.chipset.delay_ms(milliseconds);
    }
}

impl<H> WifiOwnedAdapterContract for Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn descriptor(&self) -> &'static WifiAdapterDescriptor {
        self.descriptor
    }
}

impl<H> WifiRadioControlContract for Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn set_powered(&mut self, powered: bool) -> Result<(), WifiError> {
        self.chipset.set_controller_powered_wifi(powered)
    }

    fn is_powered(&self) -> Result<bool, WifiError> {
        self.chipset.controller_powered_wifi()
    }

    fn current_channel(
        &self,
    ) -> Result<Option<crate::contract::drivers::net::wifi::WifiChannelDescriptor>, WifiError> {
        Self::unsupported()
    }

    fn set_channel(
        &mut self,
        _channel: crate::contract::drivers::net::wifi::WifiChannelDescriptor,
    ) -> Result<(), WifiError> {
        Self::unsupported()
    }
}

impl<H> WifiScanControlContract for Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn start_scan(
        &mut self,
        _parameters: WifiScanParameters,
    ) -> Result<WifiScanSessionId, WifiError> {
        Self::unsupported()
    }

    fn stop_scan(&mut self, _session: WifiScanSessionId) -> Result<(), WifiError> {
        Self::unsupported()
    }

    fn next_scan_report<'a>(
        &mut self,
        _session: WifiScanSessionId,
        _information_elements: &'a mut [u8],
    ) -> Result<Option<WifiScanReport<'a>>, WifiError> {
        Self::unsupported()
    }
}

impl<H> WifiStationControlContract for Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn connect(&mut self, _parameters: WifiConnectParameters<'_>) -> Result<WifiLinkId, WifiError> {
        Self::unsupported()
    }

    fn disconnect(&mut self, _link: WifiLinkId) -> Result<(), WifiError> {
        Self::unsupported()
    }

    fn connection(&self, _link: WifiLinkId) -> Result<WifiConnectionDescriptor, WifiError> {
        Self::unsupported()
    }

    fn current_station_link(&self) -> Result<Option<WifiLinkId>, WifiError> {
        Self::unsupported()
    }

    fn roam(
        &mut self,
        _link: WifiLinkId,
        _parameters: WifiConnectParameters<'_>,
    ) -> Result<(), WifiError> {
        Self::unsupported()
    }
}

impl<H> WifiSecurityControlContract for Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn clear_cached_security_state(&mut self) -> Result<(), WifiError> {
        Self::unsupported()
    }

    fn set_management_frame_protection_required(
        &mut self,
        _required: bool,
    ) -> Result<(), WifiError> {
        Self::unsupported()
    }
}

impl<H> WifiAccessPointControlContract for Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn start_access_point(
        &mut self,
        _configuration: WifiApConfiguration<'_>,
    ) -> Result<WifiAccessPointId, WifiError> {
        Self::unsupported()
    }

    fn stop_access_point(&mut self, _ap: WifiAccessPointId) -> Result<(), WifiError> {
        Self::unsupported()
    }

    fn associated_clients(
        &self,
        _ap: WifiAccessPointId,
        _out: &mut [WifiAssociatedClient],
    ) -> Result<usize, WifiError> {
        Self::unsupported()
    }
}

impl<H> WifiDataControlContract for Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn transmit(&mut self, _link: WifiLinkId, _payload: &[u8]) -> Result<(), WifiError> {
        Self::unsupported()
    }

    fn receive<'a>(
        &mut self,
        _link: WifiLinkId,
        _frame: &'a mut [u8],
    ) -> Result<Option<WifiReceivedFrame<'a>>, WifiError> {
        Self::unsupported()
    }
}

impl<H> WifiMonitorControlContract for Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn start_monitor(
        &mut self,
        _parameters: WifiMonitorParameters,
    ) -> Result<WifiMonitorSessionId, WifiError> {
        Self::unsupported()
    }

    fn stop_monitor(&mut self, _session: WifiMonitorSessionId) -> Result<(), WifiError> {
        Self::unsupported()
    }

    fn next_monitor_frame<'a>(
        &mut self,
        _session: WifiMonitorSessionId,
        _frame: &'a mut [u8],
    ) -> Result<Option<WifiReceivedFrame<'a>>, WifiError> {
        Self::unsupported()
    }
}

impl<H> WifiP2pControlContract for Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn start_p2p_discovery(&mut self) -> Result<(), WifiError> {
        Self::unsupported()
    }

    fn stop_p2p_discovery(&mut self) -> Result<(), WifiError> {
        Self::unsupported()
    }
}

impl<H> WifiMeshControlContract for Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn join_mesh(
        &mut self,
        _configuration: WifiMeshConfiguration<'_>,
    ) -> Result<WifiMeshId, WifiError> {
        Self::unsupported()
    }

    fn leave_mesh(&mut self, _mesh: WifiMeshId) -> Result<(), WifiError> {
        Self::unsupported()
    }
}
