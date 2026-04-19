//! Infineon CYW43439 Bluetooth driver implementation.

use core::marker::PhantomData;

use fusion_hal::contract::drivers::driver::{
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
    DriverUsefulness,
    RegisteredDriver,
};
use fusion_hal::contract::drivers::net::NetVendorIdentity;
use fusion_hal::contract::drivers::net::bluetooth::{
    BLUETOOTH_HCI_EVENT_COMMAND_COMPLETE,
    BLUETOOTH_HCI_OPCODE_LE_SET_ADVERTISING_DATA,
    BLUETOOTH_HCI_OPCODE_LE_SET_ADVERTISING_ENABLE,
    BLUETOOTH_HCI_OPCODE_LE_SET_ADVERTISING_PARAMETERS,
    BLUETOOTH_HCI_OPCODE_LE_SET_SCAN_RESPONSE_DATA,
    BLUETOOTH_HCI_OPCODE_READ_BD_ADDR,
    BLUETOOTH_HCI_OPCODE_RESET,
    BluetoothAddress,
    BluetoothAddressKind,
    BluetoothAdapterDescriptor,
    BluetoothAdapterId,
    BluetoothAdvertisingControlContract,
    BluetoothAdvertisingMode,
    BluetoothAdvertisingParameters,
    BluetoothAdvertisingSetId,
    BluetoothAttAttributeHandle,
    BluetoothAttAttributeValue,
    BluetoothAttClientContract,
    BluetoothBaseContract,
    BluetoothBondState,
    BluetoothCanonicalFrame,
    BluetoothCanonicalFrameControlContract,
    BluetoothConnectionControlContract,
    BluetoothConnectionDescriptor,
    BluetoothConnectionId,
    BluetoothConnectionParameters,
    BluetoothControlContract,
    BluetoothError,
    BluetoothHciCommandComplete,
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
    BluetoothHciAclFrame,
    BluetoothHciCommandHeader,
    BluetoothHciCommandFrame,
    BluetoothHciEventFrame,
    BluetoothHciFrame,
    BluetoothHciFrameView,
    BluetoothHciLeAdvertisingChannelMap,
    BluetoothHciLeAdvertisingData,
    BluetoothHciLeAdvertisingFilterPolicy,
    BluetoothHciLeAdvertisingParameters,
    BluetoothHciLeAdvertisingType,
    BluetoothHciLeOwnAddressType,
    BluetoothHciLePeerAddressType,
    BluetoothHciPacketType,
    BluetoothLePhy,
};
use crate::{
    core::{
        Cyw43439Chipset,
    },
    transport::bluetooth::Cyw43439BluetoothTransportLease,
    interface::{
        backend::UnsupportedBackend,
        contract::{
            Cyw43439HardwareContract,
            Cyw43439Radio,
        },
    },
};

pub use crate::core::Cyw43439DriverContext;

pub const CYW43439_BLUETOOTH_VENDOR_IDENTITY: NetVendorIdentity = NetVendorIdentity {
    vendor: "Infineon",
    family: Some("AIROC"),
    package: Some("CYW43439"),
    product: "Wi-Fi + Bluetooth combo",
    advertised_interface: "Bluetooth 5.2",
};

const CYW43439_BLUETOOTH_DRIVER_CONTRACTS: [DriverContractKey; 1] =
    [DriverContractKey("net.bluetooth")];
const CYW43439_BLUETOOTH_REQUIRED_CONTRACTS: [DriverContractKey; 0] = [];
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
    required_contracts: &CYW43439_BLUETOOTH_REQUIRED_CONTRACTS,
    usefulness: DriverUsefulness::Standalone,
    singleton_class: None,
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

pub fn driver_metadata() -> &'static DriverMetadata {
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
    active_advertising_set: Option<BluetoothAdvertisingSetId>,
}

