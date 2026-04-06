//! Backend-neutral unsupported generic Bluetooth implementation.

use super::BluetoothAdapterSupport;
use super::BluetoothAdapterDescriptor;
use super::BluetoothAdapterId;
use super::BluetoothAdvertisingControlContract;
use super::BluetoothAdvertisingParameters;
use super::BluetoothAdvertisingSetId;
use super::BluetoothBaseContract;
use super::BluetoothBondState;
use super::BluetoothCanonicalFrame;
use super::BluetoothCanonicalFrameControlContract;
use super::BluetoothConnectionControlContract;
use super::BluetoothConnectionDescriptor;
use super::BluetoothConnectionId;
use super::BluetoothConnectionParameters;
use super::BluetoothError;
use super::BluetoothAttAttributeHandle;
use super::BluetoothAttAttributeValue;
use super::BluetoothAttClientContract;
use super::BluetoothGattAttributeValue;
use super::BluetoothGattCharacteristicHandle;
use super::BluetoothGattCharacteristicRange;
use super::BluetoothGattDescriptorRange;
use super::BluetoothGattClientContract;
use super::BluetoothGattServerContract;
use super::BluetoothL2capChannelDescriptor;
use super::BluetoothL2capChannelId;
use super::BluetoothL2capChannelParameters;
use super::BluetoothL2capControlContract;
use super::BluetoothL2capSdu;
use super::BluetoothGattServiceDefinition;
use super::BluetoothGattServiceRange;
use super::BluetoothOwnedAdapterContract;
use super::BluetoothPairingParameters;
use super::BluetoothRadioControlContract;
use super::BluetoothScanParameters;
use super::BluetoothScanReport;
use super::BluetoothScanSessionId;
use super::BluetoothScanningControlContract;
use super::BluetoothSecurityControlContract;
use super::BluetoothSupport;
use super::BluetoothControlContract;
use super::BluetoothVersion;
use super::BluetoothVersionRange;

/// Unsupported generic Bluetooth provider placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedBluetooth;

/// Unsupported opened Bluetooth adapter placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedBluetoothAdapter;

impl UnsupportedBluetooth {
    /// Creates a new unsupported generic Bluetooth provider placeholder.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl BluetoothBaseContract for UnsupportedBluetooth {
    fn support(&self) -> BluetoothSupport {
        BluetoothSupport::unsupported()
    }

    fn adapters(&self) -> &'static [BluetoothAdapterDescriptor] {
        &[]
    }
}

impl BluetoothControlContract for UnsupportedBluetooth {
    type Adapter = UnsupportedBluetoothAdapter;

    fn open_adapter(
        &mut self,
        _adapter: BluetoothAdapterId,
    ) -> Result<Self::Adapter, BluetoothError> {
        Err(BluetoothError::unsupported())
    }
}

impl BluetoothOwnedAdapterContract for UnsupportedBluetoothAdapter {
    fn descriptor(&self) -> &'static BluetoothAdapterDescriptor {
        static DESCRIPTOR: BluetoothAdapterDescriptor = BluetoothAdapterDescriptor {
            id: BluetoothAdapterId(0),
            name: "unsupported-bluetooth",
            vendor_identity: None,
            shared_chipset: false,
            address: None,
            version: BluetoothVersionRange {
                minimum: BluetoothVersion::new(0, 0),
                maximum: BluetoothVersion::new(0, 0),
            },
            support: BluetoothAdapterSupport {
                transports: super::BluetoothTransportCaps::empty(),
                roles: super::BluetoothRoleCaps::empty(),
                le_phys: super::BluetoothLePhyCaps::empty(),
                advertising: super::BluetoothAdvertisingCaps::empty(),
                scanning: super::BluetoothScanningCaps::empty(),
                connection: super::BluetoothConnectionCaps::empty(),
                security: super::BluetoothSecurityCaps::empty(),
                l2cap: super::BluetoothL2capCaps::empty(),
                att: super::BluetoothAttCaps::empty(),
                gatt: super::BluetoothGattCaps::empty(),
                iso: super::BluetoothIsoCaps::empty(),
                max_connections: 0,
                max_advertising_sets: 0,
                max_periodic_advertising_sets: 0,
                max_att_mtu: 0,
                max_attribute_value_len: 0,
                max_l2cap_channels: 0,
                max_l2cap_sdu_len: 0,
            },
        };

