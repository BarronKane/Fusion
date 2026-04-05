//! Infineon CYW43439 Bluetooth driver implementation.

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
use crate::contract::drivers::net::bluetooth::{
    BluetoothAdapterDescriptor,
    BluetoothAdapterId,
    BluetoothAdvertisingControlContract,
    BluetoothAdvertisingParameters,
    BluetoothAdvertisingSetId,
    BluetoothAttAttributeHandle,
    BluetoothAttAttributeValue,
    BluetoothAttClientContract,
    BluetoothBaseContract,
    BluetoothBondState,
    BluetoothConnectionControlContract,
    BluetoothConnectionDescriptor,
    BluetoothConnectionId,
    BluetoothConnectionParameters,
    BluetoothControlContract,
    BluetoothError,
    BluetoothGattAttributeValue,
    BluetoothGattCharacteristicHandle,
    BluetoothGattCharacteristicRange,
    BluetoothGattClientContract,
    BluetoothGattDescriptorRange,
    BluetoothGattServerContract,
    BluetoothGattServiceDefinition,
    BluetoothGattServiceRange,
    BluetoothL2capChannelDescriptor,
    BluetoothL2capChannelId,
    BluetoothL2capChannelParameters,
    BluetoothL2capControlContract,
    BluetoothL2capSdu,
    BluetoothOwnedAdapterContract,
    BluetoothPairingParameters,
    BluetoothRadioControlContract,
    BluetoothScanParameters,
    BluetoothScanReport,
    BluetoothScanSessionId,
    BluetoothScanningControlContract,
    BluetoothSecurityControlContract,
    BluetoothSupport,
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
        },
    },
};

pub use crate::drivers::net::chipset::infineon::cyw43439::core::Cyw43439DriverContext;

pub(crate) const CYW43439_BLUETOOTH_VENDOR_IDENTITY: NetVendorIdentity = NetVendorIdentity {
    vendor: "Infineon",
    family: Some("AIROC"),
    package: Some("CYW43439"),
    product: "Wi-Fi + Bluetooth combo",
    advertised_interface: "Bluetooth 5.2",
};

const CYW43439_BLUETOOTH_DRIVER_CONTRACTS: [DriverContractKey; 1] =
    [DriverContractKey("net.bluetooth")];
const CYW43439_BLUETOOTH_BINDING_SOURCES: [DriverBindingSource; 4] = [
    DriverBindingSource::StaticSoc,
    DriverBindingSource::BoardManifest,
    DriverBindingSource::Devicetree,
    DriverBindingSource::Manual,
];
const CYW43439_BLUETOOTH_DRIVER_METADATA: DriverMetadata = DriverMetadata {
    key: "net.bluetooth.infineon.cyw43439",
    class: DriverClass::Network,
    identity: CYW43439_BLUETOOTH_VENDOR_IDENTITY,
    contracts: &CYW43439_BLUETOOTH_DRIVER_CONTRACTS,
    binding_sources: &CYW43439_BLUETOOTH_BINDING_SOURCES,
    description: "Infineon AIROC CYW43439 Bluetooth controller driver",
};

/// Discoverable binding surfaced by the CYW43439 Bluetooth driver family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cyw43439Binding {
    pub adapter: BluetoothAdapterId,
}

/// Registerable CYW43439 Bluetooth driver family marker.
#[derive(Debug, Clone, Copy, Default)]
pub struct Cyw43439Driver<H: Cyw43439HardwareContract = UnsupportedBackend> {
    marker: PhantomData<fn() -> H>,
}

fn cyw43439_bluetooth_driver_metadata() -> &'static DriverMetadata {
    &CYW43439_BLUETOOTH_DRIVER_METADATA
}

