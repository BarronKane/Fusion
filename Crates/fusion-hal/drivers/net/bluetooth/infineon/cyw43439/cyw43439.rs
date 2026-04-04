//! Infineon CYW43439 Bluetooth driver implementation.

use crate::contract::drivers::net::bluetooth::{
    BluetoothAdapterDescriptor,
    BluetoothAdapterId,
    BluetoothAdvertisingControl,
    BluetoothAdvertisingParameters,
    BluetoothAdvertisingSetId,
    BluetoothAttAttributeHandle,
    BluetoothAttAttributeValue,
    BluetoothAttClient,
    BluetoothBase,
    BluetoothBondState,
    BluetoothConnectionControl,
    BluetoothConnectionDescriptor,
    BluetoothConnectionId,
    BluetoothConnectionParameters,
    BluetoothControl,
    BluetoothError,
    BluetoothGattAttributeValue,
    BluetoothGattCharacteristicHandle,
    BluetoothGattCharacteristicRange,
    BluetoothGattClient,
    BluetoothGattDescriptorRange,
    BluetoothGattServer,
    BluetoothGattServiceDefinition,
    BluetoothGattServiceRange,
    BluetoothL2capChannelDescriptor,
    BluetoothL2capChannelId,
    BluetoothL2capChannelParameters,
    BluetoothL2capControl,
    BluetoothL2capSdu,
    BluetoothOwnedAdapter,
    BluetoothPairingParameters,
    BluetoothRadioControl,
    BluetoothScanParameters,
    BluetoothScanReport,
    BluetoothScanSessionId,
    BluetoothScanningControl,
    BluetoothSecurityControl,
    BluetoothSupport,
};

#[path = "interface/interface.rs"]
pub mod interface;

mod unsupported;

use self::interface::contract::{
    Cyw43439ControllerCaps,
    Cyw43439Hardware,
};

/// Universal CYW43439 Bluetooth driver composed over one hardware-facing CYW43439 substrate.
#[derive(Debug)]
pub struct CYW43439<H: Cyw43439Hardware = unsupported::UnsupportedCyw43439Hardware> {
    hardware: Option<H>,
}

/// Opened CYW43439 Bluetooth adapter managed by the universal CYW43439 driver.
#[derive(Debug)]
pub struct Cyw43439Adapter<H: Cyw43439Hardware = unsupported::UnsupportedCyw43439Hardware> {
    descriptor: &'static BluetoothAdapterDescriptor,
    hardware: H,
}

impl<H> CYW43439<H>
where
    H: Cyw43439Hardware,
{
    /// Creates one universal CYW43439 Bluetooth provider over one hardware-facing substrate.
    #[must_use]
    pub fn new(hardware: H) -> Self {
        Self {
            hardware: Some(hardware),
        }
    }

    fn hardware(&self) -> Option<&H> {
        self.hardware.as_ref()
    }
}

impl Default for CYW43439<unsupported::UnsupportedCyw43439Hardware> {
    fn default() -> Self {
        Self::new(unsupported::UnsupportedCyw43439Hardware)
    }
}

impl<H> BluetoothBase for CYW43439<H>
where
    H: Cyw43439Hardware,
{
    fn support(&self) -> BluetoothSupport {
        self.hardware()
            .map_or_else(BluetoothSupport::unsupported, Cyw43439Hardware::support)
    }

    fn adapters(&self) -> &'static [BluetoothAdapterDescriptor] {
        self.hardware().map_or(&[], Cyw43439Hardware::adapters)
    }
}

impl<H> BluetoothControl for CYW43439<H>
where
    H: Cyw43439Hardware,
{
    type Adapter = Cyw43439Adapter<H>;

    fn open_adapter(
        &mut self,
        adapter: BluetoothAdapterId,
    ) -> Result<Self::Adapter, BluetoothError> {
        let mut hardware = self
            .hardware
            .take()
            .ok_or_else(BluetoothError::state_conflict)?;
        let Some(descriptor) = hardware
            .adapters()
            .iter()
            .find(|descriptor| descriptor.id == adapter)
        else {
            self.hardware = Some(hardware);
            return Err(BluetoothError::invalid());
        };

        if let Err(error) = hardware.claim_controller(adapter) {
            self.hardware = Some(hardware);
            return Err(error);
        }

        Ok(Cyw43439Adapter {
            descriptor,
            hardware,
        })
    }
}

