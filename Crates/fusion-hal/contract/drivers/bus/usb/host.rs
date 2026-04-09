//! USB host-side framework contracts.

use super::core::*;
use super::controller::*;
use super::error::*;
use super::topology::*;

/// Host-side lifecycle state for one enumerated USB function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsbHostDeviceState {
    Attached,
    Default,
    Addressed,
    Configured,
    Suspended,
    Detached,
}

/// One host-visible transfer submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbTransferRequest<'a> {
    pub endpoint: UsbEndpointAddress,
    pub transfer_type: UsbTransferType,
    pub payload: &'a [u8],
}

/// One host-visible transfer completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbTransferCompletion<'a> {
    pub bytes_transferred: usize,
    pub payload: &'a [u8],
}

/// Shared contract for one enumerated host-side device handle.
pub trait UsbHostDeviceContract: UsbCoreContract {
    /// Returns the assigned USB address.
    fn address(&self) -> UsbDeviceAddress;

    /// Returns the current host-visible lifecycle state.
    fn state(&self) -> UsbHostDeviceState;

    /// Returns the parsed device descriptor.
    fn device_descriptor(&self) -> UsbDeviceDescriptor;

    /// Returns the parsed active or selected configuration descriptor.
    fn configuration_descriptor(&self) -> Option<UsbConfigurationDescriptor>;

    /// Returns the interface descriptors currently visible for this function.
    fn interface_descriptors(&self) -> &[UsbInterfaceDescriptor];

    /// Returns the endpoint descriptors currently visible for this function.
    fn endpoint_descriptors(&self) -> &[UsbEndpointDescriptor];

    /// Executes one control transfer.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails, stalls, or the device disappears.
    fn control_transfer<'a>(
        &mut self,
        setup: UsbSetupPacket,
        data: &'a mut [u8],
    ) -> Result<&'a [u8], UsbError>;

    /// Submits one non-control transfer.
    ///
    /// # Errors
    ///
    /// Returns an error when the transfer cannot be scheduled or completed.
    fn submit_transfer<'a>(
        &mut self,
        request: UsbTransferRequest<'a>,
    ) -> Result<UsbTransferCompletion<'a>, UsbError>;
}

/// Shared contract for one host-side USB framework/controller surface.
pub trait UsbHostControllerContract: UsbTopologyContract + UsbControllerContract {
    /// Concrete device handle returned by this host stack.
    type Device: UsbHostDeviceContract;

    /// Enumerates one port and returns the resulting device handle.
    ///
    /// # Errors
    ///
    /// Returns an error when enumeration fails or the port is invalid.
    fn enumerate(&mut self, port: UsbPortId) -> Result<Self::Device, UsbError>;

    /// Returns one parsed descriptor blob directly from the device before or during binding.
    ///
    /// # Errors
    ///
    /// Returns an error when the device or descriptor cannot be reached.
    fn get_descriptor(
        &mut self,
        address: UsbDeviceAddress,
        descriptor_type: UsbDescriptorType,
        descriptor_index: u8,
        language_id: u16,
        buffer: &mut [u8],
    ) -> Result<usize, UsbError>;
}
