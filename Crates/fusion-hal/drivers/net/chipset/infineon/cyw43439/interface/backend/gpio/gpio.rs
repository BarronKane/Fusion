//! GPIO-composed CYW43439 backend.

use crate::contract::drivers::net::bluetooth::{
    BluetoothAdapterDescriptor,
    BluetoothAdapterId,
    BluetoothAdapterSupport,
    BluetoothImplementationKind,
    BluetoothProviderCaps,
    BluetoothSupport,
    BluetoothVersion,
    BluetoothVersionRange,
};
use crate::contract::drivers::net::wifi::{
    WifiAccessPointCaps,
    WifiAdapterDescriptor,
    WifiAdapterId,
    WifiAdapterSupport,
    WifiBandCaps,
    WifiChannelDescriptor,
    WifiChannelWidthCaps,
    WifiDataCaps,
    WifiImplementationKind,
    WifiMeshCaps,
    WifiMloCaps,
    WifiMonitorCaps,
    WifiP2pCaps,
    WifiProviderCaps,
    WifiRoleCaps,
    WifiScanCaps,
    WifiSecurityCaps,
    WifiStandardFamilyCaps,
    WifiStationCaps,
    WifiSupport,
};
use crate::drivers::bus::gpio::{
    GpioFunction,
    GpioPin,
};
use crate::drivers::bus::gpio::interface::contract::GpioHardwarePin;
use crate::drivers::net::chipset::infineon::cyw43439::firmware::Cyw43439FirmwareAssets;
use crate::drivers::net::chipset::infineon::cyw43439::bluetooth::CYW43439_BLUETOOTH_VENDOR_IDENTITY;
use crate::drivers::net::chipset::infineon::cyw43439::interface::contract::{
    Cyw43439ControllerCaps,
    Cyw43439Error,
    Cyw43439HardwareContract,
    Cyw43439Radio,
};
use crate::drivers::net::chipset::infineon::cyw43439::transport::{
    Cyw43439BluetoothTransport,
    Cyw43439BluetoothTransportClockProfile,
    Cyw43439TransportTopology,
    Cyw43439WlanTransport,
    Cyw43439WlanTransportClockProfile,
};
use crate::drivers::net::chipset::infineon::cyw43439::wifi::CYW43439_WIFI_VENDOR_IDENTITY;

const CYW43439_BLUETOOTH_ADAPTER_ID: BluetoothAdapterId = BluetoothAdapterId(0);
const CYW43439_WIFI_ADAPTER_ID: WifiAdapterId = WifiAdapterId(0);
const CYW43439_WIFI_CHANNELS: [WifiChannelDescriptor; 0] = [];
const CYW43439_STANDARD_FAMILIES: WifiStandardFamilyCaps = WifiStandardFamilyCaps::from_bits_retain(
    WifiStandardFamilyCaps::LEGACY.bits() | WifiStandardFamilyCaps::HT.bits(),
);

const CYW43439_BLUETOOTH_ADAPTERS: [BluetoothAdapterDescriptor; 1] = [BluetoothAdapterDescriptor {
    id: CYW43439_BLUETOOTH_ADAPTER_ID,
    name: "CYW43439",
    vendor_identity: Some(CYW43439_BLUETOOTH_VENDOR_IDENTITY),
    shared_chipset: true,
    address: None,
    version: BluetoothVersionRange {
        minimum: BluetoothVersion::new(5, 2),
        maximum: BluetoothVersion::new(5, 2),
    },
    support: BluetoothAdapterSupport {
        transports: crate::contract::drivers::net::bluetooth::BluetoothTransportCaps::empty(),
        roles: crate::contract::drivers::net::bluetooth::BluetoothRoleCaps::empty(),
        le_phys: crate::contract::drivers::net::bluetooth::BluetoothLePhyCaps::empty(),
        advertising: crate::contract::drivers::net::bluetooth::BluetoothAdvertisingCaps::empty(),
        scanning: crate::contract::drivers::net::bluetooth::BluetoothScanningCaps::empty(),
        connection: crate::contract::drivers::net::bluetooth::BluetoothConnectionCaps::empty(),
        security: crate::contract::drivers::net::bluetooth::BluetoothSecurityCaps::empty(),
        l2cap: crate::contract::drivers::net::bluetooth::BluetoothL2capCaps::empty(),
        att: crate::contract::drivers::net::bluetooth::BluetoothAttCaps::empty(),
        gatt: crate::contract::drivers::net::bluetooth::BluetoothGattCaps::empty(),
        iso: crate::contract::drivers::net::bluetooth::BluetoothIsoCaps::empty(),
        max_connections: 0,
        max_advertising_sets: 0,
        max_periodic_advertising_sets: 0,
        max_att_mtu: 0,
        max_attribute_value_len: 0,
        max_l2cap_channels: 0,
        max_l2cap_sdu_len: 0,
    },
}];