impl<H> Drop for Cyw43439Adapter<H>
where
    H: Cyw43439Hardware,
{
    fn drop(&mut self) {
        self.hardware.release_controller(self.descriptor.id);
    }
}

impl<H> Cyw43439Adapter<H>
where
    H: Cyw43439Hardware,
{
    fn adapter_id(&self) -> BluetoothAdapterId {
        self.descriptor.id
    }

    fn unsupported<T>() -> Result<T, BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    /// Returns the truthful controller-plumbing capability surface for this adapter binding.
    #[must_use]
    pub fn controller_caps(&self) -> Cyw43439ControllerCaps {
        self.hardware.controller_caps(self.adapter_id())
    }

    /// Asserts or deasserts the controller reset line.
    pub fn set_controller_reset(&mut self, asserted: bool) -> Result<(), BluetoothError> {
        self.hardware
            .set_controller_reset(self.adapter_id(), asserted)
    }

    /// Asserts or deasserts the controller wake line.
    pub fn set_controller_wake(&mut self, awake: bool) -> Result<(), BluetoothError> {
        self.hardware.set_controller_wake(self.adapter_id(), awake)
    }

    /// Waits for one controller interrupt indication.
    pub fn wait_for_controller_irq(
        &mut self,
        timeout_ms: Option<u32>,
    ) -> Result<bool, BluetoothError> {
        self.hardware
            .wait_for_controller_irq(self.adapter_id(), timeout_ms)
    }

    /// Acknowledges one pending controller interrupt indication.
    pub fn acknowledge_controller_irq(&mut self) -> Result<(), BluetoothError> {
        self.hardware.acknowledge_controller_irq(self.adapter_id())
    }

    /// Writes one raw controller transport frame.
    pub fn write_controller_transport(&mut self, payload: &[u8]) -> Result<(), BluetoothError> {
        self.hardware
            .write_controller_transport(self.adapter_id(), payload)
    }

    /// Reads one raw controller transport frame into caller-owned storage.
    pub fn read_controller_transport(&mut self, out: &mut [u8]) -> Result<usize, BluetoothError> {
        self.hardware
            .read_controller_transport(self.adapter_id(), out)
    }

    /// Returns one optional controller firmware image.
    pub fn firmware_image(&self) -> Result<Option<&'static [u8]>, BluetoothError> {
        self.hardware.firmware_image(self.adapter_id())
    }

    /// Returns one optional controller NVRAM/config image.
    pub fn nvram_image(&self) -> Result<Option<&'static [u8]>, BluetoothError> {
        self.hardware.nvram_image(self.adapter_id())
    }

    /// Sleeps for one board-truthful delay interval.
    pub fn delay_ms(&self, milliseconds: u32) {
        self.hardware.delay_ms(milliseconds);
    }
}

impl<H> BluetoothOwnedAdapter for Cyw43439Adapter<H>
where
    H: Cyw43439Hardware,
{
    fn descriptor(&self) -> &'static BluetoothAdapterDescriptor {
        self.descriptor
    }
}

impl<H> BluetoothRadioControl for Cyw43439Adapter<H>
where
    H: Cyw43439Hardware,
{
    fn set_powered(&mut self, powered: bool) -> Result<(), BluetoothError> {
        self.hardware
            .set_controller_powered(self.adapter_id(), powered)
    }

    fn is_powered(&self) -> Result<bool, BluetoothError> {
        self.hardware.controller_powered(self.adapter_id())
    }
}

impl<H> BluetoothScanningControl for Cyw43439Adapter<H>
where
    H: Cyw43439Hardware,
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

impl<H> BluetoothAdvertisingControl for Cyw43439Adapter<H>
where
    H: Cyw43439Hardware,
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

impl<H> BluetoothConnectionControl for Cyw43439Adapter<H>
where
    H: Cyw43439Hardware,
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

impl<H> BluetoothSecurityControl for Cyw43439Adapter<H>
where
    H: Cyw43439Hardware,
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

impl<H> BluetoothL2capControl for Cyw43439Adapter<H>
where
    H: Cyw43439Hardware,
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

impl<H> BluetoothAttClient for Cyw43439Adapter<H>
where
    H: Cyw43439Hardware,
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
        _value: &'a mut [u8],
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

impl<H> BluetoothGattClient for Cyw43439Adapter<H>
where
    H: Cyw43439Hardware,
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
        _value: &'a mut [u8],
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

impl<H> BluetoothGattServer for Cyw43439Adapter<H>
where
    H: Cyw43439Hardware,
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