fn enumerate_cyw43439_bluetooth_bindings<H>(
    _registered: &RegisteredDriver<Cyw43439Driver<H>>,
    context: &mut DriverDiscoveryContext<'_>,
    out: &mut [Cyw43439Binding],
) -> Result<usize, DriverError>
where
    H: Cyw43439HardwareContract + 'static,
{
    let context = context.downcast_mut::<Cyw43439DriverContext<H>>()?;
    let chipset = context.chipset().ok_or_else(DriverError::state_conflict)?;
    let adapters = chipset.bluetooth_adapters();
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

fn activate_cyw43439_bluetooth_binding<H>(
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
        .bluetooth_adapters()
        .iter()
        .any(|descriptor| descriptor.id == binding.adapter)
    {
        context.replace_chipset(chipset);
        return Err(DriverError::invalid());
    }

    Ok(ActiveDriver::new(binding, CYW43439::new(chipset)))
}

/// Universal CYW43439 Bluetooth driver composed over one shared CYW43439 chipset substrate.
#[derive(Debug)]
pub struct CYW43439<H: Cyw43439HardwareContract = UnsupportedBackend> {
    chipset: Option<Cyw43439Chipset<H>>,
}

/// Opened CYW43439 Bluetooth adapter managed by the universal CYW43439 driver.
#[derive(Debug)]
pub struct Cyw43439Adapter<H: Cyw43439HardwareContract = UnsupportedBackend> {
    descriptor: &'static BluetoothAdapterDescriptor,
    chipset: Cyw43439Chipset<H>,
}

impl<H> CYW43439<H>
where
    H: Cyw43439HardwareContract,
{
    /// Creates one universal CYW43439 Bluetooth provider over one hardware-facing substrate.
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

impl CYW43439 {
    /// Returns the canonical marketed identity for this chip's Bluetooth surface.
    #[must_use]
    pub const fn vendor_identity() -> NetVendorIdentity {
        CYW43439_BLUETOOTH_VENDOR_IDENTITY
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
            cyw43439_bluetooth_driver_metadata,
            DriverActivation::new(
                enumerate_cyw43439_bluetooth_bindings::<H>,
                activate_cyw43439_bluetooth_binding::<H>,
            ),
        )
    }
}

impl<H> BluetoothBaseContract for CYW43439<H>
where
    H: Cyw43439HardwareContract,
{
    fn support(&self) -> BluetoothSupport {
        self.chipset().map_or_else(
            BluetoothSupport::unsupported,
            Cyw43439Chipset::bluetooth_support,
        )
    }

    fn adapters(&self) -> &'static [BluetoothAdapterDescriptor] {
        self.chipset()
            .map_or(&[], Cyw43439Chipset::bluetooth_adapters)
    }
}

impl<H> BluetoothControlContract for CYW43439<H>
where
    H: Cyw43439HardwareContract,
{
    type Adapter = Cyw43439Adapter<H>;

    fn open_adapter(
        &mut self,
        adapter: BluetoothAdapterId,
    ) -> Result<Self::Adapter, BluetoothError> {
        let mut chipset = self
            .chipset
            .take()
            .ok_or_else(BluetoothError::state_conflict)?;
        let Some(descriptor) = chipset
            .bluetooth_adapters()
            .iter()
            .find(|descriptor| descriptor.id == adapter)
        else {
            self.chipset = Some(chipset);
            return Err(BluetoothError::invalid());
        };

        if let Err(error) = chipset.claim_bluetooth() {
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
        self.chipset.release_bluetooth();
    }
}

impl<H> Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn unsupported<T>() -> Result<T, BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    /// Returns the truthful controller-plumbing capability surface for this adapter binding.
    #[must_use]
    pub fn controller_caps(&self) -> Cyw43439ControllerCaps {
        self.chipset
            .controller_caps(crate::drivers::net::chipset::infineon::cyw43439::interface::contract::Cyw43439Radio::Bluetooth)
    }

    /// Asserts or deasserts the controller reset line.
    pub fn set_controller_reset(&mut self, asserted: bool) -> Result<(), BluetoothError> {
        self.chipset.set_controller_reset_bluetooth(asserted)
    }

    /// Asserts or deasserts the controller wake line.
    pub fn set_controller_wake(&mut self, awake: bool) -> Result<(), BluetoothError> {
        self.chipset.set_controller_wake_bluetooth(awake)
    }

    /// Waits for one controller interrupt indication.
    pub fn wait_for_controller_irq(
        &mut self,
        timeout_ms: Option<u32>,
    ) -> Result<bool, BluetoothError> {
        self.chipset.wait_for_controller_irq_bluetooth(timeout_ms)
    }

    /// Acknowledges one pending controller interrupt indication.
    pub fn acknowledge_controller_irq(&mut self) -> Result<(), BluetoothError> {
        self.chipset.acknowledge_controller_irq_bluetooth()
    }

    /// Writes one raw controller transport frame.
    pub fn write_controller_transport(&mut self, payload: &[u8]) -> Result<(), BluetoothError> {
        self.chipset.write_controller_transport_bluetooth(payload)
    }

    /// Reads one raw controller transport frame into caller-owned storage.
    pub fn read_controller_transport(&mut self, out: &mut [u8]) -> Result<usize, BluetoothError> {
        self.chipset.read_controller_transport_bluetooth(out)
    }

    /// Returns one optional controller firmware image.
    pub fn firmware_image(&self) -> Result<Option<&'static [u8]>, BluetoothError> {
        self.chipset.firmware_image_bluetooth()
    }

    /// Returns one optional controller NVRAM/config image.
    pub fn nvram_image(&self) -> Result<Option<&'static [u8]>, BluetoothError> {
        self.chipset.nvram_image_bluetooth()
    }

    /// Sleeps for one board-truthful delay interval.
    pub fn delay_ms(&self, milliseconds: u32) {
        self.chipset.delay_ms(milliseconds);
    }
}