const CYW43439_WIFI_ADAPTERS: [WifiAdapterDescriptor; 1] = [WifiAdapterDescriptor {
    id: CYW43439_WIFI_ADAPTER_ID,
    name: "CYW43439",
    vendor_identity: Some(CYW43439_WIFI_VENDOR_IDENTITY),
    shared_chipset: true,
    mac_address: None,
    regulatory_domain: None,
    channels: &CYW43439_WIFI_CHANNELS,
    support: WifiAdapterSupport {
        standards: CYW43439_STANDARD_FAMILIES,
        roles: WifiRoleCaps::empty(),
        bands: WifiBandCaps::GHZ_2_4,
        channel_widths: WifiChannelWidthCaps::WIDTH_20_MHZ,
        security: WifiSecurityCaps::empty(),
        scan: WifiScanCaps::empty(),
        station: WifiStationCaps::empty(),
        access_point: WifiAccessPointCaps::empty(),
        data: WifiDataCaps::empty(),
        monitor: WifiMonitorCaps::empty(),
        p2p: WifiP2pCaps::empty(),
        mesh: WifiMeshCaps::empty(),
        mlo: WifiMloCaps::empty(),
        max_scan_results: 0,
        max_links: 0,
        max_access_points: 0,
        max_associated_clients: 0,
        max_mesh_peers: 0,
        max_tx_queues: 0,
        max_spatial_streams: 0,
    },
}];

/// CYW43439 backend composed over owned GPIO pins.
#[derive(Debug)]
pub struct GpioBackend<
    ClockPin: GpioHardwarePin,
    ChipSelectPin: GpioHardwarePin,
    DataIrqPin: GpioHardwarePin,
    PowerPin: GpioHardwarePin,
    ResetPin: GpioHardwarePin,
    WakePin: GpioHardwarePin,
> {
    clock: GpioPin<ClockPin>,
    chip_select: GpioPin<ChipSelectPin>,
    data_irq: GpioPin<DataIrqPin>,
    power: Option<GpioPin<PowerPin>>,
    reset: Option<GpioPin<ResetPin>>,
    wake: Option<GpioPin<WakePin>>,
    bluetooth_transport: Cyw43439BluetoothTransport,
    bluetooth_target_rate: Option<u32>,
    wifi_transport: Cyw43439WlanTransport,
    wifi_target_clock_hz: Option<u32>,
    transport_topology: Cyw43439TransportTopology,
    host_source_clock_hz: Option<fn() -> Option<u64>>,
    reference_clock_hz: Option<u32>,
    sleep_clock_hz: Option<u32>,
    delay: fn(u32),
    firmware: Cyw43439FirmwareAssets,
    bluetooth_available: bool,
    wifi_available: bool,
    bluetooth_claimed: bool,
    wifi_claimed: bool,
    bluetooth_enabled: bool,
    wifi_enabled: bool,
    shared_transport_owner: Option<Cyw43439Radio>,
    bluetooth_transport_acquired: bool,
    wifi_transport_acquired: bool,
    powered: bool,
    power_configured: bool,
    reset_configured: bool,
    wake_configured: bool,
}

impl<
    ClockPin: GpioHardwarePin,
    ChipSelectPin: GpioHardwarePin,
    DataIrqPin: GpioHardwarePin,
    PowerPin: GpioHardwarePin,
    ResetPin: GpioHardwarePin,
    WakePin: GpioHardwarePin,
