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
use crate::drivers::net::chipset::infineon::cyw43439::bluetooth::CYW43439_BLUETOOTH_VENDOR_IDENTITY;
use crate::drivers::net::chipset::infineon::cyw43439::interface::contract::{
    Cyw43439ControllerCaps,
    Cyw43439Error,
    Cyw43439HardwareContract,
    Cyw43439Radio,
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

/// Optional firmware/config images surfaced for one CYW43439 combo chip.
#[derive(Debug, Clone, Copy, Default)]
pub struct Cyw43439Images {
    pub bluetooth_firmware: Option<&'static [u8]>,
    pub bluetooth_nvram: Option<&'static [u8]>,
    pub wifi_firmware: Option<&'static [u8]>,
    pub wifi_nvram: Option<&'static [u8]>,
}

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
    delay: fn(u32),
    images: Cyw43439Images,
    bluetooth_available: bool,
    wifi_available: bool,
    bluetooth_claimed: bool,
    wifi_claimed: bool,
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
        delay: fn(u32),
        images: Cyw43439Images,
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
            delay,
            images,
            bluetooth_available,
            wifi_available,
            bluetooth_claimed: false,
            wifi_claimed: false,
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
        if self.radio_available(radio) {
            *self.claim_flag_mut(radio) = false;
        }
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

    fn wait_for_controller_irq(
        &mut self,
        radio: Cyw43439Radio,
        _timeout_ms: Option<u32>,
    ) -> Result<bool, Cyw43439Error> {
        if !self.radio_available(radio) {
            return Err(Cyw43439Error::unsupported());
        }
        Err(Cyw43439Error::unsupported())
    }

    fn acknowledge_controller_irq(&mut self, radio: Cyw43439Radio) -> Result<(), Cyw43439Error> {
        if !self.radio_available(radio) {
            return Err(Cyw43439Error::unsupported());
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
        Err(Cyw43439Error::unsupported())
    }

    fn firmware_image(&self, radio: Cyw43439Radio) -> Result<Option<&'static [u8]>, Cyw43439Error> {
        if !self.radio_available(radio) {
            return Err(Cyw43439Error::unsupported());
        }
        Ok(match radio {
            Cyw43439Radio::Bluetooth => self.images.bluetooth_firmware,
            Cyw43439Radio::Wifi => self.images.wifi_firmware,
        })
    }

    fn nvram_image(&self, radio: Cyw43439Radio) -> Result<Option<&'static [u8]>, Cyw43439Error> {
        if !self.radio_available(radio) {
            return Err(Cyw43439Error::unsupported());
        }
        Ok(match radio {
            Cyw43439Radio::Bluetooth => self.images.bluetooth_nvram,
            Cyw43439Radio::Wifi => self.images.wifi_nvram,
        })
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