        &DESCRIPTOR
    }
}

impl BluetoothCanonicalFrameControlContract for UnsupportedBluetoothAdapter {
    fn wait_frame(&mut self, _timeout_ms: Option<u32>) -> Result<bool, BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn send_frame(
        &mut self,
        _frame: BluetoothCanonicalFrame<'_>,
        _scratch: &mut [u8],
    ) -> Result<(), BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn recv_frame<'a>(
        &mut self,
        _out: &'a mut [u8],
    ) -> Result<Option<BluetoothCanonicalFrame<'a>>, BluetoothError> {
        Err(BluetoothError::unsupported())
    }
}

impl BluetoothRadioControlContract for UnsupportedBluetoothAdapter {
    fn set_powered(&mut self, _powered: bool) -> Result<(), BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn is_powered(&self) -> Result<bool, BluetoothError> {
        Err(BluetoothError::unsupported())
    }
}

impl BluetoothScanningControlContract for UnsupportedBluetoothAdapter {
    fn start_scan(
        &mut self,
        _parameters: BluetoothScanParameters,
    ) -> Result<BluetoothScanSessionId, BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn stop_scan(&mut self, _session: BluetoothScanSessionId) -> Result<(), BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn next_scan_report<'a>(
        &mut self,
        _session: BluetoothScanSessionId,
        _data: &'a mut [u8],
    ) -> Result<Option<BluetoothScanReport<'a>>, BluetoothError> {
        Err(BluetoothError::unsupported())
    }
}

impl BluetoothAdvertisingControlContract for UnsupportedBluetoothAdapter {
    fn start_advertising(
        &mut self,
        _parameters: BluetoothAdvertisingParameters,
        _data: &[u8],
        _scan_response: Option<&[u8]>,
    ) -> Result<BluetoothAdvertisingSetId, BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn stop_advertising(
        &mut self,
        _advertising_set: BluetoothAdvertisingSetId,
    ) -> Result<(), BluetoothError> {
        Err(BluetoothError::unsupported())
    }
}

impl BluetoothConnectionControlContract for UnsupportedBluetoothAdapter {
    fn connect(
        &mut self,
        _peer: super::BluetoothAddress,
        _parameters: BluetoothConnectionParameters,
    ) -> Result<BluetoothConnectionId, BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn disconnect(&mut self, _connection: BluetoothConnectionId) -> Result<(), BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn connection(
        &self,
        _connection: BluetoothConnectionId,
    ) -> Result<BluetoothConnectionDescriptor, BluetoothError> {
        Err(BluetoothError::unsupported())
    }
}

impl BluetoothSecurityControlContract for UnsupportedBluetoothAdapter {
    fn pair(
        &mut self,
        _connection: BluetoothConnectionId,
        _parameters: BluetoothPairingParameters,
    ) -> Result<(), BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn delete_bond(&mut self, _peer: super::BluetoothAddress) -> Result<(), BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn bond_state(
        &self,
        _peer: super::BluetoothAddress,
    ) -> Result<BluetoothBondState, BluetoothError> {
        Err(BluetoothError::unsupported())
    }
}