> GpioBackend<ClockPin, ChipSelectPin, DataIrqPin, PowerPin, ResetPin, WakePin>
{
    /// Creates one GPIO-composed CYW43439 combo-chip backend.
    #[must_use]
    pub fn new(
        clock: GpioPin<ClockPin>,
        chip_select: GpioPin<ChipSelectPin>,
        data_irq: GpioPin<DataIrqPin>,
        power: Option<GpioPin<PowerPin>>,
        reset: Option<GpioPin<ResetPin>>,
        wake: Option<GpioPin<WakePin>>,
        bluetooth_transport: Cyw43439BluetoothTransport,
        bluetooth_target_rate: Option<u32>,
        wifi_transport: Cyw43439WlanTransport,
        wifi_target_clock_hz: Option<u32>,
        transport_topology: Cyw43439TransportTopology,
        host_source_clock_hz: Option<fn() -> Option<u64>>,
        reference_clock_hz: Option<u32>,
        sleep_clock_hz: Option<u32>,
        delay: fn(u32),
        firmware: Cyw43439FirmwareAssets,
        bluetooth_available: bool,
        wifi_available: bool,
    ) -> Self {
        Self {
            clock,
            chip_select,
            data_irq,
            power,
            reset,
            wake,
            bluetooth_transport,
            bluetooth_target_rate,
            wifi_transport,
            wifi_target_clock_hz,
            transport_topology,
            host_source_clock_hz,
            reference_clock_hz,
            sleep_clock_hz,
            delay,
            firmware,
            bluetooth_available,
            wifi_available,
            bluetooth_claimed: false,
            wifi_claimed: false,
            bluetooth_enabled: false,
            wifi_enabled: false,
            shared_transport_owner: None,
            bluetooth_transport_acquired: false,
            wifi_transport_acquired: false,
            powered: false,
            power_configured: false,
            reset_configured: false,
            wake_configured: false,
        }
    }

    fn radio_available(&self, radio: Cyw43439Radio) -> bool {
        match radio {
            Cyw43439Radio::Bluetooth => self.bluetooth_available,
            Cyw43439Radio::Wifi => self.wifi_available,
        }
    }

    fn configure_output_pin<P: GpioHardwarePin>(
        pin: &mut GpioPin<P>,
        initial_high: bool,
        already_configured: &mut bool,
    ) -> Result<(), Cyw43439Error> {
        pin.set_function(GpioFunction::Sio)
            .map_err(map_gpio_error)?;

        if *already_configured {
            pin.set_level(initial_high).map_err(map_gpio_error)
        } else {
            pin.configure_output(initial_high).map_err(map_gpio_error)?;
            *already_configured = true;
            Ok(())
        }
    }

    fn claim_flag_mut(&mut self, radio: Cyw43439Radio) -> &mut bool {
        match radio {
            Cyw43439Radio::Bluetooth => &mut self.bluetooth_claimed,
            Cyw43439Radio::Wifi => &mut self.wifi_claimed,
        }
    }

    fn claim_flag(&self, radio: Cyw43439Radio) -> bool {
        match radio {
            Cyw43439Radio::Bluetooth => self.bluetooth_claimed,
            Cyw43439Radio::Wifi => self.wifi_claimed,
        }
    }

    fn enabled_flag_mut(&mut self, radio: Cyw43439Radio) -> &mut bool {
        match radio {
            Cyw43439Radio::Bluetooth => &mut self.bluetooth_enabled,
            Cyw43439Radio::Wifi => &mut self.wifi_enabled,
        }
    }

    fn enabled_flag(&self, radio: Cyw43439Radio) -> bool {
        match radio {
            Cyw43439Radio::Bluetooth => self.bluetooth_enabled,
            Cyw43439Radio::Wifi => self.wifi_enabled,
        }
    }

    fn any_enabled(&self) -> bool {
        self.bluetooth_enabled || self.wifi_enabled
    }

    fn transport_acquired_flag_mut(&mut self, radio: Cyw43439Radio) -> &mut bool {
        match radio {
            Cyw43439Radio::Bluetooth => &mut self.bluetooth_transport_acquired,
            Cyw43439Radio::Wifi => &mut self.wifi_transport_acquired,
        }
    }

    fn transport_acquired_flag(&self, radio: Cyw43439Radio) -> bool {
        match radio {
            Cyw43439Radio::Bluetooth => self.bluetooth_transport_acquired,
            Cyw43439Radio::Wifi => self.wifi_transport_acquired,
        }
    }

    fn transport_held(&self, radio: Cyw43439Radio) -> bool {
        match self.transport_topology {
            Cyw43439TransportTopology::SharedBoardTransport => {
                self.shared_transport_owner == Some(radio)
            }
            Cyw43439TransportTopology::SplitHostTransports => self.transport_acquired_flag(radio),
        }
    }

    #[must_use]
    pub fn bluetooth_transport_target_rate(&self) -> Option<u32> {
        self.bluetooth_target_rate
    }

    #[must_use]
    pub fn wifi_transport_target_clock_hz(&self) -> Option<u32> {
        self.wifi_target_clock_hz
    }

    fn controller_caps_inner(&self) -> Cyw43439ControllerCaps {
        let mut caps = Cyw43439ControllerCaps::CLAIM_CONTROLLER;

        if self.power.is_some() {
            caps |= Cyw43439ControllerCaps::POWER_CONTROL;
        }
        if self.reset.is_some() {
            caps |= Cyw43439ControllerCaps::RESET_CONTROL;
        }
        if self.wake.is_some() {
            caps |= Cyw43439ControllerCaps::WAKE_CONTROL;
        }
        caps |= Cyw43439ControllerCaps::TIMING_DELAY;
        caps
    }
}

