//! Unsupported hardware-facing USB substrate used when no SoC or interface backend is selected.

use fusion_hal::contract::drivers::bus::usb::{
    ThunderboltMetadata,
    UsbControllerCapabilities,
    UsbControllerContract,
    UsbControllerMetadata,
    Usb4Metadata,
    Usb4RouterState,
    UsbCoreMetadata,
    UsbDeviceControllerContract,
    UsbDeviceDescriptor,
    UsbDeviceEndpointConfiguration,
    UsbDeviceState,
    UsbError,
    UsbHostControllerContract,
    UsbPortId,
    UsbPortStatus,
    UsbSetupPacket,
    UsbSupport,
    UsbTypecPortStatus,
    UsbPdContractState,
};
use crate::interface::contract::{
    UsbHardware,
    UsbHardwarePd,
    UsbHardwareThunderbolt,
    UsbHardwareTopology,
    UsbHardwareTypec,
    UsbHardwareUsb4,
};

/// Unsupported USB hardware substrate.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedUsbHardware;

/// Unsupported host-controller placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedUsbHostController;

/// Unsupported device-controller placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedUsbDeviceController;

impl UsbHardware for UnsupportedUsbHardware {
    type HostController = UnsupportedUsbHostController;
    type DeviceController = UnsupportedUsbDeviceController;

    fn support() -> UsbSupport {
        UsbSupport::unsupported()
    }

    fn core_metadata() -> UsbCoreMetadata {
        UsbCoreMetadata::default()
    }

    fn host_controller() -> Result<Option<Self::HostController>, UsbError> {
        Ok(None)
    }

    fn device_controller() -> Result<Option<Self::DeviceController>, UsbError> {
        Ok(None)
    }
}

impl UsbHardwareTopology for UnsupportedUsbHardware {
    fn topology_port_count() -> usize {
        0
    }

    fn topology_port_status(_port: UsbPortId) -> Result<UsbPortStatus, UsbError> {
        Err(UsbError::unsupported())
    }
}

impl UsbHardwareTypec for UnsupportedUsbHardware {
    fn typec_status() -> Result<UsbTypecPortStatus<'static>, UsbError> {
        Err(UsbError::unsupported())
    }
}

impl UsbHardwarePd for UnsupportedUsbHardware {
    fn pd_contract_state() -> Result<UsbPdContractState<'static>, UsbError> {
        Err(UsbError::unsupported())
    }
}

impl UsbHardwareUsb4 for UnsupportedUsbHardware {
    fn usb4_metadata() -> Usb4Metadata {
        Usb4Metadata::default()
    }

    fn usb4_state() -> Result<Usb4RouterState, UsbError> {
        Err(UsbError::unsupported())
    }
}

impl UsbHardwareThunderbolt for UnsupportedUsbHardware {
    fn thunderbolt_metadata() -> ThunderboltMetadata {
        ThunderboltMetadata::default()
    }

    fn thunderbolt_active() -> Result<bool, UsbError> {
        Err(UsbError::unsupported())
    }
}

impl fusion_hal::contract::drivers::bus::usb::UsbCoreContract for UnsupportedUsbHostController {
    fn usb_support(&self) -> UsbSupport {
        UsbSupport::unsupported()
    }

    fn usb_core_metadata(&self) -> UsbCoreMetadata {
        UsbCoreMetadata::default()
    }
}

impl UsbControllerContract for UnsupportedUsbHostController {
    fn controller_metadata(&self) -> UsbControllerMetadata {
        UsbControllerMetadata::default()
    }

    fn controller_capabilities(&self) -> UsbControllerCapabilities {
        UsbControllerCapabilities::default()
    }
}

impl fusion_hal::contract::drivers::bus::usb::UsbTopologyContract for UnsupportedUsbHostController {
    fn port_count(&self) -> usize {
        0
    }

    fn port_status(&self, _port: UsbPortId) -> Result<UsbPortStatus, UsbError> {
        Err(UsbError::unsupported())
    }
}

impl UsbHostControllerContract for UnsupportedUsbHostController {
    type Device = UnsupportedUsbDeviceController;

    fn enumerate(&mut self, _port: UsbPortId) -> Result<Self::Device, UsbError> {
        Err(UsbError::unsupported())
    }

    fn get_descriptor(
        &mut self,
        _address: fusion_hal::contract::drivers::bus::usb::UsbDeviceAddress,
        _descriptor_type: fusion_hal::contract::drivers::bus::usb::UsbDescriptorType,
        _descriptor_index: u8,
        _language_id: u16,
        _buffer: &mut [u8],
    ) -> Result<usize, UsbError> {
        Err(UsbError::unsupported())
    }
}