impl BluetoothL2capControlContract for UnsupportedBluetoothAdapter {
    fn open_l2cap_channel(
        &mut self,
        _connection: BluetoothConnectionId,
        _parameters: BluetoothL2capChannelParameters,
    ) -> Result<BluetoothL2capChannelId, BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn close_l2cap_channel(
        &mut self,
        _channel: BluetoothL2capChannelId,
    ) -> Result<(), BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn l2cap_channel(
        &self,
        _channel: BluetoothL2capChannelId,
    ) -> Result<BluetoothL2capChannelDescriptor, BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn send_l2cap(
        &mut self,
        _channel: BluetoothL2capChannelId,
        _payload: &[u8],
    ) -> Result<(), BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn recv_l2cap<'a>(
        &mut self,
        _channel: BluetoothL2capChannelId,
        _out: &'a mut [u8],
    ) -> Result<Option<BluetoothL2capSdu<'a>>, BluetoothError> {
        Err(BluetoothError::unsupported())
    }
}

impl BluetoothAttClientContract for UnsupportedBluetoothAdapter {
    fn exchange_mtu(
        &mut self,
        _connection: BluetoothConnectionId,
        _preferred_mtu: u16,
    ) -> Result<u16, BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn read_attribute<'a>(
        &mut self,
        _connection: BluetoothConnectionId,
        _attribute: BluetoothAttAttributeHandle,
        _out: &'a mut [u8],
    ) -> Result<BluetoothAttAttributeValue<'a>, BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn write_attribute(
        &mut self,
        _connection: BluetoothConnectionId,
        _attribute: BluetoothAttAttributeHandle,
        _value: &[u8],
        _with_response: bool,
    ) -> Result<(), BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn prepare_write_attribute(
        &mut self,
        _connection: BluetoothConnectionId,
        _attribute: BluetoothAttAttributeHandle,
        _offset: u16,
        _value: &[u8],
    ) -> Result<(), BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn execute_prepared_writes(
        &mut self,
        _connection: BluetoothConnectionId,
        _commit: bool,
    ) -> Result<(), BluetoothError> {
        Err(BluetoothError::unsupported())
    }
}

impl BluetoothGattClientContract for UnsupportedBluetoothAdapter {
    fn discover_primary_services(
        &mut self,
        _connection: BluetoothConnectionId,
        _out: &mut [BluetoothGattServiceRange],
    ) -> Result<usize, BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn discover_characteristics(
        &mut self,
        _connection: BluetoothConnectionId,
        _service: super::BluetoothGattServiceHandle,
        _out: &mut [BluetoothGattCharacteristicRange],
    ) -> Result<usize, BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn discover_descriptors(
        &mut self,
        _connection: BluetoothConnectionId,
        _characteristic: BluetoothGattCharacteristicHandle,
        _out: &mut [BluetoothGattDescriptorRange],
    ) -> Result<usize, BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn read_characteristic<'a>(
        &mut self,
        _connection: BluetoothConnectionId,
        _characteristic: BluetoothGattCharacteristicHandle,
        _out: &'a mut [u8],
    ) -> Result<BluetoothGattAttributeValue<'a>, BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn write_characteristic(
        &mut self,
        _connection: BluetoothConnectionId,
        _characteristic: BluetoothGattCharacteristicHandle,
        _value: &[u8],
        _with_response: bool,
    ) -> Result<(), BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn subscribe(
        &mut self,
        _connection: BluetoothConnectionId,
        _characteristic: BluetoothGattCharacteristicHandle,
        _notify: bool,
        _indicate: bool,
    ) -> Result<(), BluetoothError> {
        Err(BluetoothError::unsupported())
    }
}

impl BluetoothGattServerContract for UnsupportedBluetoothAdapter {
    fn publish_services(
        &mut self,
        _services: &[BluetoothGattServiceDefinition<'_>],
    ) -> Result<(), BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn notify(
        &mut self,
        _connection: BluetoothConnectionId,
        _characteristic: BluetoothGattCharacteristicHandle,
        _value: &[u8],
    ) -> Result<(), BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn indicate(
        &mut self,
        _connection: BluetoothConnectionId,
        _characteristic: BluetoothGattCharacteristicHandle,
        _value: &[u8],
    ) -> Result<(), BluetoothError> {
        Err(BluetoothError::unsupported())
    }
}
