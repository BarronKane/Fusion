//! CYW43439 backend implementation families.

use fusion_hal::contract::drivers::net::bluetooth::BluetoothSupport;
use fusion_hal::contract::drivers::net::wifi::WifiSupport;
use crate::interface::contract::{
    Cyw43439ControllerCaps,
    Cyw43439Error,
    Cyw43439HardwareContract,
    Cyw43439Radio,
};
use crate::transport::{
    Cyw43439BluetoothTransport,
    Cyw43439BluetoothTransportClockProfile,
    Cyw43439TransportTopology,
    Cyw43439WlanTransport,
    Cyw43439WlanTransportClockProfile,
};

#[path = "gpio/gpio.rs"]
pub mod gpio;

/// Unsupported CYW43439 hardware substrate.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedBackend;

impl Cyw43439HardwareContract for UnsupportedBackend {
    fn bluetooth_support(&self) -> BluetoothSupport {
        BluetoothSupport::unsupported()
    }

    fn bluetooth_adapters(
        &self,
    ) -> &'static [fusion_hal::contract::drivers::net::bluetooth::BluetoothAdapterDescriptor] {
        &[]
    }

    fn bluetooth_transport(&self) -> Result<Cyw43439BluetoothTransport, Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    fn bluetooth_transport_clock_profile(
        &self,
    ) -> Result<Cyw43439BluetoothTransportClockProfile, Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    fn wifi_support(&self) -> WifiSupport {
        WifiSupport::unsupported()
    }

    fn wifi_adapters(
        &self,
    ) -> &'static [fusion_hal::contract::drivers::net::wifi::WifiAdapterDescriptor] {
        &[]
    }

    fn wifi_transport(&self) -> Result<Cyw43439WlanTransport, Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    fn wifi_transport_clock_profile(
        &self,
    ) -> Result<Cyw43439WlanTransportClockProfile, Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    fn transport_topology(&self) -> Result<Cyw43439TransportTopology, Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    fn controller_caps(&self, _radio: Cyw43439Radio) -> Cyw43439ControllerCaps {
        Cyw43439ControllerCaps::empty()
    }

    fn claim_controller(&mut self, _radio: Cyw43439Radio) -> Result<(), Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    fn release_controller(&mut self, _radio: Cyw43439Radio) {}

    fn facet_enabled(&self, _radio: Cyw43439Radio) -> Result<bool, Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    fn set_facet_enabled(
        &mut self,
        _radio: Cyw43439Radio,
        _enabled: bool,
    ) -> Result<(), Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    fn controller_powered(&self) -> Result<bool, Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    fn set_controller_powered(&mut self, _powered: bool) -> Result<(), Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    fn set_controller_reset(&mut self, _asserted: bool) -> Result<(), Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    fn set_controller_wake(&mut self, _awake: bool) -> Result<(), Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    fn acquire_transport(&mut self, _radio: Cyw43439Radio) -> Result<(), Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    fn release_transport(&mut self, _radio: Cyw43439Radio) {}

    fn wait_for_controller_irq(
        &mut self,
        _radio: Cyw43439Radio,
        _timeout_ms: Option<u32>,
    ) -> Result<bool, Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    fn acknowledge_controller_irq(&mut self, _radio: Cyw43439Radio) -> Result<(), Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    fn write_controller_transport(
        &mut self,
        _radio: Cyw43439Radio,
        _payload: &[u8],
    ) -> Result<(), Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    fn read_controller_transport(
        &mut self,
        _radio: Cyw43439Radio,
        _out: &mut [u8],
    ) -> Result<usize, Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    fn firmware_image(
        &self,
        _radio: Cyw43439Radio,
    ) -> Result<Option<&'static [u8]>, Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    fn nvram_image(&self, _radio: Cyw43439Radio) -> Result<Option<&'static [u8]>, Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    fn clm_image(&self, _radio: Cyw43439Radio) -> Result<Option<&'static [u8]>, Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    fn reference_clock_hz(&self) -> Result<Option<u32>, Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    fn sleep_clock_hz(&self) -> Result<Option<u32>, Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    fn delay_ms(&self, _milliseconds: u32) {}
}
