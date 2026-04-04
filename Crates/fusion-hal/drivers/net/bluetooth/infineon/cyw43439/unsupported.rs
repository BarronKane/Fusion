//! Unsupported CYW43439 hardware substrate used when no CYW43439 backend is selected.

use crate::contract::drivers::net::bluetooth::{
    BluetoothAdapterDescriptor,
    BluetoothAdapterId,
    BluetoothError,
    BluetoothSupport,
};
use crate::drivers::net::bluetooth::infineon::cyw43439::interface::contract::{
    Cyw43439ControllerCaps,
    Cyw43439Hardware,
};

/// Unsupported CYW43439 hardware substrate.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedCyw43439Hardware;

impl Cyw43439Hardware for UnsupportedCyw43439Hardware {
    fn support(&self) -> BluetoothSupport {
        BluetoothSupport::unsupported()
    }

    fn adapters(&self) -> &'static [BluetoothAdapterDescriptor] {
        &[]
    }

    fn controller_caps(&self, _adapter: BluetoothAdapterId) -> Cyw43439ControllerCaps {
        Cyw43439ControllerCaps::empty()
    }

    fn claim_controller(&mut self, _adapter: BluetoothAdapterId) -> Result<(), BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn release_controller(&mut self, _adapter: BluetoothAdapterId) {}

    fn controller_powered(&self, _adapter: BluetoothAdapterId) -> Result<bool, BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn set_controller_powered(
        &mut self,
        _adapter: BluetoothAdapterId,
        _powered: bool,
    ) -> Result<(), BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn set_controller_reset(
        &mut self,
        _adapter: BluetoothAdapterId,
        _asserted: bool,
    ) -> Result<(), BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn set_controller_wake(
        &mut self,
        _adapter: BluetoothAdapterId,
        _awake: bool,
    ) -> Result<(), BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn wait_for_controller_irq(
        &mut self,
        _adapter: BluetoothAdapterId,
        _timeout_ms: Option<u32>,
    ) -> Result<bool, BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn acknowledge_controller_irq(
        &mut self,
        _adapter: BluetoothAdapterId,
    ) -> Result<(), BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn write_controller_transport(
        &mut self,
        _adapter: BluetoothAdapterId,
        _payload: &[u8],
    ) -> Result<(), BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn read_controller_transport(
        &mut self,
        _adapter: BluetoothAdapterId,
        _out: &mut [u8],
    ) -> Result<usize, BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn firmware_image(
        &self,
        _adapter: BluetoothAdapterId,
    ) -> Result<Option<&'static [u8]>, BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn nvram_image(
        &self,
        _adapter: BluetoothAdapterId,
    ) -> Result<Option<&'static [u8]>, BluetoothError> {
        Err(BluetoothError::unsupported())
    }

    fn delay_ms(&self, _milliseconds: u32) {}
}
