//! Internal shared CYW43439 chipset runtime helpers.

use crate::contract::drivers::net::bluetooth::{
    BluetoothAdapterDescriptor,
    BluetoothError,
    BluetoothSupport,
};
use crate::contract::drivers::net::wifi::{
    WifiAdapterDescriptor,
    WifiError,
    WifiSupport,
};
use crate::drivers::net::chipset::infineon::cyw43439::interface::{
    backend::UnsupportedBackend,
    contract::{
        Cyw43439ControllerCaps,
        Cyw43439Error,
        Cyw43439ErrorKind,
        Cyw43439HardwareContract,
        Cyw43439Radio,
    },
};

/// Shared internal CYW43439 chipset wrapper used by the Bluetooth and Wi-Fi driver facets.
#[derive(Debug)]
pub(crate) struct Cyw43439Chipset<H: Cyw43439HardwareContract = UnsupportedBackend> {
    hardware: H,
}

impl<H> Cyw43439Chipset<H>
where
    H: Cyw43439HardwareContract,
{
    #[must_use]
    pub(crate) fn new(hardware: H) -> Self {
        Self { hardware }
    }

    #[must_use]
    pub(crate) fn bluetooth_support(&self) -> BluetoothSupport {
        self.hardware.bluetooth_support()
    }

    #[must_use]
    pub(crate) fn bluetooth_adapters(&self) -> &'static [BluetoothAdapterDescriptor] {
        self.hardware.bluetooth_adapters()
    }

    #[must_use]
    pub(crate) fn wifi_support(&self) -> WifiSupport {
        self.hardware.wifi_support()
    }

    #[must_use]
    pub(crate) fn wifi_adapters(&self) -> &'static [WifiAdapterDescriptor] {
        self.hardware.wifi_adapters()
    }

    #[must_use]
    pub(crate) fn controller_caps(&self, radio: Cyw43439Radio) -> Cyw43439ControllerCaps {
        self.hardware.controller_caps(radio)
    }

    pub(crate) fn claim_bluetooth(&mut self) -> Result<(), BluetoothError> {
        self.hardware
            .claim_controller(Cyw43439Radio::Bluetooth)
            .map_err(map_bluetooth_error)
    }

    pub(crate) fn release_bluetooth(&mut self) {
        self.hardware.release_controller(Cyw43439Radio::Bluetooth);
    }

    pub(crate) fn claim_wifi(&mut self) -> Result<(), WifiError> {
        self.hardware
            .claim_controller(Cyw43439Radio::Wifi)
            .map_err(map_wifi_error)
    }

    pub(crate) fn release_wifi(&mut self) {
        self.hardware.release_controller(Cyw43439Radio::Wifi);
    }

    pub(crate) fn controller_powered_bluetooth(&self) -> Result<bool, BluetoothError> {
        self.hardware.controller_powered().map_err(map_bluetooth_error)
    }

    pub(crate) fn controller_powered_wifi(&self) -> Result<bool, WifiError> {
        self.hardware.controller_powered().map_err(map_wifi_error)
    }

    pub(crate) fn set_controller_powered_bluetooth(
        &mut self,
        powered: bool,
    ) -> Result<(), BluetoothError> {
        self.hardware
            .set_controller_powered(powered)
            .map_err(map_bluetooth_error)
    }

    pub(crate) fn set_controller_powered_wifi(&mut self, powered: bool) -> Result<(), WifiError> {
        self.hardware
            .set_controller_powered(powered)
            .map_err(map_wifi_error)
    }

    pub(crate) fn set_controller_reset_bluetooth(
        &mut self,
        asserted: bool,
    ) -> Result<(), BluetoothError> {
        self.hardware
            .set_controller_reset(asserted)
            .map_err(map_bluetooth_error)
    }

    pub(crate) fn set_controller_reset_wifi(&mut self, asserted: bool) -> Result<(), WifiError> {
        self.hardware
            .set_controller_reset(asserted)
            .map_err(map_wifi_error)
    }

    pub(crate) fn set_controller_wake_bluetooth(
        &mut self,
        awake: bool,
    ) -> Result<(), BluetoothError> {
        self.hardware
            .set_controller_wake(awake)
            .map_err(map_bluetooth_error)
    }

    pub(crate) fn set_controller_wake_wifi(&mut self, awake: bool) -> Result<(), WifiError> {
        self.hardware
            .set_controller_wake(awake)
            .map_err(map_wifi_error)
    }

    pub(crate) fn wait_for_controller_irq_bluetooth(
        &mut self,
        timeout_ms: Option<u32>,
    ) -> Result<bool, BluetoothError> {
        self.hardware
            .wait_for_controller_irq(Cyw43439Radio::Bluetooth, timeout_ms)
            .map_err(map_bluetooth_error)
    }

    pub(crate) fn wait_for_controller_irq_wifi(
        &mut self,
        timeout_ms: Option<u32>,
    ) -> Result<bool, WifiError> {
        self.hardware
            .wait_for_controller_irq(Cyw43439Radio::Wifi, timeout_ms)
            .map_err(map_wifi_error)
    }

    pub(crate) fn acknowledge_controller_irq_bluetooth(
        &mut self,
    ) -> Result<(), BluetoothError> {
        self.hardware
            .acknowledge_controller_irq(Cyw43439Radio::Bluetooth)
            .map_err(map_bluetooth_error)
    }

    pub(crate) fn acknowledge_controller_irq_wifi(&mut self) -> Result<(), WifiError> {
        self.hardware
            .acknowledge_controller_irq(Cyw43439Radio::Wifi)
            .map_err(map_wifi_error)
    }

    pub(crate) fn write_controller_transport_bluetooth(
        &mut self,
        payload: &[u8],
    ) -> Result<(), BluetoothError> {
        self.hardware
            .write_controller_transport(Cyw43439Radio::Bluetooth, payload)
            .map_err(map_bluetooth_error)
    }

    pub(crate) fn write_controller_transport_wifi(
        &mut self,
        payload: &[u8],
    ) -> Result<(), WifiError> {
        self.hardware
            .write_controller_transport(Cyw43439Radio::Wifi, payload)
            .map_err(map_wifi_error)
    }

    pub(crate) fn read_controller_transport_bluetooth(
        &mut self,
        out: &mut [u8],
    ) -> Result<usize, BluetoothError> {
        self.hardware
            .read_controller_transport(Cyw43439Radio::Bluetooth, out)
            .map_err(map_bluetooth_error)
    }

    pub(crate) fn read_controller_transport_wifi(
        &mut self,
        out: &mut [u8],
    ) -> Result<usize, WifiError> {
        self.hardware
            .read_controller_transport(Cyw43439Radio::Wifi, out)
            .map_err(map_wifi_error)
    }

    pub(crate) fn firmware_image_bluetooth(
        &self,
    ) -> Result<Option<&'static [u8]>, BluetoothError> {
        self.hardware
            .firmware_image(Cyw43439Radio::Bluetooth)
            .map_err(map_bluetooth_error)
    }

    pub(crate) fn firmware_image_wifi(&self) -> Result<Option<&'static [u8]>, WifiError> {
        self.hardware
            .firmware_image(Cyw43439Radio::Wifi)
            .map_err(map_wifi_error)
    }

    pub(crate) fn nvram_image_bluetooth(
        &self,
    ) -> Result<Option<&'static [u8]>, BluetoothError> {
        self.hardware
            .nvram_image(Cyw43439Radio::Bluetooth)
            .map_err(map_bluetooth_error)
    }

    pub(crate) fn nvram_image_wifi(&self) -> Result<Option<&'static [u8]>, WifiError> {
        self.hardware
            .nvram_image(Cyw43439Radio::Wifi)
            .map_err(map_wifi_error)
    }

    pub(crate) fn delay_ms(&self, milliseconds: u32) {
        self.hardware.delay_ms(milliseconds);
    }
}