impl<
    ClockPin: GpioHardwarePin,
    ChipSelectPin: GpioHardwarePin,
    DataIrqPin: GpioHardwarePin,
    PowerPin: GpioHardwarePin,
    ResetPin: GpioHardwarePin,
    WakePin: GpioHardwarePin,
> Cyw43439HardwareContract
    for GpioBackend<ClockPin, ChipSelectPin, DataIrqPin, PowerPin, ResetPin, WakePin>
{
    fn bluetooth_support(&self) -> BluetoothSupport {
        if !self.bluetooth_available {
            return BluetoothSupport::unsupported();
        }

        let mut caps = BluetoothProviderCaps::ENUMERATE_ADAPTERS
            | BluetoothProviderCaps::OPEN_ADAPTER
            | BluetoothProviderCaps::STATIC_TOPOLOGY;
        if self.power.is_some() {
            caps |= BluetoothProviderCaps::POWER_CONTROL;
        }

        BluetoothSupport {
            caps,
            implementation: BluetoothImplementationKind::Native,
            adapter_count: 1,
        }
    }

    fn bluetooth_adapters(&self) -> &'static [BluetoothAdapterDescriptor] {
        if self.bluetooth_available {
            &CYW43439_BLUETOOTH_ADAPTERS
        } else {
            &[]
        }
    }

    fn bluetooth_transport(&self) -> Result<Cyw43439BluetoothTransport, Cyw43439Error> {
        if !self.bluetooth_available {
            return Err(Cyw43439Error::unsupported());
        }

        Ok(self.bluetooth_transport)
    }

    fn bluetooth_transport_clock_profile(
        &self,
    ) -> Result<Cyw43439BluetoothTransportClockProfile, Cyw43439Error> {
        if !self.bluetooth_available {
            return Err(Cyw43439Error::unsupported());
        }

        let host_source_clock_hz = self.host_source_clock_hz.and_then(|f| f());
        Ok(match self.bluetooth_transport {
            Cyw43439BluetoothTransport::HciUartH4 | Cyw43439BluetoothTransport::HciUartH5 => {
                Cyw43439BluetoothTransportClockProfile::HciUart {
                    target_baud: self.bluetooth_target_rate,
                    host_source_clock_hz,
                }
            }
            Cyw43439BluetoothTransport::BoardSharedSpiHci => {
                Cyw43439BluetoothTransportClockProfile::BoardSharedSpiHci {
                    target_clock_hz: self.bluetooth_target_rate,
                    host_source_clock_hz,
                }
            }
        })
    }

    fn wifi_support(&self) -> WifiSupport {
        if !self.wifi_available {
            return WifiSupport::unsupported();
        }

        let mut caps = WifiProviderCaps::ENUMERATE_ADAPTERS
            | WifiProviderCaps::OPEN_ADAPTER
            | WifiProviderCaps::STATIC_TOPOLOGY
            | WifiProviderCaps::RADIO_CONTROL;
        if self.power.is_some() {
            caps |= WifiProviderCaps::POWER_CONTROL;
        }

        WifiSupport {
            caps,
            implementation: WifiImplementationKind::Native,
            adapter_count: 1,
        }
    }

    fn wifi_adapters(&self) -> &'static [WifiAdapterDescriptor] {
        if self.wifi_available {
            &CYW43439_WIFI_ADAPTERS
        } else {
            &[]
        }
    }

    fn wifi_transport(&self) -> Result<Cyw43439WlanTransport, Cyw43439Error> {
        if !self.wifi_available {
            return Err(Cyw43439Error::unsupported());
        }

        Ok(self.wifi_transport)
    }

    fn wifi_transport_clock_profile(
        &self,
    ) -> Result<Cyw43439WlanTransportClockProfile, Cyw43439Error> {
        if !self.wifi_available {
            return Err(Cyw43439Error::unsupported());
        }

        let host_source_clock_hz = self.host_source_clock_hz.and_then(|f| f());
        Ok(match self.wifi_transport {
            Cyw43439WlanTransport::Gspi => Cyw43439WlanTransportClockProfile::Gspi {
                target_clock_hz: self.wifi_target_clock_hz,
                host_source_clock_hz,
            },
            Cyw43439WlanTransport::Sdio => Cyw43439WlanTransportClockProfile::Sdio {
                target_clock_hz: self.wifi_target_clock_hz,
                host_source_clock_hz,
            },
            Cyw43439WlanTransport::BoardSharedSpi => {
                Cyw43439WlanTransportClockProfile::BoardSharedSpi {
                    target_clock_hz: self.wifi_target_clock_hz,
                    host_source_clock_hz,
                }
            }
        })
    }

    fn transport_topology(&self) -> Result<Cyw43439TransportTopology, Cyw43439Error> {
        if !self.bluetooth_available && !self.wifi_available {
            return Err(Cyw43439Error::unsupported());
        }

        Ok(self.transport_topology)
    }

    fn controller_caps(&self, radio: Cyw43439Radio) -> Cyw43439ControllerCaps {
        if self.radio_available(radio) {
            self.controller_caps_inner()
        } else {
            Cyw43439ControllerCaps::empty()
        }
    }

    fn claim_controller(&mut self, radio: Cyw43439Radio) -> Result<(), Cyw43439Error> {
        if !self.radio_available(radio) {
            return Err(Cyw43439Error::unsupported());
        }

        let _ = self.clock.pin();
        let _ = self.chip_select.pin();
        let _ = self.data_irq.pin();
        let claimed = self.claim_flag_mut(radio);
        if *claimed {
            return Err(Cyw43439Error::state_conflict());
        }
        *claimed = true;
        Ok(())
    }

    fn release_controller(&mut self, radio: Cyw43439Radio) {
        if !self.radio_available(radio) {
            return;
        }

        self.release_transport(radio);
        let _ = self.set_facet_enabled(radio, false);
        *self.claim_flag_mut(radio) = false;
    }

    fn facet_enabled(&self, radio: Cyw43439Radio) -> Result<bool, Cyw43439Error> {
        if !self.radio_available(radio) {
            return Err(Cyw43439Error::unsupported());
        }

        Ok(self.enabled_flag(radio))
    }

    fn set_facet_enabled(
        &mut self,
        radio: Cyw43439Radio,
        enabled: bool,
    ) -> Result<(), Cyw43439Error> {
        if !self.radio_available(radio) {
            return Err(Cyw43439Error::unsupported());
        }

        if !self.claim_flag(radio) {
            return Err(Cyw43439Error::state_conflict());
        }

        if self.enabled_flag(radio) == enabled {
            return Ok(());
        }

        if enabled {
            if !self.powered {
                self.set_controller_powered(true)?;
            }
            *self.enabled_flag_mut(radio) = true;
            return Ok(());
        }

        *self.enabled_flag_mut(radio) = false;
        if !self.any_enabled() && self.powered {
            self.set_controller_powered(false)?;
        }
        Ok(())
    }

    fn controller_powered(&self) -> Result<bool, Cyw43439Error> {
        if self.power.is_none() {
            return Err(Cyw43439Error::unsupported());
        }
        Ok(self.powered)
    }

    fn set_controller_powered(&mut self, powered: bool) -> Result<(), Cyw43439Error> {
        let power = self.power.as_mut().ok_or_else(Cyw43439Error::unsupported)?;
        Self::configure_output_pin(power, powered, &mut self.power_configured)?;
        self.powered = powered;
        Ok(())
    }

    fn set_controller_reset(&mut self, asserted: bool) -> Result<(), Cyw43439Error> {
        let reset = self.reset.as_mut().ok_or_else(Cyw43439Error::unsupported)?;
        Self::configure_output_pin(reset, asserted, &mut self.reset_configured)
    }

    fn set_controller_wake(&mut self, awake: bool) -> Result<(), Cyw43439Error> {
        let wake = self.wake.as_mut().ok_or_else(Cyw43439Error::unsupported)?;
        Self::configure_output_pin(wake, awake, &mut self.wake_configured)
    }

    fn acquire_transport(&mut self, radio: Cyw43439Radio) -> Result<(), Cyw43439Error> {
        if !self.radio_available(radio) {
            return Err(Cyw43439Error::unsupported());
        }
        if !self.claim_flag(radio) {
            return Err(Cyw43439Error::state_conflict());
        }

        match self.transport_topology {
            Cyw43439TransportTopology::SharedBoardTransport => match self.shared_transport_owner {
                Some(owner) if owner != radio => Err(Cyw43439Error::busy()),
                _ => {
                    self.shared_transport_owner = Some(radio);
                    Ok(())
                }
            },
            Cyw43439TransportTopology::SplitHostTransports => {
                *self.transport_acquired_flag_mut(radio) = true;
                Ok(())
            }
        }
    }

    fn release_transport(&mut self, radio: Cyw43439Radio) {
        match self.transport_topology {
            Cyw43439TransportTopology::SharedBoardTransport => {
                if self.shared_transport_owner == Some(radio) {
                    self.shared_transport_owner = None;
                }
            }
            Cyw43439TransportTopology::SplitHostTransports => {
                *self.transport_acquired_flag_mut(radio) = false;
            }
        }
    }

    fn wait_for_controller_irq(
        &mut self,
        radio: Cyw43439Radio,
        _timeout_ms: Option<u32>,
    ) -> Result<bool, Cyw43439Error> {
        if !self.radio_available(radio) {
            return Err(Cyw43439Error::unsupported());
        }
        if !self.transport_held(radio) {
            return Err(Cyw43439Error::state_conflict());
        }
        Err(Cyw43439Error::unsupported())
    }

    fn acknowledge_controller_irq(&mut self, radio: Cyw43439Radio) -> Result<(), Cyw43439Error> {
        if !self.radio_available(radio) {
            return Err(Cyw43439Error::unsupported());
        }
        if !self.transport_held(radio) {
            return Err(Cyw43439Error::state_conflict());
        }
        Err(Cyw43439Error::unsupported())
    }

    fn write_controller_transport(
        &mut self,
        radio: Cyw43439Radio,
        _payload: &[u8],
    ) -> Result<(), Cyw43439Error> {
        if !self.radio_available(radio) {
            return Err(Cyw43439Error::unsupported());
        }
        if !self.transport_held(radio) {
            return Err(Cyw43439Error::state_conflict());
        }
        Err(Cyw43439Error::unsupported())
    }

    fn read_controller_transport(
        &mut self,
        radio: Cyw43439Radio,
        _out: &mut [u8],
    ) -> Result<usize, Cyw43439Error> {
        if !self.radio_available(radio) {
            return Err(Cyw43439Error::unsupported());
        }
        if !self.transport_held(radio) {
            return Err(Cyw43439Error::state_conflict());
        }
        Err(Cyw43439Error::unsupported())
    }

    fn firmware_image(&self, radio: Cyw43439Radio) -> Result<Option<&'static [u8]>, Cyw43439Error> {
        if !self.radio_available(radio) {
            return Err(Cyw43439Error::unsupported());
        }
        Ok(match radio {
            Cyw43439Radio::Bluetooth => self.firmware.bluetooth.patch_image,
            Cyw43439Radio::Wifi => self.firmware.wifi.firmware_image,
        })
    }

    fn nvram_image(&self, radio: Cyw43439Radio) -> Result<Option<&'static [u8]>, Cyw43439Error> {
        if !self.radio_available(radio) {
            return Err(Cyw43439Error::unsupported());
        }
        Ok(match radio {
            Cyw43439Radio::Bluetooth => None,
            Cyw43439Radio::Wifi => self.firmware.wifi.nvram_image,
        })
    }

    fn clm_image(&self, radio: Cyw43439Radio) -> Result<Option<&'static [u8]>, Cyw43439Error> {
        if !self.radio_available(radio) {
            return Err(Cyw43439Error::unsupported());
        }
        Ok(match radio {
            Cyw43439Radio::Bluetooth => None,
            Cyw43439Radio::Wifi => self.firmware.wifi.clm_image,
        })
    }

    fn reference_clock_hz(&self) -> Result<Option<u32>, Cyw43439Error> {
        if !self.bluetooth_available && !self.wifi_available {
            return Err(Cyw43439Error::unsupported());
        }
        Ok(self.reference_clock_hz)
    }

    fn sleep_clock_hz(&self) -> Result<Option<u32>, Cyw43439Error> {
        if !self.bluetooth_available && !self.wifi_available {
            return Err(Cyw43439Error::unsupported());
        }
        Ok(self.sleep_clock_hz)
    }

    fn delay_ms(&self, milliseconds: u32) {
        (self.delay)(milliseconds);
    }
}