impl fusion_hal::contract::drivers::bus::usb::UsbHostDeviceContract
    for UnsupportedUsbDeviceController
{
    fn address(&self) -> fusion_hal::contract::drivers::bus::usb::UsbDeviceAddress {
        fusion_hal::contract::drivers::bus::usb::UsbDeviceAddress(0)
    }

    fn state(&self) -> fusion_hal::contract::drivers::bus::usb::UsbHostDeviceState {
        fusion_hal::contract::drivers::bus::usb::UsbHostDeviceState::Detached
    }

    fn device_descriptor(&self) -> UsbDeviceDescriptor {
        UsbDeviceDescriptor {
            usb_revision: fusion_hal::contract::drivers::bus::usb::UsbSpecRevision::USB_2_0,
            device_class: 0,
            device_subclass: 0,
            device_protocol: 0,
            max_packet_size_ep0: 0,
            vendor_id: 0,
            product_id: 0,
            device_revision: 0,
            manufacturer_string_index: 0,
            product_string_index: 0,
            serial_number_string_index: 0,
            configuration_count: 0,
        }
    }

    fn configuration_descriptor(
        &self,
    ) -> Option<fusion_hal::contract::drivers::bus::usb::UsbConfigurationDescriptor> {
        None
    }

    fn interface_descriptors(
        &self,
    ) -> &[fusion_hal::contract::drivers::bus::usb::UsbInterfaceDescriptor] {
        &[]
    }

    fn endpoint_descriptors(
        &self,
    ) -> &[fusion_hal::contract::drivers::bus::usb::UsbEndpointDescriptor] {
        &[]
    }

    fn control_transfer<'a>(
        &mut self,
        _setup: UsbSetupPacket,
        _data: &'a mut [u8],
    ) -> Result<&'a [u8], UsbError> {
        Err(UsbError::unsupported())
    }

    fn submit_transfer<'a>(
        &mut self,
        _request: fusion_hal::contract::drivers::bus::usb::UsbTransferRequest<'a>,
    ) -> Result<fusion_hal::contract::drivers::bus::usb::UsbTransferCompletion<'a>, UsbError> {
        Err(UsbError::unsupported())
    }
}

impl fusion_hal::contract::drivers::bus::usb::UsbCoreContract for UnsupportedUsbDeviceController {
    fn usb_support(&self) -> UsbSupport {
        UsbSupport::unsupported()
    }

    fn usb_core_metadata(&self) -> UsbCoreMetadata {
        UsbCoreMetadata::default()
    }
}

impl UsbControllerContract for UnsupportedUsbDeviceController {
    fn controller_metadata(&self) -> UsbControllerMetadata {
        UsbControllerMetadata::default()
    }

    fn controller_capabilities(&self) -> UsbControllerCapabilities {
        UsbControllerCapabilities::default()
    }
}

impl UsbDeviceControllerContract for UnsupportedUsbDeviceController {
    fn device_state(&self) -> UsbDeviceState {
        UsbDeviceState::Detached
    }

    fn device_descriptor(&self) -> UsbDeviceDescriptor {
        <Self as fusion_hal::contract::drivers::bus::usb::UsbHostDeviceContract>::device_descriptor(
            self,
        )
    }

    fn configuration_descriptors(
        &self,
    ) -> &[fusion_hal::contract::drivers::bus::usb::UsbConfigurationDescriptor] {
        &[]
    }

    fn interface_descriptors(
        &self,
    ) -> &[fusion_hal::contract::drivers::bus::usb::UsbInterfaceDescriptor] {
        &[]
    }

    fn endpoint_descriptors(
        &self,
    ) -> &[fusion_hal::contract::drivers::bus::usb::UsbEndpointDescriptor] {
        &[]
    }

    fn configure_endpoint(
        &mut self,
        _endpoint: UsbDeviceEndpointConfiguration,
    ) -> Result<(), UsbError> {
        Err(UsbError::unsupported())
    }

    fn queue_in(
        &mut self,
        _endpoint: fusion_hal::contract::drivers::bus::usb::UsbEndpointAddress,
        _payload: &[u8],
    ) -> Result<(), UsbError> {
        Err(UsbError::unsupported())
    }

    fn dequeue_out<'a>(
        &mut self,
        _endpoint: fusion_hal::contract::drivers::bus::usb::UsbEndpointAddress,
        _buffer: &'a mut [u8],
    ) -> Result<&'a [u8], UsbError> {
        Err(UsbError::unsupported())
    }

    fn handle_setup<'a>(
        &mut self,
        _setup: UsbSetupPacket,
        _data: &'a mut [u8],
    ) -> Result<&'a [u8], UsbError> {
        Err(UsbError::unsupported())
    }
}