impl<H> BluetoothOwnedAdapterContract for Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn descriptor(&self) -> &'static BluetoothAdapterDescriptor {
        self.descriptor
    }
}

impl<H> BluetoothRadioControlContract for Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn set_powered(&mut self, powered: bool) -> Result<(), BluetoothError> {
        self.chipset.set_controller_powered_bluetooth(powered)
    }

    fn is_powered(&self) -> Result<bool, BluetoothError> {
        self.chipset.controller_powered_bluetooth()
    }
}

impl<H> BluetoothScanningControlContract for Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn start_scan(
        &mut self,
        _parameters: BluetoothScanParameters,
    ) -> Result<BluetoothScanSessionId, BluetoothError> {
        Self::unsupported()
    }

    fn stop_scan(&mut self, _session: BluetoothScanSessionId) -> Result<(), BluetoothError> {
        Self::unsupported()
    }

    fn next_scan_report<'a>(
        &mut self,
        _session: BluetoothScanSessionId,
        _data: &'a mut [u8],
    ) -> Result<Option<BluetoothScanReport<'a>>, BluetoothError> {
        Self::unsupported()
    }
}

impl<H> BluetoothAdvertisingControlContract for Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn start_advertising(
        &mut self,
        _parameters: BluetoothAdvertisingParameters,
        _data: &[u8],
        _scan_response: Option<&[u8]>,
    ) -> Result<BluetoothAdvertisingSetId, BluetoothError> {
        Self::unsupported()
    }

    fn stop_advertising(
        &mut self,
        _advertising_set: BluetoothAdvertisingSetId,
    ) -> Result<(), BluetoothError> {
        Self::unsupported()
    }
}

impl<H> BluetoothConnectionControlContract for Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn connect(
        &mut self,
        _peer: crate::contract::drivers::net::bluetooth::BluetoothAddress,
        _parameters: BluetoothConnectionParameters,
    ) -> Result<BluetoothConnectionId, BluetoothError> {
        Self::unsupported()
    }

    fn disconnect(&mut self, _connection: BluetoothConnectionId) -> Result<(), BluetoothError> {
        Self::unsupported()
    }

    fn connection(
        &self,
        _connection: BluetoothConnectionId,
    ) -> Result<BluetoothConnectionDescriptor, BluetoothError> {
        Self::unsupported()
    }
}

impl<H> BluetoothSecurityControlContract for Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn pair(
        &mut self,
        _connection: BluetoothConnectionId,
        _parameters: BluetoothPairingParameters,
    ) -> Result<(), BluetoothError> {
        Self::unsupported()
    }

    fn delete_bond(
        &mut self,
        _peer: crate::contract::drivers::net::bluetooth::BluetoothAddress,
    ) -> Result<(), BluetoothError> {
        Self::unsupported()
    }

    fn bond_state(
        &self,
        _peer: crate::contract::drivers::net::bluetooth::BluetoothAddress,
    ) -> Result<BluetoothBondState, BluetoothError> {
        Self::unsupported()
    }
}