fn map_gpio_error(error: crate::contract::drivers::bus::gpio::GpioError) -> Cyw43439Error {
    match error.kind() {
        crate::contract::drivers::bus::gpio::GpioErrorKind::Unsupported => {
            Cyw43439Error::unsupported()
        }
        crate::contract::drivers::bus::gpio::GpioErrorKind::Invalid => Cyw43439Error::invalid(),
        crate::contract::drivers::bus::gpio::GpioErrorKind::Busy => Cyw43439Error::busy(),
        crate::contract::drivers::bus::gpio::GpioErrorKind::ResourceExhausted => {
            Cyw43439Error::resource_exhausted()
        }
        crate::contract::drivers::bus::gpio::GpioErrorKind::StateConflict => {
            Cyw43439Error::state_conflict()
        }
        crate::contract::drivers::bus::gpio::GpioErrorKind::Platform(code) => {
            Cyw43439Error::platform(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Cyw43439HardwareContract,
        Cyw43439Radio,
        GpioBackend,
    };
    use crate::contract::drivers::bus::gpio::{
        GpioCapabilities,
        GpioDriveStrength,
        GpioError,
        GpioFunction,
        GpioPull,
    };
    use crate::drivers::bus::gpio::GpioPin;
    use crate::drivers::bus::gpio::interface::contract::GpioHardwarePin;
    use crate::drivers::net::chipset::infineon::cyw43439::firmware::Cyw43439FirmwareAssets;
    use crate::drivers::net::chipset::infineon::cyw43439::interface::contract::Cyw43439ErrorKind;
    use crate::drivers::net::chipset::infineon::cyw43439::transport::{
        Cyw43439BluetoothTransport,
        Cyw43439TransportTopology,
        Cyw43439WlanTransport,
    };

    type Cyw43439GpioBackend = GpioBackend<FakePin, FakePin, FakePin, FakePin, FakePin, FakePin>;

    #[derive(Debug)]
    struct FakePin {
        pin: u8,
        level: bool,
        function: GpioFunction,
    }

    impl FakePin {
        const fn new(pin: u8) -> Self {
            Self {
                pin,
                level: false,
                function: GpioFunction::Sio,
            }
        }
    }

    impl GpioHardwarePin for FakePin {
        fn pin(&self) -> u8 {
            self.pin
        }

        fn capabilities(&self) -> GpioCapabilities {
            GpioCapabilities::INPUT.union(GpioCapabilities::OUTPUT)
        }

        fn set_function(&mut self, function: GpioFunction) -> Result<(), GpioError> {
            self.function = function;
            Ok(())
        }

        fn configure_input(&mut self) -> Result<(), GpioError> {
            Ok(())
        }

        fn read_level(&self) -> Result<bool, GpioError> {
            Ok(self.level)
        }

        fn configure_output(&mut self, initial_high: bool) -> Result<(), GpioError> {
            self.level = initial_high;
            Ok(())
        }

        fn set_level(&mut self, high: bool) -> Result<(), GpioError> {
            self.level = high;
            Ok(())
        }

        fn set_pull(&mut self, _pull: GpioPull) -> Result<(), GpioError> {
            Ok(())
        }

        fn set_drive_strength(&mut self, _strength: GpioDriveStrength) -> Result<(), GpioError> {
            Ok(())
        }
    }

    fn fake_backend() -> Cyw43439GpioBackend {
        GpioBackend::new(
            GpioPin::from_inner(FakePin::new(29)),
            GpioPin::from_inner(FakePin::new(25)),
            GpioPin::from_inner(FakePin::new(24)),
            Some(GpioPin::from_inner(FakePin::new(23))),
            None,
            None,
            Cyw43439BluetoothTransport::BoardSharedSpiHci,
            Some(31_250_000),
            Cyw43439WlanTransport::BoardSharedSpi,
            Some(31_250_000),
            Cyw43439TransportTopology::SharedBoardTransport,
            Some(|| Some(150_000_000)),
            Some(37_400_000),
            None,
            |_| {},
            Cyw43439FirmwareAssets::default(),
            true,
            true,
        )
    }

    #[test]
    fn facet_power_tracks_logical_enable_without_last_write_wins() {
        let mut backend = fake_backend();

        backend.claim_controller(Cyw43439Radio::Bluetooth).unwrap();
        backend.claim_controller(Cyw43439Radio::Wifi).unwrap();
        backend
            .set_facet_enabled(Cyw43439Radio::Bluetooth, true)
            .unwrap();
        backend
            .set_facet_enabled(Cyw43439Radio::Wifi, true)
            .unwrap();

        assert!(backend.controller_powered().unwrap());
        assert!(backend.facet_enabled(Cyw43439Radio::Bluetooth).unwrap());
        assert!(backend.facet_enabled(Cyw43439Radio::Wifi).unwrap());

        backend
            .set_facet_enabled(Cyw43439Radio::Bluetooth, false)
            .unwrap();

        assert!(backend.controller_powered().unwrap());
        assert!(!backend.facet_enabled(Cyw43439Radio::Bluetooth).unwrap());
        assert!(backend.facet_enabled(Cyw43439Radio::Wifi).unwrap());

        backend
            .set_facet_enabled(Cyw43439Radio::Wifi, false)
            .unwrap();

        assert!(!backend.controller_powered().unwrap());
        assert!(!backend.facet_enabled(Cyw43439Radio::Wifi).unwrap());
    }

    #[test]
    fn shared_transport_is_exclusive_across_facets() {
        let mut backend = fake_backend();

        backend.claim_controller(Cyw43439Radio::Bluetooth).unwrap();
        backend.claim_controller(Cyw43439Radio::Wifi).unwrap();

        backend.acquire_transport(Cyw43439Radio::Bluetooth).unwrap();

        let error = backend.acquire_transport(Cyw43439Radio::Wifi).unwrap_err();
        assert_eq!(error.kind(), Cyw43439ErrorKind::Busy);

        backend.release_transport(Cyw43439Radio::Bluetooth);
        backend.acquire_transport(Cyw43439Radio::Wifi).unwrap();
    }

    #[test]
    fn split_topology_allows_independent_transport_leases() {
        let mut backend = GpioBackend::new(
            GpioPin::from_inner(FakePin::new(29)),
            GpioPin::from_inner(FakePin::new(25)),
            GpioPin::from_inner(FakePin::new(24)),
            Some(GpioPin::from_inner(FakePin::new(23))),
            None::<GpioPin<FakePin>>,
            None::<GpioPin<FakePin>>,
            Cyw43439BluetoothTransport::HciUartH4,
            Some(3_000_000),
            Cyw43439WlanTransport::Gspi,
            Some(31_250_000),
            Cyw43439TransportTopology::SplitHostTransports,
            Some(|| Some(150_000_000)),
            Some(37_400_000),
            None,
            |_| {},
            Cyw43439FirmwareAssets::default(),
            true,
            true,
        );

        backend.claim_controller(Cyw43439Radio::Bluetooth).unwrap();
        backend.claim_controller(Cyw43439Radio::Wifi).unwrap();
        backend.acquire_transport(Cyw43439Radio::Bluetooth).unwrap();
        backend.acquire_transport(Cyw43439Radio::Wifi).unwrap();
    }
}