const CYW43439_ADVERTISING_SET_ID: BluetoothAdvertisingSetId = BluetoothAdvertisingSetId(0);
const CYW43439_BLUETOOTH_VENDOR_SET_PUBLIC_BD_ADDR_OPCODE: u16 = 0xFC01;
const CYW43439_HCI_SEND_SCRATCH_BYTES: usize = 320;
const CYW43439_HCI_READ_BUFFER_BYTES: usize = 272;

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
            driver_metadata,
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
            active_advertising_set: None,
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

    fn send_hci_command(
        &mut self,
        opcode: u16,
        parameters: &[u8],
        scratch: &mut [u8],
    ) -> Result<(), BluetoothError> {
        self.send_frame(
            BluetoothCanonicalFrame::Hci(BluetoothHciFrameView::Command(
                BluetoothHciCommandFrame {
                    header: BluetoothHciCommandHeader {
                        opcode,
                        parameter_length: parameters.len() as u8,
                    },
                    parameters,
                },
            )),
            scratch,
        )
    }

    fn frame_command_complete(
        frame: BluetoothCanonicalFrame<'_>,
    ) -> Option<BluetoothHciCommandComplete<'_>> {
        match frame {
            BluetoothCanonicalFrame::Hci(BluetoothHciFrameView::Event(event)) => {
                event.as_command_complete()
            }
            BluetoothCanonicalFrame::Hci(BluetoothHciFrameView::Opaque(BluetoothHciFrame {
                packet_type: BluetoothHciPacketType::Event,
                bytes,
            })) => {
                if bytes.len() < 5 || bytes[0] != BLUETOOTH_HCI_EVENT_COMMAND_COMPLETE {
                    return None;
                }
                let parameter_length = usize::from(bytes[1]);
                if bytes.len() != 2 + parameter_length || parameter_length < 3 {
                    return None;
                }
                Some(BluetoothHciCommandComplete {
                    num_hci_command_packets: bytes[2],
                    opcode: u16::from_le_bytes([bytes[3], bytes[4]]),
                    return_parameters: &bytes[5..],
                })
            }
            _ => None,
        }
    }

    fn wait_for_command_complete<T>(
        &mut self,
        expected_opcode: u16,
        read_buffer: &mut [u8],
        parser: impl Fn(BluetoothHciCommandComplete<'_>) -> Option<T>,
    ) -> Result<T, BluetoothError> {
        for _ in 0..4_096 {
            if self.wait_frame(Some(0))? {
                if let Some(frame) = self.recv_frame(read_buffer)? {
                    if let Some(command_complete) = Self::frame_command_complete(frame) {
                        if command_complete.opcode == expected_opcode {
                            return parser(command_complete).ok_or_else(BluetoothError::invalid);
                        }
                    }
                }
            }
        }
        Err(BluetoothError::timed_out())
    }

    fn wait_for_command_complete_status_zero(
        &mut self,
        expected_opcode: u16,
        read_buffer: &mut [u8],
    ) -> Result<(), BluetoothError> {
        self.wait_for_command_complete(expected_opcode, read_buffer, |command_complete| {
            command_complete
                .return_parameters
                .first()
                .copied()
                .filter(|status| *status == 0)
                .map(|_| ())
        })
    }

    fn configure_public_identity(&mut self) -> Result<(), BluetoothError> {
        let mut send_scratch = [0_u8; CYW43439_HCI_SEND_SCRATCH_BYTES];
        let mut read_buffer = [0_u8; CYW43439_HCI_READ_BUFFER_BYTES];
        self.send_hci_command(BLUETOOTH_HCI_OPCODE_READ_BD_ADDR, &[], &mut send_scratch)?;
        let (_, bd_addr) = self.wait_for_command_complete(
            BLUETOOTH_HCI_OPCODE_READ_BD_ADDR,
            &mut read_buffer,
            |command_complete| {
                command_complete
                    .bd_addr()
                    .filter(|(status, _)| *status == 0)
            },
        )?;
        if bd_addr.kind != BluetoothAddressKind::Public {
            return Err(BluetoothError::unsupported());
        }
        self.send_hci_command(
            CYW43439_BLUETOOTH_VENDOR_SET_PUBLIC_BD_ADDR_OPCODE,
            &bd_addr.bytes,
            &mut send_scratch,
        )?;
        self.wait_for_command_complete_status_zero(
            CYW43439_BLUETOOTH_VENDOR_SET_PUBLIC_BD_ADDR_OPCODE,
            &mut read_buffer,
        )
    }

    fn map_legacy_advertising_parameters(
        parameters: BluetoothAdvertisingParameters,
    ) -> Result<BluetoothHciLeAdvertisingParameters, BluetoothError> {
        if parameters.anonymous {
            return Err(BluetoothError::unsupported());
        }
        if parameters.primary_phy != BluetoothLePhy::Le1M || parameters.secondary_phy.is_some() {
            return Err(BluetoothError::unsupported());
        }
        if parameters.interval_min_units == 0
            || parameters.interval_max_units == 0
            || parameters.interval_min_units > parameters.interval_max_units
            || parameters.interval_max_units > u16::MAX as u32
        {
            return Err(BluetoothError::invalid());
        }

        let advertising_type = match parameters.mode {
            BluetoothAdvertisingMode::ConnectableUndirected
                if parameters.connectable && parameters.scannable =>
            {
                BluetoothHciLeAdvertisingType::ConnectableUndirected
            }
            BluetoothAdvertisingMode::ScannableUndirected
                if !parameters.connectable && parameters.scannable =>
            {
                BluetoothHciLeAdvertisingType::ScannableUndirected
            }
            BluetoothAdvertisingMode::NonConnectableUndirected
                if !parameters.connectable && !parameters.scannable =>
            {
                BluetoothHciLeAdvertisingType::NonConnectableUndirected
            }
            BluetoothAdvertisingMode::DirectedHighDuty
            | BluetoothAdvertisingMode::DirectedLowDuty => {
                return Err(BluetoothError::unsupported());
            }
            _ => return Err(BluetoothError::invalid()),
        };

        Ok(BluetoothHciLeAdvertisingParameters {
            interval_min: parameters.interval_min_units as u16,
            interval_max: parameters.interval_max_units as u16,
            advertising_type,
            own_address_type: BluetoothHciLeOwnAddressType::PublicDevice,
            peer_address_type: BluetoothHciLePeerAddressType::PublicDevice,
            peer_address: BluetoothAddress {
                bytes: [0; 6],
                kind: BluetoothAddressKind::Public,
            },
            channel_map: BluetoothHciLeAdvertisingChannelMap::ALL,
            filter_policy: BluetoothHciLeAdvertisingFilterPolicy::ProcessAll,
        })
    }

    fn send_hci_frame(
        &mut self,
        frame: BluetoothHciFrameView<'_>,
        scratch: &mut [u8],
    ) -> Result<(), BluetoothError> {
        self.chipset.with_driver_activity(|chipset| {
            let mut transport = Cyw43439BluetoothTransportLease::acquire(&mut chipset.hardware)
                .map_err(crate::core::map_bluetooth_error)?;
            match frame {
                BluetoothHciFrameView::Command(BluetoothHciCommandFrame { header, parameters }) => {
                    transport
                        .write_command(header, parameters, scratch)
                        .map_err(crate::core::map_bluetooth_error)?;
                    Ok(())
                }
                BluetoothHciFrameView::Acl(BluetoothHciAclFrame { header, payload }) => {
                    transport
                        .write_acl(header, payload, scratch)
                        .map_err(crate::core::map_bluetooth_error)?;
                    Ok(())
                }
                BluetoothHciFrameView::Sco(payload) => {
                    transport
                        .write_packet(BluetoothHciPacketType::ScoData, payload, scratch)
                        .map_err(crate::core::map_bluetooth_error)?;
                    Ok(())
                }
                BluetoothHciFrameView::Iso(payload) => {
                    transport
                        .write_packet(BluetoothHciPacketType::IsoData, payload, scratch)
                        .map_err(crate::core::map_bluetooth_error)?;
                    Ok(())
                }
                BluetoothHciFrameView::Opaque(BluetoothHciFrame { packet_type, bytes }) => {
                    if packet_type == BluetoothHciPacketType::Event {
                        return Err(BluetoothError::invalid());
                    }
                    transport
                        .write_packet(packet_type, bytes, scratch)
                        .map_err(crate::core::map_bluetooth_error)?;
                    Ok(())
                }
                BluetoothHciFrameView::Event(_) => Err(BluetoothError::invalid()),
            }
        })
    }

    fn parse_hci_frame<'a>(
        packet_type: BluetoothHciPacketType,
        bytes: &'a [u8],
    ) -> BluetoothCanonicalFrame<'a> {
        let view = match packet_type {
            BluetoothHciPacketType::Command => {
                if bytes.len() >= fusion_hal::contract::drivers::net::bluetooth::BluetoothHciCommandHeader::ENCODED_LEN
                {
                    let header =
                        fusion_hal::contract::drivers::net::bluetooth::BluetoothHciCommandHeader::decode([
                            bytes[0], bytes[1], bytes[2],
                        ]);
                    let declared = usize::from(header.parameter_length);
                    if bytes.len() == 3 + declared {
                        BluetoothHciFrameView::Command(BluetoothHciCommandFrame {
                            header,
                            parameters: &bytes[3..],
                        })
                    } else {
                        BluetoothHciFrameView::Opaque(BluetoothHciFrame { packet_type, bytes })
                    }
                } else {
                    BluetoothHciFrameView::Opaque(BluetoothHciFrame { packet_type, bytes })
                }
            }
            BluetoothHciPacketType::Event => {
                if bytes.len()
                    >= fusion_hal::contract::drivers::net::bluetooth::BluetoothHciEventHeader::ENCODED_LEN
                {
                    let header =
                        fusion_hal::contract::drivers::net::bluetooth::BluetoothHciEventHeader::decode([
                            bytes[0], bytes[1],
                        ]);
                    let declared = usize::from(header.parameter_length);
                    if bytes.len() == 2 + declared {
                        BluetoothHciFrameView::Event(BluetoothHciEventFrame {
                            header,
                            parameters: &bytes[2..],
                        })
                    } else {
                        BluetoothHciFrameView::Opaque(BluetoothHciFrame { packet_type, bytes })
                    }
                } else {
                    BluetoothHciFrameView::Opaque(BluetoothHciFrame { packet_type, bytes })
                }
            }
            BluetoothHciPacketType::AclData => {
                if bytes.len()
                    >= fusion_hal::contract::drivers::net::bluetooth::BluetoothHciAclHeader::ENCODED_LEN
                {
                    let header =
                        fusion_hal::contract::drivers::net::bluetooth::BluetoothHciAclHeader::decode([
                            bytes[0], bytes[1], bytes[2], bytes[3],
                        ]);
                    let declared = usize::from(header.payload_length);
                    if bytes.len() == 4 + declared {
                        BluetoothHciFrameView::Acl(BluetoothHciAclFrame {
                            header,
                            payload: &bytes[4..],
                        })
                    } else {
                        BluetoothHciFrameView::Opaque(BluetoothHciFrame { packet_type, bytes })
                    }
                } else {
                    BluetoothHciFrameView::Opaque(BluetoothHciFrame { packet_type, bytes })
                }
            }
            BluetoothHciPacketType::ScoData => BluetoothHciFrameView::Sco(bytes),
            BluetoothHciPacketType::IsoData => BluetoothHciFrameView::Iso(bytes),
        };
        BluetoothCanonicalFrame::Hci(view)
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