impl<H> BluetoothL2capControlContract for Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn open_l2cap_channel(
        &mut self,
        _connection: BluetoothConnectionId,
        _parameters: BluetoothL2capChannelParameters,
    ) -> Result<BluetoothL2capChannelId, BluetoothError> {
        Self::unsupported()
    }

    fn close_l2cap_channel(
        &mut self,
        _channel: BluetoothL2capChannelId,
    ) -> Result<(), BluetoothError> {
        Self::unsupported()
    }

    fn l2cap_channel(
        &self,
        _channel: BluetoothL2capChannelId,
    ) -> Result<BluetoothL2capChannelDescriptor, BluetoothError> {
        Self::unsupported()
    }

    fn send_l2cap(
        &mut self,
        _channel: BluetoothL2capChannelId,
        _payload: &[u8],
    ) -> Result<(), BluetoothError> {
        Self::unsupported()
    }

    fn recv_l2cap<'a>(
        &mut self,
        _channel: BluetoothL2capChannelId,
        _out: &'a mut [u8],
    ) -> Result<Option<BluetoothL2capSdu<'a>>, BluetoothError> {
        Self::unsupported()
    }
}

impl<H> BluetoothAttClientContract for Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn exchange_mtu(
        &mut self,
        _connection: BluetoothConnectionId,
        _preferred_mtu: u16,
    ) -> Result<u16, BluetoothError> {
        Self::unsupported()
    }

    fn read_attribute<'a>(
        &mut self,
        _connection: BluetoothConnectionId,
        _attribute: BluetoothAttAttributeHandle,
        _out: &'a mut [u8],
    ) -> Result<BluetoothAttAttributeValue<'a>, BluetoothError> {
        Self::unsupported()
    }

    fn write_attribute(
        &mut self,
        _connection: BluetoothConnectionId,
        _attribute: BluetoothAttAttributeHandle,
        _value: &[u8],
        _with_response: bool,
    ) -> Result<(), BluetoothError> {
        Self::unsupported()
    }

    fn prepare_write_attribute(
        &mut self,
        _connection: BluetoothConnectionId,
        _attribute: BluetoothAttAttributeHandle,
        _offset: u16,
        _value: &[u8],
    ) -> Result<(), BluetoothError> {
        Self::unsupported()
    }

    fn execute_prepared_writes(
        &mut self,
        _connection: BluetoothConnectionId,
        _commit: bool,
    ) -> Result<(), BluetoothError> {
        Self::unsupported()
    }
}

impl<H> BluetoothGattClientContract for Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn discover_primary_services(
        &mut self,
        _connection: BluetoothConnectionId,
        _out: &mut [BluetoothGattServiceRange],
    ) -> Result<usize, BluetoothError> {
        Self::unsupported()
    }

    fn discover_characteristics(
        &mut self,
        _connection: BluetoothConnectionId,
        _service: crate::contract::drivers::net::bluetooth::BluetoothGattServiceHandle,
        _out: &mut [BluetoothGattCharacteristicRange],
    ) -> Result<usize, BluetoothError> {
        Self::unsupported()
    }

    fn discover_descriptors(
        &mut self,
        _connection: BluetoothConnectionId,
        _characteristic: BluetoothGattCharacteristicHandle,
        _out: &mut [BluetoothGattDescriptorRange],
    ) -> Result<usize, BluetoothError> {
        Self::unsupported()
    }

    fn read_characteristic<'a>(
        &mut self,
        _connection: BluetoothConnectionId,
        _characteristic: BluetoothGattCharacteristicHandle,
        _out: &'a mut [u8],
    ) -> Result<BluetoothGattAttributeValue<'a>, BluetoothError> {
        Self::unsupported()
    }

    fn write_characteristic(
        &mut self,
        _connection: BluetoothConnectionId,
        _characteristic: BluetoothGattCharacteristicHandle,
        _value: &[u8],
        _with_response: bool,
    ) -> Result<(), BluetoothError> {
        Self::unsupported()
    }

    fn subscribe(
        &mut self,
        _connection: BluetoothConnectionId,
        _characteristic: BluetoothGattCharacteristicHandle,
        _notify: bool,
        _indicate: bool,
    ) -> Result<(), BluetoothError> {
        Self::unsupported()
    }
}

impl<H> BluetoothGattServerContract for Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn publish_services(
        &mut self,
        _services: &[BluetoothGattServiceDefinition<'_>],
    ) -> Result<(), BluetoothError> {
        Self::unsupported()
    }

    fn notify(
        &mut self,
        _connection: BluetoothConnectionId,
        _characteristic: BluetoothGattCharacteristicHandle,
        _value: &[u8],
    ) -> Result<(), BluetoothError> {
        Self::unsupported()
    }

    fn indicate(
        &mut self,
        _connection: BluetoothConnectionId,
        _characteristic: BluetoothGattCharacteristicHandle,
        _value: &[u8],
    ) -> Result<(), BluetoothError> {
        Self::unsupported()
    }
}