/// Registration-owned activation context for one CYW43439 family instance.
#[derive(Debug)]
pub struct Cyw43439DriverContext<H: Cyw43439HardwareContract = UnsupportedBackend> {
    chipset: Option<Cyw43439Chipset<H>>,
}

impl<H> Cyw43439DriverContext<H>
where
    H: Cyw43439HardwareContract,
{
    /// Creates one activation context over one concrete CYW43439 hardware substrate.
    #[allow(dead_code)]
    #[must_use]
    pub fn new(hardware: H) -> Self {
        Self {
            chipset: Some(Cyw43439Chipset::new(hardware)),
        }
    }

    pub(crate) fn chipset(&self) -> Option<&Cyw43439Chipset<H>> {
        self.chipset.as_ref()
    }

    pub(crate) fn take_chipset(&mut self) -> Option<Cyw43439Chipset<H>> {
        self.chipset.take()
    }

    pub(crate) fn replace_chipset(&mut self, chipset: Cyw43439Chipset<H>) {
        self.chipset = Some(chipset);
    }
}

pub(crate) fn map_bluetooth_error(error: Cyw43439Error) -> BluetoothError {
    match error.kind() {
        Cyw43439ErrorKind::Unsupported => BluetoothError::unsupported(),
        Cyw43439ErrorKind::Invalid => BluetoothError::invalid(),
        Cyw43439ErrorKind::Busy => BluetoothError::busy(),
        Cyw43439ErrorKind::ResourceExhausted => BluetoothError::resource_exhausted(),
        Cyw43439ErrorKind::StateConflict => BluetoothError::state_conflict(),
        Cyw43439ErrorKind::Platform(code) => BluetoothError::platform(code),
    }
}

pub(crate) fn map_wifi_error(error: Cyw43439Error) -> WifiError {
    match error.kind() {
        Cyw43439ErrorKind::Unsupported => WifiError::unsupported(),
        Cyw43439ErrorKind::Invalid => WifiError::invalid(),
        Cyw43439ErrorKind::Busy => WifiError::busy(),
        Cyw43439ErrorKind::ResourceExhausted => WifiError::resource_exhausted(),
        Cyw43439ErrorKind::StateConflict => WifiError::state_conflict(),
        Cyw43439ErrorKind::Platform(code) => WifiError::platform(code),
    }
}