impl<H> BluetoothCanonicalFrameControlContract for Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn wait_frame(&mut self, timeout_ms: Option<u32>) -> Result<bool, BluetoothError> {
        self.chipset
            .hardware
            .acquire_transport(Cyw43439Radio::Bluetooth)
            .map_err(crate::core::map_bluetooth_error)?;
        let wait = self
            .chipset
            .hardware
            .wait_for_controller_irq(Cyw43439Radio::Bluetooth, timeout_ms)
            .map_err(crate::core::map_bluetooth_error);
        self.chipset
            .hardware
            .release_transport(Cyw43439Radio::Bluetooth);
        wait
    }

    fn send_frame(
        &mut self,
        frame: BluetoothCanonicalFrame<'_>,
        scratch: &mut [u8],
    ) -> Result<(), BluetoothError> {
        match frame {
            BluetoothCanonicalFrame::Hci(frame) => self.send_hci_frame(frame, scratch),
            _ => Err(BluetoothError::unsupported()),
        }
    }

    fn recv_frame<'a>(
        &mut self,
        out: &'a mut [u8],
    ) -> Result<Option<BluetoothCanonicalFrame<'a>>, BluetoothError> {
        self.chipset.with_driver_activity(|chipset| {
            let mut transport = Cyw43439BluetoothTransportLease::acquire(&mut chipset.hardware)
                .map_err(crate::core::map_bluetooth_error)?;
            let Some((packet_type, bytes)) = transport
                .read_packet(out)
                .map_err(crate::core::map_bluetooth_error)?
            else {
                return Ok(None);
            };
            Ok(Some(Self::parse_hci_frame(packet_type, bytes)))
        })
    }
}

