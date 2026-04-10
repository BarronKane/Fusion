//! USB device-controller / gadget contracts.

use super::core::*;
use super::controller::*;
use super::error::*;

/// Device-side lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsbDeviceState {
    Detached,
    Attached,
    Powered,
    Default,
    Addressed,
    Configured,
    Suspended,
}

/// Device-side endpoint configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbDeviceEndpointConfiguration {
    pub address: UsbEndpointAddress,
    pub transfer_type: UsbTransferType,
    pub max_packet_size: u16,
    pub interval: u8,
}

/// Shared contract for one USB device-controller / gadget surface.
pub trait UsbDeviceControllerContract: UsbControllerContract {
    /// Returns the current device-side lifecycle state.
    fn device_state(&self) -> UsbDeviceState;

    /// Returns the surfaced device descriptor.
    fn device_descriptor(&self) -> UsbDeviceDescriptor;

    /// Returns the surfaced configuration descriptors.
    fn configuration_descriptors(&self) -> &[UsbConfigurationDescriptor];

    /// Returns the surfaced interface descriptors for the active configuration model.
    fn interface_descriptors(&self) -> &[UsbInterfaceDescriptor];

    /// Returns the surfaced endpoint descriptors for the active configuration model.
    fn endpoint_descriptors(&self) -> &[UsbEndpointDescriptor];

    /// Realizes one endpoint configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when the endpoint cannot be configured.
    fn configure_endpoint(
        &mut self,
        endpoint: UsbDeviceEndpointConfiguration,
    ) -> Result<(), UsbError>;

    /// Queues one IN payload toward the host.
    ///
    /// # Errors
    ///
    /// Returns an error when the endpoint cannot accept the payload.
    fn queue_in(&mut self, endpoint: UsbEndpointAddress, payload: &[u8]) -> Result<(), UsbError>;

    /// Reads one OUT payload received from the host.
    ///
    /// # Errors
    ///
    /// Returns an error when the endpoint cannot be read.
    fn dequeue_out<'a>(
        &mut self,
        endpoint: UsbEndpointAddress,
        buffer: &'a mut [u8],
    ) -> Result<&'a [u8], UsbError>;

    /// Handles one control setup packet directed at the device.
    ///
    /// # Errors
    ///
    /// Returns an error when the request cannot be satisfied.
    fn handle_setup<'a>(
        &mut self,
        setup: UsbSetupPacket,
        data: &'a mut [u8],
    ) -> Result<&'a [u8], UsbError>;
}