impl<H> BluetoothRadioControlContract for Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn set_powered(&mut self, powered: bool) -> Result<(), BluetoothError> {
        self.chipset.set_bluetooth_enabled(powered)?;
        if !powered {
            self.active_advertising_set = None;
        }
        Ok(())
    }

    fn is_powered(&self) -> Result<bool, BluetoothError> {
        self.chipset.bluetooth_enabled()
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
        parameters: BluetoothAdvertisingParameters,
        data: &[u8],
        scan_response: Option<&[u8]>,
    ) -> Result<BluetoothAdvertisingSetId, BluetoothError> {
        if self.active_advertising_set.is_some() {
            return Err(BluetoothError::busy());
        }

        let hci_parameters = Self::map_legacy_advertising_parameters(parameters)?;
        let advertising_data = BluetoothHciLeAdvertisingData { bytes: data }
            .encode()
            .ok_or_else(BluetoothError::invalid)?;
        let scan_response_data = match scan_response {
            Some(bytes) => Some(
                BluetoothHciLeAdvertisingData { bytes }
                    .encode()
                    .ok_or_else(BluetoothError::invalid)?,
            ),
            None => None,
        };
        if scan_response_data.is_some() && !parameters.scannable {
            return Err(BluetoothError::invalid());
        }

        let mut send_scratch = [0_u8; CYW43439_HCI_SEND_SCRATCH_BYTES];
        let mut read_buffer = [0_u8; CYW43439_HCI_READ_BUFFER_BYTES];

        self.send_hci_command(BLUETOOTH_HCI_OPCODE_RESET, &[], &mut send_scratch)?;
        self.wait_for_command_complete_status_zero(BLUETOOTH_HCI_OPCODE_RESET, &mut read_buffer)?;
        self.configure_public_identity()?;

        let encoded_parameters = hci_parameters.encode();
        self.send_hci_command(
            BLUETOOTH_HCI_OPCODE_LE_SET_ADVERTISING_PARAMETERS,
            &encoded_parameters,
            &mut send_scratch,
        )?;
        self.wait_for_command_complete_status_zero(
            BLUETOOTH_HCI_OPCODE_LE_SET_ADVERTISING_PARAMETERS,
            &mut read_buffer,
        )?;

        self.send_hci_command(
            BLUETOOTH_HCI_OPCODE_LE_SET_ADVERTISING_DATA,
            &advertising_data,
            &mut send_scratch,
        )?;
        self.wait_for_command_complete_status_zero(
            BLUETOOTH_HCI_OPCODE_LE_SET_ADVERTISING_DATA,
            &mut read_buffer,
        )?;

        if let Some(scan_response_data) = scan_response_data.as_ref() {
            self.send_hci_command(
                BLUETOOTH_HCI_OPCODE_LE_SET_SCAN_RESPONSE_DATA,
                scan_response_data,
                &mut send_scratch,
            )?;
            self.wait_for_command_complete_status_zero(
                BLUETOOTH_HCI_OPCODE_LE_SET_SCAN_RESPONSE_DATA,
                &mut read_buffer,
            )?;
        }

        self.send_hci_command(
            BLUETOOTH_HCI_OPCODE_LE_SET_ADVERTISING_ENABLE,
            &[1],
            &mut send_scratch,
        )?;
        self.wait_for_command_complete_status_zero(
            BLUETOOTH_HCI_OPCODE_LE_SET_ADVERTISING_ENABLE,
            &mut read_buffer,
        )?;

        self.active_advertising_set = Some(CYW43439_ADVERTISING_SET_ID);
        Ok(CYW43439_ADVERTISING_SET_ID)
    }

    fn stop_advertising(
        &mut self,
        advertising_set: BluetoothAdvertisingSetId,
    ) -> Result<(), BluetoothError> {
        if self.active_advertising_set != Some(advertising_set) {
            return Err(BluetoothError::invalid());
        }
        let mut send_scratch = [0_u8; CYW43439_HCI_SEND_SCRATCH_BYTES];
        let mut read_buffer = [0_u8; CYW43439_HCI_READ_BUFFER_BYTES];
        self.send_hci_command(
            BLUETOOTH_HCI_OPCODE_LE_SET_ADVERTISING_ENABLE,
            &[0],
            &mut send_scratch,
        )?;
        self.wait_for_command_complete_status_zero(
            BLUETOOTH_HCI_OPCODE_LE_SET_ADVERTISING_ENABLE,
            &mut read_buffer,
        )?;
        self.active_advertising_set = None;
        Ok(())
    }
}

impl<H> BluetoothConnectionControlContract for Cyw43439Adapter<H>
where
    H: Cyw43439HardwareContract,
{
    fn connect(
        &mut self,
        _peer: fusion_hal::contract::drivers::net::bluetooth::BluetoothAddress,
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
        _peer: fusion_hal::contract::drivers::net::bluetooth::BluetoothAddress,
    ) -> Result<(), BluetoothError> {
        Self::unsupported()
    }

    fn bond_state(
        &self,
        _peer: fusion_hal::contract::drivers::net::bluetooth::BluetoothAddress,
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
        _service: fusion_hal::contract::drivers::net::bluetooth::BluetoothGattServiceHandle,
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
