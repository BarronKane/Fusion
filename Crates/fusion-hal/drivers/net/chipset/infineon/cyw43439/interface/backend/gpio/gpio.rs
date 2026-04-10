//! GPIO-composed CYW43439 backend.

use core::sync::atomic::{
    AtomicU32,
    Ordering,
};

use fusion_hal::contract::drivers::net::bluetooth::{
    BluetoothAdapterDescriptor,
    BluetoothAdapterId,
    BluetoothAdapterSupport,
    BluetoothImplementationKind,
    BluetoothProviderCaps,
    BluetoothSupport,
    BluetoothVersion,
    BluetoothVersionRange,
};
use fusion_hal::contract::drivers::net::wifi::{
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
use fusion_hal::contract::drivers::bus::gpio::{
    GpioCapabilities,
    GpioImplementationKind,
    GpioPinDescriptor,
    GpioProviderCaps,
    GpioSupport,
};
use fd_bus_gpio::{
    GpioDriveStrength,
    GpioFunction,
    GpioPin,
    GpioPull,
};
use fd_bus_gpio::interface::contract::GpioHardwarePin;
use crate::firmware::Cyw43439FirmwareAssets;
use crate::bluetooth::CYW43439_BLUETOOTH_VENDOR_IDENTITY;
use crate::interface::contract::{
    Cyw43439ControllerCaps,
    Cyw43439Error,
    Cyw43439HardwareContract,
    Cyw43439Radio,
};
use crate::transport::{
    bluetooth::{
        Cyw43439BluetoothSharedBufferIndex,
        Cyw43439BluetoothSharedBufferLayout,
        CYW43439_BTFW_MEM_OFFSET,
        CYW43439_BTFW_WAIT_TIME_MS,
        CYW43439_BT2WLAN_PWRUP_ADDR,
        CYW43439_BT2WLAN_PWRUP_WAKE,
        CYW43439_BTSDIO_FWBUF_SIZE,
        CYW43439_BTSDIO_FW_AWAKE_POLLING_INTERVAL_MS,
        CYW43439_BTSDIO_FW_AWAKE_POLLING_RETRY_COUNT,
        CYW43439_BTSDIO_FW_READY_POLLING_INTERVAL_MS,
        CYW43439_BTSDIO_FW_READY_POLLING_RETRY_COUNT,
        CYW43439_BTSDIO_REG_BT_AWAKE_BITMASK,
        CYW43439_BTSDIO_REG_DATA_VALID_BITMASK,
        CYW43439_BTSDIO_REG_FW_RDY_BITMASK,
        CYW43439_BTSDIO_REG_SW_RDY_BITMASK,
        CYW43439_BTSDIO_REG_WAKE_BT_BITMASK,
        CYW43439_BT_CTRL_REG_ADDR,
        CYW43439_BT_HOST_CTRL_REG_ADDR,
        CYW43439_BT_WLAN_RAM_BASE_REG_ADDR,
        bt_shared_round_up_4,
        for_each_patch_data_record,
    },
    wlan::{
        CYW43439_GSPI_BACKPLANE_ACCESS_2_4B_FLAG,
        CYW43439_GSPI_BACKPLANE_ADDR_MASK,
        CYW43439_GSPI_BACKPLANE_ADDRESS_HIGH,
        CYW43439_GSPI_BACKPLANE_ADDRESS_LOW,
        CYW43439_GSPI_BACKPLANE_ADDRESS_MID,
        CYW43439_GSPI_BACKPLANE_READ_PAD_LEN_BYTES,
        CYW43439_GSPI_CHIPCOMMON_BASE_ADDRESS,
        CYW43439_GSPI_CHIPCOMMON_GPIOIN_OFFSET,
        CYW43439_GSPI_CHIPCOMMON_GPIOCONTROL_OFFSET,
        CYW43439_GSPI_CHIPCOMMON_GPIOOUTEN_OFFSET,
        CYW43439_GSPI_CHIPCOMMON_GPIOOUT_OFFSET,
        Cyw43439GspiBusControlFlags,
        Cyw43439GspiCommand,
        Cyw43439GspiF0Register,
        Cyw43439GspiFunction,
    },
    Cyw43439BluetoothTransport,
    Cyw43439BluetoothTransportClockProfile,
    Cyw43439TransportTopology,
    Cyw43439WlanTransport,
    Cyw43439WlanTransportClockProfile,
};
use crate::wifi::CYW43439_WIFI_VENDOR_IDENTITY;

const CYW43439_BLUETOOTH_ADAPTER_ID: BluetoothAdapterId = BluetoothAdapterId(0);
const CYW43439_WIFI_ADAPTER_ID: WifiAdapterId = WifiAdapterId(0);
const CYW43439_WIFI_CHANNELS: [WifiChannelDescriptor; 0] = [];
const CYW43439_STANDARD_FAMILIES: WifiStandardFamilyCaps = WifiStandardFamilyCaps::from_bits_retain(
    WifiStandardFamilyCaps::LEGACY.bits() | WifiStandardFamilyCaps::HT.bits(),
);
const CYW43439_SHARED_SPI_HOST_WAKE_IRQ_HIGH: bool = true;
const CYW43439_WL_GPIO_COUNT: u16 = 3;
const CYW43439_WL_GPIO_IO_CAPS: GpioCapabilities =
    GpioCapabilities::INPUT.union(GpioCapabilities::OUTPUT);
const CYW43439_WL_GPIO_INPUT_ONLY_CAPS: GpioCapabilities = GpioCapabilities::INPUT;
const CYW43439_WL_GPIO_PINS: [GpioPinDescriptor; CYW43439_WL_GPIO_COUNT as usize] = [
    GpioPinDescriptor {
        pin: 0,
        name: "wl_gpio0",
        capabilities: CYW43439_WL_GPIO_IO_CAPS,
    },
    GpioPinDescriptor {
        pin: 1,
        name: "wl_gpio1",
        capabilities: CYW43439_WL_GPIO_IO_CAPS,
    },
    GpioPinDescriptor {
        pin: 2,
        name: "wl_gpio2",
        capabilities: CYW43439_WL_GPIO_INPUT_ONLY_CAPS,
    },
];
// GPIO-driver calls dominate the bit-bang path already, so a large extra
// half-cycle spin budget just turns firmware download into geology.
const CYW43439_SHARED_SPI_HALF_CYCLE_SPINS: usize = 8;
const CYW43439_BT_PATCH_STAGING_BYTES: usize = 264;

#[unsafe(no_mangle)]
pub static CYW43439_SHARED_BUS_LAST_READ_RAW: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_BLUETOOTH_PHASE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_BLUETOOTH_LAST_ERROR: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_BLUETOOTH_LAST_CTRL: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_BLUETOOTH_LAST_HOST_CTRL: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_BLUETOOTH_LAST_H2B_IN: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_BLUETOOTH_LAST_H2B_OUT: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_BLUETOOTH_LAST_B2H_IN: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_BLUETOOTH_LAST_B2H_OUT: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_BLUETOOTH_LAST_SPACE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_BLUETOOTH_LAST_WRITE_LEN: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_BLUETOOTH_LAST_RING_HEADER: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_BLUETOOTH_LAST_WRITE_WORDS: [AtomicU32; 8] = [const { AtomicU32::new(0) }; 8];
#[unsafe(no_mangle)]
pub static CYW43439_BLUETOOTH_LAST_RING_READBACK_WORDS: [AtomicU32; 8] =
    [const { AtomicU32::new(0) }; 8];

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
        transports: fusion_hal::contract::drivers::net::bluetooth::BluetoothTransportCaps::LE,
        roles: fusion_hal::contract::drivers::net::bluetooth::BluetoothRoleCaps::from_bits_retain(
            fusion_hal::contract::drivers::net::bluetooth::BluetoothRoleCaps::PERIPHERAL.bits()
                | fusion_hal::contract::drivers::net::bluetooth::BluetoothRoleCaps::BROADCASTER
                    .bits(),
        ),
        le_phys: fusion_hal::contract::drivers::net::bluetooth::BluetoothLePhyCaps::LE_1M,
        advertising: fusion_hal::contract::drivers::net::bluetooth::BluetoothAdvertisingCaps::from_bits_retain(
            fusion_hal::contract::drivers::net::bluetooth::BluetoothAdvertisingCaps::LEGACY.bits()
                | fusion_hal::contract::drivers::net::bluetooth::BluetoothAdvertisingCaps::CONNECTABLE.bits()
                | fusion_hal::contract::drivers::net::bluetooth::BluetoothAdvertisingCaps::SCANNABLE.bits(),
        ),
        scanning: fusion_hal::contract::drivers::net::bluetooth::BluetoothScanningCaps::empty(),
        connection: fusion_hal::contract::drivers::net::bluetooth::BluetoothConnectionCaps::empty(),
        security: fusion_hal::contract::drivers::net::bluetooth::BluetoothSecurityCaps::empty(),
        l2cap: fusion_hal::contract::drivers::net::bluetooth::BluetoothL2capCaps::empty(),
        att: fusion_hal::contract::drivers::net::bluetooth::BluetoothAttCaps::empty(),
        gatt: fusion_hal::contract::drivers::net::bluetooth::BluetoothGattCaps::empty(),
        iso: fusion_hal::contract::drivers::net::bluetooth::BluetoothIsoCaps::empty(),
        max_connections: 0,
        max_advertising_sets: 1,
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
    shared_bus_pins_configured: bool,
    shared_bus_high_speed: bool,
    shared_bus_backplane_window: Option<u32>,
    wifi_pending_read_command: Option<Cyw43439GspiCommand>,
    bluetooth_transport_ready: bool,
    bluetooth_host_ctrl_cache: u32,
    bluetooth_buffer_layout: Option<Cyw43439BluetoothSharedBufferLayout>,
    activity_gpio: Option<u8>,
    activity_gpio_configured: bool,
    activity_indicator_active: bool,
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
        activity_gpio: Option<u8>,
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
            shared_bus_pins_configured: false,
            shared_bus_high_speed: true,
            shared_bus_backplane_window: None,
            wifi_pending_read_command: None,
            bluetooth_transport_ready: false,
            bluetooth_host_ctrl_cache: 0,
            bluetooth_buffer_layout: None,
            activity_gpio,
            activity_gpio_configured: false,
            activity_indicator_active: false,
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

    fn controller_caps_inner(&self, radio: Cyw43439Radio) -> Cyw43439ControllerCaps {
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
        match radio {
            Cyw43439Radio::Bluetooth => {
                if matches!(
                    self.bluetooth_transport,
                    Cyw43439BluetoothTransport::BoardSharedSpiHci
                ) {
                    caps |= Cyw43439ControllerCaps::TRANSPORT_WRITE
                        | Cyw43439ControllerCaps::TRANSPORT_READ;
                }
                if self.firmware.bluetooth.patch_image.is_some() {
                    caps |= Cyw43439ControllerCaps::FIRMWARE_IMAGE;
                }
            }
            Cyw43439Radio::Wifi => {
                if matches!(self.wifi_transport, Cyw43439WlanTransport::BoardSharedSpi) {
                    caps |= Cyw43439ControllerCaps::IRQ_WAIT
                        | Cyw43439ControllerCaps::IRQ_ACKNOWLEDGE
                        | Cyw43439ControllerCaps::TRANSPORT_WRITE
                        | Cyw43439ControllerCaps::TRANSPORT_READ;
                }
                if self.firmware.wifi.firmware_image.is_some() {
                    caps |= Cyw43439ControllerCaps::FIRMWARE_IMAGE;
                }
                if self.firmware.wifi.nvram_image.is_some() {
                    caps |= Cyw43439ControllerCaps::NVRAM_IMAGE;
                }
            }
        }
        caps
    }

    fn ensure_shared_bus_pins_ready(&mut self) -> Result<(), Cyw43439Error> {
        if self.shared_bus_pins_configured {
            return Ok(());
        }

        self.clock
            .set_function(GpioFunction::Sio)
            .map_err(map_gpio_error)?;
        self.chip_select
            .set_function(GpioFunction::Sio)
            .map_err(map_gpio_error)?;
        self.data_irq
            .set_function(GpioFunction::Sio)
            .map_err(map_gpio_error)?;
        self.clock
            .set_drive_strength(GpioDriveStrength::MilliAmps12)
            .map_err(map_gpio_error)?;
        self.clock
            .set_pull(GpioPull::Down)
            .map_err(map_gpio_error)?;
        self.data_irq
            .set_pull(GpioPull::Down)
            .map_err(map_gpio_error)?;
        self.clock.configure_output(false).map_err(map_gpio_error)?;
        self.chip_select
            .configure_output(true)
            .map_err(map_gpio_error)?;
        self.data_irq.configure_input().map_err(map_gpio_error)?;
        self.shared_bus_pins_configured = true;
        Ok(())
    }

    fn prepare_shared_bus_power_cycle(&mut self) -> Result<(), Cyw43439Error> {
        self.clock
            .set_function(GpioFunction::Sio)
            .map_err(map_gpio_error)?;
        self.chip_select
            .set_function(GpioFunction::Sio)
            .map_err(map_gpio_error)?;
        self.data_irq
            .set_function(GpioFunction::Sio)
            .map_err(map_gpio_error)?;
        self.clock
            .set_drive_strength(GpioDriveStrength::MilliAmps12)
            .map_err(map_gpio_error)?;
        self.clock
            .set_pull(GpioPull::Down)
            .map_err(map_gpio_error)?;
        self.data_irq
            .set_pull(GpioPull::Down)
            .map_err(map_gpio_error)?;
        self.clock.configure_output(false).map_err(map_gpio_error)?;
        self.chip_select
            .configure_output(true)
            .map_err(map_gpio_error)?;
        self.data_irq
            .configure_output(false)
            .map_err(map_gpio_error)?;
        self.shared_bus_pins_configured = false;
        self.shared_bus_high_speed = true;
        self.shared_bus_backplane_window = None;
        self.wifi_pending_read_command = None;
        self.bluetooth_transport_ready = false;
        self.bluetooth_host_ctrl_cache = 0;
        self.bluetooth_buffer_layout = None;
        self.activity_gpio_configured = false;
        self.activity_indicator_active = false;
        CYW43439_BLUETOOTH_PHASE.store(0, Ordering::Release);
        CYW43439_BLUETOOTH_LAST_ERROR.store(0, Ordering::Release);
        CYW43439_BLUETOOTH_LAST_CTRL.store(0, Ordering::Release);
        CYW43439_BLUETOOTH_LAST_HOST_CTRL.store(0, Ordering::Release);
        CYW43439_BLUETOOTH_LAST_H2B_IN.store(0, Ordering::Release);
        CYW43439_BLUETOOTH_LAST_H2B_OUT.store(0, Ordering::Release);
        CYW43439_BLUETOOTH_LAST_B2H_IN.store(0, Ordering::Release);
        CYW43439_BLUETOOTH_LAST_B2H_OUT.store(0, Ordering::Release);
        CYW43439_BLUETOOTH_LAST_SPACE.store(0, Ordering::Release);
        CYW43439_BLUETOOTH_LAST_WRITE_LEN.store(0, Ordering::Release);
        CYW43439_BLUETOOTH_LAST_RING_HEADER.store(0, Ordering::Release);
        for word in &CYW43439_BLUETOOTH_LAST_WRITE_WORDS {
            word.store(0, Ordering::Release);
        }
        for word in &CYW43439_BLUETOOTH_LAST_RING_READBACK_WORDS {
            word.store(0, Ordering::Release);
        }
        Ok(())
    }

    fn shared_bus_half_cycle_pause(&self) {
        for _ in 0..CYW43439_SHARED_SPI_HALF_CYCLE_SPINS {
            core::hint::spin_loop();
        }
    }

    fn shared_bus_select(&mut self) -> Result<(), Cyw43439Error> {
        self.chip_select.set_level(false).map_err(map_gpio_error)
    }

    fn shared_bus_deselect(&mut self) -> Result<(), Cyw43439Error> {
        self.chip_select.set_level(true).map_err(map_gpio_error)?;
        self.shared_bus_half_cycle_pause();
        Ok(())
    }

    fn shared_bus_drive_data_output(&mut self, initial_high: bool) -> Result<(), Cyw43439Error> {
        self.data_irq
            .configure_output(initial_high)
            .map_err(map_gpio_error)?;
        self.shared_bus_half_cycle_pause();
        Ok(())
    }

    fn shared_bus_release_data_input(&mut self) -> Result<(), Cyw43439Error> {
        self.data_irq.configure_input().map_err(map_gpio_error)?;
        self.shared_bus_half_cycle_pause();
        Ok(())
    }

    fn shared_bus_write_bit(&mut self, high: bool) -> Result<(), Cyw43439Error> {
        self.data_irq.set_level(high).map_err(map_gpio_error)?;
        self.shared_bus_half_cycle_pause();
        self.clock.set_level(true).map_err(map_gpio_error)?;
        self.shared_bus_half_cycle_pause();
        self.clock.set_level(false).map_err(map_gpio_error)?;
        self.shared_bus_half_cycle_pause();
        Ok(())
    }

    fn shared_bus_read_bit(&mut self) -> Result<bool, Cyw43439Error> {
        if self.shared_bus_high_speed {
            let sampled = self.data_irq.read().map_err(map_gpio_error)?;
            self.shared_bus_half_cycle_pause();
            self.clock.set_level(true).map_err(map_gpio_error)?;
            self.shared_bus_half_cycle_pause();
            self.clock.set_level(false).map_err(map_gpio_error)?;
            self.shared_bus_half_cycle_pause();
            return Ok(sampled);
        }

        self.clock.set_level(true).map_err(map_gpio_error)?;
        self.shared_bus_half_cycle_pause();
        self.clock.set_level(false).map_err(map_gpio_error)?;
        self.shared_bus_half_cycle_pause();
        self.data_irq.read().map_err(map_gpio_error)
    }

    fn shared_bus_write_bytes(&mut self, payload: &[u8]) -> Result<(), Cyw43439Error> {
        self.ensure_shared_bus_pins_ready()?;
        self.shared_bus_drive_data_output(false)?;
        self.shared_bus_select()?;
        for byte in payload {
            for shift in (0..8).rev() {
                self.shared_bus_write_bit(((byte >> shift) & 1) != 0)?;
            }
        }
        self.shared_bus_release_data_input()?;
        self.shared_bus_deselect()
    }

    fn shared_bus_transfer_read(
        &mut self,
        command: &[u8],
        out: &mut [u8],
    ) -> Result<(), Cyw43439Error> {
        self.ensure_shared_bus_pins_ready()?;
        self.shared_bus_drive_data_output(false)?;
        self.shared_bus_select()?;
        for byte in command {
            for shift in (0..8).rev() {
                self.shared_bus_write_bit(((byte >> shift) & 1) != 0)?;
            }
        }
        self.shared_bus_release_data_input()?;
        for byte in out.iter_mut() {
            let mut value = 0_u8;
            for _ in 0..8 {
                value <<= 1;
                if self.shared_bus_read_bit()? {
                    value |= 1;
                }
            }
            *byte = value;
        }
        self.shared_bus_deselect()
    }

    const fn swap16x2_encode_u32(value: u32) -> [u8; 4] {
        [
            (value >> 8) as u8,
            value as u8,
            (value >> 24) as u8,
            (value >> 16) as u8,
        ]
    }

    const fn swap16x2_decode_u32(bytes: [u8; 4]) -> u32 {
        (bytes[1] as u32)
            | ((bytes[0] as u32) << 8)
            | ((bytes[3] as u32) << 16)
            | ((bytes[2] as u32) << 24)
    }

    fn shared_bus_write_function_bytes(
        &mut self,
        function: Cyw43439GspiFunction,
        address: u32,
        payload: &[u8],
    ) -> Result<(), Cyw43439Error> {
        if payload.is_empty() || payload.len() > 64 {
            return Err(Cyw43439Error::invalid());
        }
        let packet_length = u16::try_from(payload.len()).map_err(|_| Cyw43439Error::invalid())?;
        let command = Cyw43439GspiCommand {
            write: true,
            incrementing: true,
            function,
            address,
            packet_length,
        }
        .encode()
        .ok_or_else(Cyw43439Error::invalid)?;
        let aligned_len = (payload.len() + 3) & !3;
        let mut scratch = [0_u8; 68];
        scratch[..4].copy_from_slice(&command.to_le_bytes());
        scratch[4..4 + payload.len()].copy_from_slice(payload);
        self.shared_bus_write_bytes(&scratch[..4 + aligned_len])
    }

    fn shared_bus_read_register_bytes(
        &mut self,
        function: Cyw43439GspiFunction,
        address: u32,
        len: usize,
        out: &mut [u8],
    ) -> Result<(), Cyw43439Error> {
        if len == 0 || len > out.len() || len > 64 {
            return Err(Cyw43439Error::invalid());
        }
        let packet_length = u16::try_from(len).map_err(|_| Cyw43439Error::invalid())?;
        let command = Cyw43439GspiCommand {
            write: false,
            incrementing: true,
            function,
            address,
            packet_length,
        }
        .encode()
        .ok_or_else(Cyw43439Error::invalid)?;
        let padding = if matches!(function, Cyw43439GspiFunction::F1) {
            CYW43439_GSPI_BACKPLANE_READ_PAD_LEN_BYTES
        } else {
            0
        };
        let aligned_len = (len + 3) & !3;
        let transfer_len = padding + aligned_len;
        let mut scratch = [0_u8; CYW43439_GSPI_BACKPLANE_READ_PAD_LEN_BYTES + 64];
        self.shared_bus_transfer_read(&command.to_le_bytes(), &mut scratch[..transfer_len])?;
        out[..len].copy_from_slice(&scratch[padding..padding + len]);
        Ok(())
    }

    fn shared_bus_write_register_word(
        &mut self,
        function: Cyw43439GspiFunction,
        address: u32,
        packet_length: u16,
        value: u32,
    ) -> Result<(), Cyw43439Error> {
        let command = Cyw43439GspiCommand {
            write: true,
            incrementing: true,
            function,
            address,
            packet_length,
        }
        .encode()
        .ok_or_else(Cyw43439Error::invalid)?;
        let mut payload = [0_u8; 8];
        payload[..4].copy_from_slice(&command.to_le_bytes());
        payload[4..].copy_from_slice(&value.to_le_bytes());
        self.shared_bus_write_bytes(&payload)
    }

    fn shared_bus_set_backplane_window(&mut self, address: u32) -> Result<(), Cyw43439Error> {
        let window = address & !CYW43439_GSPI_BACKPLANE_ADDR_MASK;
        if self.shared_bus_backplane_window == Some(window) {
            return Ok(());
        }
        self.shared_bus_write_register_word(
            Cyw43439GspiFunction::F1,
            CYW43439_GSPI_BACKPLANE_ADDRESS_HIGH,
            1,
            (window >> 24) & 0xff,
        )?;
        self.shared_bus_write_register_word(
            Cyw43439GspiFunction::F1,
            CYW43439_GSPI_BACKPLANE_ADDRESS_MID,
            1,
            (window >> 16) & 0xff,
        )?;
        self.shared_bus_write_register_word(
            Cyw43439GspiFunction::F1,
            CYW43439_GSPI_BACKPLANE_ADDRESS_LOW,
            1,
            (window >> 8) & 0xff,
        )?;
        self.shared_bus_backplane_window = Some(window);
        Ok(())
    }

    fn shared_bus_read_backplane_u32(&mut self, address: u32) -> Result<u32, Cyw43439Error> {
        let mut out = [0_u8; 4];
        self.shared_bus_set_backplane_window(address)?;
        let register = (address & CYW43439_GSPI_BACKPLANE_ADDR_MASK)
            | CYW43439_GSPI_BACKPLANE_ACCESS_2_4B_FLAG;
        self.shared_bus_read_register_bytes(Cyw43439GspiFunction::F1, register, 4, &mut out)?;
        Ok(u32::from_le_bytes(out))
    }

    fn shared_bus_write_backplane_u32(
        &mut self,
        address: u32,
        value: u32,
    ) -> Result<(), Cyw43439Error> {
        self.shared_bus_set_backplane_window(address)?;
        let register = (address & CYW43439_GSPI_BACKPLANE_ADDR_MASK)
            | CYW43439_GSPI_BACKPLANE_ACCESS_2_4B_FLAG;
        self.shared_bus_write_register_word(Cyw43439GspiFunction::F1, register, 4, value)
    }

    fn set_module_activity_indicator_internal(
        &mut self,
        active: bool,
    ) -> Result<(), Cyw43439Error> {
        self.activity_indicator_active = active;

        let Some(activity_gpio) = self.activity_gpio else {
            return Ok(());
        };
        if !self.powered || self.shared_bus_backplane_window.is_none() || activity_gpio >= 32 {
            return Ok(());
        }

        let activity_mask = 1_u32 << activity_gpio;
        if !self.activity_gpio_configured {
            let gpio_out_en_addr =
                CYW43439_GSPI_CHIPCOMMON_BASE_ADDRESS + CYW43439_GSPI_CHIPCOMMON_GPIOOUTEN_OFFSET;
            let gpio_control_addr =
                CYW43439_GSPI_CHIPCOMMON_BASE_ADDRESS + CYW43439_GSPI_CHIPCOMMON_GPIOCONTROL_OFFSET;

            let gpio_out_en = self.shared_bus_read_backplane_u32(gpio_out_en_addr)?;
            self.shared_bus_write_backplane_u32(gpio_out_en_addr, gpio_out_en | activity_mask)?;

            let gpio_control = self.shared_bus_read_backplane_u32(gpio_control_addr)?;
            self.shared_bus_write_backplane_u32(gpio_control_addr, gpio_control & !activity_mask)?;
            self.activity_gpio_configured = true;
        }

        let gpio_out_addr =
            CYW43439_GSPI_CHIPCOMMON_BASE_ADDRESS + CYW43439_GSPI_CHIPCOMMON_GPIOOUT_OFFSET;
        let gpio_out = self.shared_bus_read_backplane_u32(gpio_out_addr)?;
        let desired = if active {
            gpio_out | activity_mask
        } else {
            gpio_out & !activity_mask
        };
        if desired != gpio_out {
            self.shared_bus_write_backplane_u32(gpio_out_addr, desired)?;
        }
        Ok(())
    }

    fn validate_wl_gpio(&self, wl_gpio: u8) -> Result<GpioCapabilities, Cyw43439Error> {
        CYW43439_WL_GPIO_PINS
            .iter()
            .find(|descriptor| descriptor.pin == wl_gpio)
            .map(|descriptor| descriptor.capabilities)
            .ok_or_else(Cyw43439Error::invalid)
    }

    fn configure_wl_gpio_mode(&mut self, wl_gpio: u8) -> Result<GpioCapabilities, Cyw43439Error> {
        let capabilities = self.validate_wl_gpio(wl_gpio)?;
        let gpio_mask = 1_u32 << wl_gpio;
        let gpio_control_addr =
            CYW43439_GSPI_CHIPCOMMON_BASE_ADDRESS + CYW43439_GSPI_CHIPCOMMON_GPIOCONTROL_OFFSET;
        let gpio_control = self.shared_bus_read_backplane_u32(gpio_control_addr)?;
        self.shared_bus_write_backplane_u32(gpio_control_addr, gpio_control & !gpio_mask)?;
        Ok(capabilities)
    }

    fn configure_wl_gpio_input_internal(&mut self, wl_gpio: u8) -> Result<(), Cyw43439Error> {
        self.configure_wl_gpio_mode(wl_gpio)?;
        let gpio_mask = 1_u32 << wl_gpio;
        let gpio_out_en_addr =
            CYW43439_GSPI_CHIPCOMMON_BASE_ADDRESS + CYW43439_GSPI_CHIPCOMMON_GPIOOUTEN_OFFSET;
        let gpio_out_en = self.shared_bus_read_backplane_u32(gpio_out_en_addr)?;
        self.shared_bus_write_backplane_u32(gpio_out_en_addr, gpio_out_en & !gpio_mask)
    }

    fn read_wl_gpio_internal(&mut self, wl_gpio: u8) -> Result<bool, Cyw43439Error> {
        self.validate_wl_gpio(wl_gpio)?;
        let gpio_in_addr =
            CYW43439_GSPI_CHIPCOMMON_BASE_ADDRESS + CYW43439_GSPI_CHIPCOMMON_GPIOIN_OFFSET;
        let gpio_in = self.shared_bus_read_backplane_u32(gpio_in_addr)?;
        Ok((gpio_in & (1_u32 << wl_gpio)) != 0)
    }

    fn configure_wl_gpio_output_internal(
        &mut self,
        wl_gpio: u8,
        initial_high: bool,
    ) -> Result<(), Cyw43439Error> {
        let capabilities = self.configure_wl_gpio_mode(wl_gpio)?;
        if !capabilities.contains(GpioCapabilities::OUTPUT) {
            return Err(Cyw43439Error::unsupported());
        }
        let gpio_mask = 1_u32 << wl_gpio;
        let gpio_out_addr =
            CYW43439_GSPI_CHIPCOMMON_BASE_ADDRESS + CYW43439_GSPI_CHIPCOMMON_GPIOOUT_OFFSET;
        let gpio_out_en_addr =
            CYW43439_GSPI_CHIPCOMMON_BASE_ADDRESS + CYW43439_GSPI_CHIPCOMMON_GPIOOUTEN_OFFSET;
        let gpio_out = self.shared_bus_read_backplane_u32(gpio_out_addr)?;
        let desired_out = if initial_high {
            gpio_out | gpio_mask
        } else {
            gpio_out & !gpio_mask
        };
        if desired_out != gpio_out {
            self.shared_bus_write_backplane_u32(gpio_out_addr, desired_out)?;
        }
        let gpio_out_en = self.shared_bus_read_backplane_u32(gpio_out_en_addr)?;
        self.shared_bus_write_backplane_u32(gpio_out_en_addr, gpio_out_en | gpio_mask)
    }

    fn set_wl_gpio_level_internal(&mut self, wl_gpio: u8, high: bool) -> Result<(), Cyw43439Error> {
        let capabilities = self.validate_wl_gpio(wl_gpio)?;
        if !capabilities.contains(GpioCapabilities::OUTPUT) {
            return Err(Cyw43439Error::unsupported());
        }
        let gpio_mask = 1_u32 << wl_gpio;
        let gpio_out_addr =
            CYW43439_GSPI_CHIPCOMMON_BASE_ADDRESS + CYW43439_GSPI_CHIPCOMMON_GPIOOUT_OFFSET;
        let gpio_out = self.shared_bus_read_backplane_u32(gpio_out_addr)?;
        let desired = if high {
            gpio_out | gpio_mask
        } else {
            gpio_out & !gpio_mask
        };
        if desired != gpio_out {
            self.shared_bus_write_backplane_u32(gpio_out_addr, desired)?;
        }
        Ok(())
    }

    fn shared_bus_write_backplane_bytes(
        &mut self,
        mut address: u32,
        mut payload: &[u8],
    ) -> Result<(), Cyw43439Error> {
        while !payload.is_empty() {
            let offset_in_window = (address & CYW43439_GSPI_BACKPLANE_ADDR_MASK) as usize;
            let remaining_in_window =
                (CYW43439_GSPI_BACKPLANE_ADDR_MASK as usize + 1).saturating_sub(offset_in_window);
            let chunk_len = payload.len().min(64).min(remaining_in_window);
            self.shared_bus_set_backplane_window(address)?;
            let register = (address & CYW43439_GSPI_BACKPLANE_ADDR_MASK)
                | CYW43439_GSPI_BACKPLANE_ACCESS_2_4B_FLAG;
            self.shared_bus_write_function_bytes(
                Cyw43439GspiFunction::F1,
                register,
                &payload[..chunk_len],
            )?;
            address += chunk_len as u32;
            payload = &payload[chunk_len..];
        }
        Ok(())
    }

    fn shared_bus_read_backplane_bytes(
        &mut self,
        mut address: u32,
        mut out: &mut [u8],
    ) -> Result<(), Cyw43439Error> {
        while !out.is_empty() {
            let offset_in_window = (address & CYW43439_GSPI_BACKPLANE_ADDR_MASK) as usize;
            let remaining_in_window =
                (CYW43439_GSPI_BACKPLANE_ADDR_MASK as usize + 1).saturating_sub(offset_in_window);
            let chunk_len = out.len().min(64).min(remaining_in_window);
            self.shared_bus_set_backplane_window(address)?;
            let register = (address & CYW43439_GSPI_BACKPLANE_ADDR_MASK)
                | CYW43439_GSPI_BACKPLANE_ACCESS_2_4B_FLAG;
            self.shared_bus_read_register_bytes(
                Cyw43439GspiFunction::F1,
                register,
                chunk_len,
                &mut out[..chunk_len],
            )?;
            address += chunk_len as u32;
            out = &mut out[chunk_len..];
        }
        Ok(())
    }

    fn bluetooth_shared_buffer_layout(
        &mut self,
    ) -> Result<Cyw43439BluetoothSharedBufferLayout, Cyw43439Error> {
        if let Some(layout) = self.bluetooth_buffer_layout {
            return Ok(layout);
        }
        let wlan_ram_base_addr =
            self.shared_bus_read_backplane_u32(CYW43439_BT_WLAN_RAM_BASE_REG_ADDR)?;
        let layout = Cyw43439BluetoothSharedBufferLayout::from_wlan_ram_base(wlan_ram_base_addr);
        self.bluetooth_buffer_layout = Some(layout);
        Ok(layout)
    }

    fn bluetooth_shared_reg_read(&mut self, reg_addr: u32) -> Result<u32, Cyw43439Error> {
        if reg_addr == CYW43439_BT_HOST_CTRL_REG_ADDR {
            CYW43439_BLUETOOTH_LAST_HOST_CTRL
                .store(self.bluetooth_host_ctrl_cache, Ordering::Release);
            return Ok(self.bluetooth_host_ctrl_cache);
        }
        let value = self.shared_bus_read_backplane_u32(reg_addr)?;
        if reg_addr == CYW43439_BT_CTRL_REG_ADDR {
            CYW43439_BLUETOOTH_LAST_CTRL.store(value, Ordering::Release);
        }
        Ok(value)
    }

    fn bluetooth_shared_reg_write(
        &mut self,
        reg_addr: u32,
        value: u32,
    ) -> Result<(), Cyw43439Error> {
        self.shared_bus_write_backplane_u32(reg_addr, value)?;
        if reg_addr == CYW43439_BT_HOST_CTRL_REG_ADDR {
            self.bluetooth_host_ctrl_cache = value;
            CYW43439_BLUETOOTH_LAST_HOST_CTRL.store(value, Ordering::Release);
        }
        Ok(())
    }

    fn bluetooth_wait_ready(&mut self) -> Result<(), Cyw43439Error> {
        self.delay_ms(CYW43439_BTFW_WAIT_TIME_MS);
        for _ in 0..CYW43439_BTSDIO_FW_READY_POLLING_RETRY_COUNT {
            let reg = self.bluetooth_shared_reg_read(CYW43439_BT_CTRL_REG_ADDR)?;
            if (reg & CYW43439_BTSDIO_REG_FW_RDY_BITMASK) != 0 {
                return Ok(());
            }
            self.delay_ms(CYW43439_BTSDIO_FW_READY_POLLING_INTERVAL_MS);
        }
        Err(Cyw43439Error::busy())
    }

    fn bluetooth_wait_awake(&mut self) -> Result<(), Cyw43439Error> {
        for _ in 0..CYW43439_BTSDIO_FW_AWAKE_POLLING_RETRY_COUNT {
            let reg = self.bluetooth_shared_reg_read(CYW43439_BT_CTRL_REG_ADDR)?;
            if (reg & CYW43439_BTSDIO_REG_BT_AWAKE_BITMASK) != 0 {
                return Ok(());
            }
            self.delay_ms(CYW43439_BTSDIO_FW_AWAKE_POLLING_INTERVAL_MS);
        }
        Err(Cyw43439Error::busy())
    }

    fn bluetooth_set_awake(&mut self, awake: bool) -> Result<(), Cyw43439Error> {
        let before = self.bluetooth_host_ctrl_cache;
        let mut after = before;
        if awake {
            after |= CYW43439_BTSDIO_REG_WAKE_BT_BITMASK;
        } else {
            after &= !CYW43439_BTSDIO_REG_WAKE_BT_BITMASK;
        }
        if after != before {
            self.bluetooth_shared_reg_write(CYW43439_BT_HOST_CTRL_REG_ADDR, after)?;
        }
        Ok(())
    }

    fn bluetooth_toggle_data_valid(&mut self) -> Result<(), Cyw43439Error> {
        let next = self.bluetooth_host_ctrl_cache ^ CYW43439_BTSDIO_REG_DATA_VALID_BITMASK;
        self.bluetooth_shared_reg_write(CYW43439_BT_HOST_CTRL_REG_ADDR, next)
    }

    fn bluetooth_set_host_ready(&mut self) -> Result<(), Cyw43439Error> {
        let next = self.bluetooth_host_ctrl_cache | CYW43439_BTSDIO_REG_SW_RDY_BITMASK;
        self.bluetooth_shared_reg_write(CYW43439_BT_HOST_CTRL_REG_ADDR, next)
    }

    fn bluetooth_get_buffer_index(
        &mut self,
    ) -> Result<Cyw43439BluetoothSharedBufferIndex, Cyw43439Error> {
        let layout = self.bluetooth_shared_buffer_layout()?;
        let mut raw = [0_u8; 16];
        self.shared_bus_read_backplane_bytes(layout.host2bt_in_addr, &mut raw)?;
        let indices = Cyw43439BluetoothSharedBufferIndex {
            host2bt_in_val: u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]),
            host2bt_out_val: u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]),
            bt2host_in_val: u32::from_le_bytes([raw[8], raw[9], raw[10], raw[11]]),
            bt2host_out_val: u32::from_le_bytes([raw[12], raw[13], raw[14], raw[15]]),
        };
        CYW43439_BLUETOOTH_LAST_H2B_IN.store(indices.host2bt_in_val, Ordering::Release);
        CYW43439_BLUETOOTH_LAST_H2B_OUT.store(indices.host2bt_out_val, Ordering::Release);
        CYW43439_BLUETOOTH_LAST_B2H_IN.store(indices.bt2host_in_val, Ordering::Release);
        CYW43439_BLUETOOTH_LAST_B2H_OUT.store(indices.bt2host_out_val, Ordering::Release);
        Ok(indices)
    }

    fn bluetooth_circ_buf_count(in_val: u32, out_val: u32) -> u32 {
        in_val.wrapping_sub(out_val) & (CYW43439_BTSDIO_FWBUF_SIZE - 1)
    }

    fn bluetooth_circ_buf_space(in_val: u32, out_val: u32) -> u32 {
        Self::bluetooth_circ_buf_count(out_val, in_val.wrapping_add(4))
    }

    fn bluetooth_mem_read_ring(
        &mut self,
        base_addr: u32,
        offset: u32,
        out: &mut [u8],
    ) -> Result<(), Cyw43439Error> {
        let first_len = ((CYW43439_BTSDIO_FWBUF_SIZE - offset) as usize).min(out.len());
        self.shared_bus_read_backplane_bytes(base_addr + offset, &mut out[..first_len])?;
        if out.len() > first_len {
            self.shared_bus_read_backplane_bytes(base_addr, &mut out[first_len..])?;
        }
        Ok(())
    }

    fn bluetooth_fill_framed_tx_bytes(
        header: [u8; 4],
        body: &[u8],
        logical_offset: usize,
        out: &mut [u8],
    ) {
        for (index, slot) in out.iter_mut().enumerate() {
            let absolute = logical_offset + index;
            *slot = if absolute < header.len() {
                header[absolute]
            } else {
                body.get(absolute - header.len()).copied().unwrap_or(0)
            };
        }
    }

    fn bluetooth_mem_write_framed_segment(
        &mut self,
        write_addr: u32,
        header: [u8; 4],
        body: &[u8],
        logical_offset: usize,
        len: usize,
    ) -> Result<(), Cyw43439Error> {
        let mut local_offset = 0_usize;
        let mut chunk = [0_u8; 64];
        while local_offset < len {
            let chunk_len = (len - local_offset).min(chunk.len());
            Self::bluetooth_fill_framed_tx_bytes(
                header,
                body,
                logical_offset + local_offset,
                &mut chunk[..chunk_len],
            );
            self.shared_bus_write_backplane_bytes(
                write_addr + local_offset as u32,
                &chunk[..chunk_len],
            )?;
            local_offset += chunk_len;
        }
        Ok(())
    }

    fn bluetooth_mem_write_framed_packet(
        &mut self,
        base_addr: u32,
        offset: u32,
        header: [u8; 4],
        body: &[u8],
        aligned_len: usize,
    ) -> Result<u32, Cyw43439Error> {
        let ring_len = CYW43439_BTSDIO_FWBUF_SIZE as usize;
        let offset = offset as usize;
        if offset + aligned_len <= ring_len {
            self.bluetooth_mem_write_framed_segment(
                base_addr + offset as u32,
                header,
                body,
                0,
                aligned_len,
            )?;
            return Ok(offset as u32);
        }

        let first_len = ring_len - offset;
        if first_len < header.len() {
            self.bluetooth_mem_write_framed_segment(base_addr, header, body, 0, aligned_len)?;
            return Ok(0);
        }

        self.bluetooth_mem_write_framed_segment(
            base_addr + offset as u32,
            header,
            body,
            0,
            first_len,
        )?;
        self.bluetooth_mem_write_framed_segment(
            base_addr,
            header,
            body,
            first_len,
            aligned_len - first_len,
        )?;
        Ok(offset as u32)
    }

    fn bluetooth_write_patch_record(
        &mut self,
        dest_addr: u32,
        payload: &[u8],
    ) -> Result<(), Cyw43439Error> {
        if payload.len() > 255 {
            return Err(Cyw43439Error::resource_exhausted());
        }

        let mut staging = [0_u8; CYW43439_BT_PATCH_STAGING_BYTES];
        let mut write_addr = CYW43439_BTFW_MEM_OFFSET + dest_addr;
        let mut write_len = 0_usize;

        if (write_addr & 0x3) != 0 {
            let aligned_addr = write_addr & !0x3;
            let leading = (write_addr & 0x3) as usize;
            let prefix = self
                .shared_bus_read_backplane_u32(aligned_addr)?
                .to_le_bytes();
            staging[..leading].copy_from_slice(&prefix[..leading]);
            write_addr = aligned_addr;
            write_len = leading;
        }

        if staging.len() < write_len + payload.len() {
            return Err(Cyw43439Error::resource_exhausted());
        }
        staging[write_len..write_len + payload.len()].copy_from_slice(payload);
        write_len += payload.len();

        let end_addr = write_addr + write_len as u32;
        if (end_addr & 0x3) != 0 {
            let aligned_tail = end_addr & !0x3;
            let tail_word = self
                .shared_bus_read_backplane_u32(aligned_tail)?
                .to_le_bytes();
            let trailing_start = (end_addr & 0x3) as usize;
            let trailing = 4 - trailing_start;
            if staging.len() < write_len + trailing {
                return Err(Cyw43439Error::resource_exhausted());
            }
            staging[write_len..write_len + trailing].copy_from_slice(&tail_word[trailing_start..]);
            write_len += trailing;
        }

        self.shared_bus_write_backplane_bytes(write_addr, &staging[..write_len])
    }

    fn ensure_bluetooth_shared_transport_ready(&mut self) -> Result<(), Cyw43439Error> {
        if self.bluetooth_transport_ready {
            CYW43439_BLUETOOTH_PHASE.store(90, Ordering::Release);
            return Ok(());
        }
        CYW43439_BLUETOOTH_PHASE.store(1, Ordering::Release);
        CYW43439_BLUETOOTH_LAST_ERROR.store(0, Ordering::Release);
        let patch = self
            .firmware
            .bluetooth
            .patch_image
            .ok_or_else(Cyw43439Error::unsupported)?;
        self.shared_bus_write_backplane_u32(
            CYW43439_BTFW_MEM_OFFSET + CYW43439_BT2WLAN_PWRUP_ADDR,
            CYW43439_BT2WLAN_PWRUP_WAKE,
        )
        .inspect_err(|_| {
            CYW43439_BLUETOOTH_LAST_ERROR.store(1, Ordering::Release);
        })?;
        CYW43439_BLUETOOTH_PHASE.store(2, Ordering::Release);
        for_each_patch_data_record(patch, |dest_addr, data| {
            self.bluetooth_write_patch_record(dest_addr, data)
        })
        .inspect_err(|_| {
            CYW43439_BLUETOOTH_LAST_ERROR.store(2, Ordering::Release);
        })?;
        CYW43439_BLUETOOTH_PHASE.store(3, Ordering::Release);
        self.bluetooth_wait_ready().inspect_err(|_| {
            CYW43439_BLUETOOTH_LAST_ERROR.store(3, Ordering::Release);
        })?;
        CYW43439_BLUETOOTH_PHASE.store(4, Ordering::Release);
        let layout = self.bluetooth_shared_buffer_layout()?;
        CYW43439_BLUETOOTH_PHASE.store(5, Ordering::Release);
        self.bluetooth_shared_reg_write(layout.host2bt_in_addr, 0)
            .inspect_err(|_| {
                CYW43439_BLUETOOTH_LAST_ERROR.store(4, Ordering::Release);
            })?;
        self.bluetooth_shared_reg_write(layout.host2bt_out_addr, 0)
            .inspect_err(|_| {
                CYW43439_BLUETOOTH_LAST_ERROR.store(5, Ordering::Release);
            })?;
        self.bluetooth_shared_reg_write(layout.bt2host_in_addr, 0)
            .inspect_err(|_| {
                CYW43439_BLUETOOTH_LAST_ERROR.store(6, Ordering::Release);
            })?;
        self.bluetooth_shared_reg_write(layout.bt2host_out_addr, 0)
            .inspect_err(|_| {
                CYW43439_BLUETOOTH_LAST_ERROR.store(7, Ordering::Release);
            })?;
        CYW43439_BLUETOOTH_PHASE.store(6, Ordering::Release);
        self.bluetooth_wait_awake().inspect_err(|_| {
            CYW43439_BLUETOOTH_LAST_ERROR.store(8, Ordering::Release);
        })?;
        CYW43439_BLUETOOTH_PHASE.store(7, Ordering::Release);
        self.bluetooth_set_host_ready().inspect_err(|_| {
            CYW43439_BLUETOOTH_LAST_ERROR.store(9, Ordering::Release);
        })?;
        CYW43439_BLUETOOTH_PHASE.store(8, Ordering::Release);
        self.bluetooth_toggle_data_valid().inspect_err(|_| {
            CYW43439_BLUETOOTH_LAST_ERROR.store(10, Ordering::Release);
        })?;
        self.bluetooth_transport_ready = true;
        CYW43439_BLUETOOTH_PHASE.store(9, Ordering::Release);
        Ok(())
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
            self.controller_caps_inner(radio)
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
        if matches!(self.wifi_transport, Cyw43439WlanTransport::BoardSharedSpi) {
            self.prepare_shared_bus_power_cycle()?;
        }
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
        timeout_ms: Option<u32>,
    ) -> Result<bool, Cyw43439Error> {
        if !self.radio_available(radio) {
            return Err(Cyw43439Error::unsupported());
        }
        if !self.transport_held(radio) {
            return Err(Cyw43439Error::state_conflict());
        }
        if radio == Cyw43439Radio::Bluetooth
            && matches!(
                self.bluetooth_transport,
                Cyw43439BluetoothTransport::BoardSharedSpiHci
            )
        {
            let timeout = timeout_ms.unwrap_or(0);
            for elapsed in 0..=timeout {
                if self.bluetooth_transport_ready {
                    let indices = self.bluetooth_get_buffer_index()?;
                    if Self::bluetooth_circ_buf_count(
                        indices.bt2host_in_val,
                        indices.bt2host_out_val,
                    ) != 0
                    {
                        return Ok(true);
                    }
                }
                if timeout == 0 {
                    return Ok(false);
                }
                if elapsed != timeout {
                    self.delay_ms(1);
                }
            }
            return Ok(false);
        }
        if radio != Cyw43439Radio::Wifi
            || !matches!(self.wifi_transport, Cyw43439WlanTransport::BoardSharedSpi)
        {
            return Err(Cyw43439Error::unsupported());
        }

        self.ensure_shared_bus_pins_ready()?;
        self.shared_bus_release_data_input()?;
        let timeout = timeout_ms.unwrap_or(0);
        for elapsed in 0..=timeout {
            let level = self.data_irq.read().map_err(map_gpio_error)?;
            if level == CYW43439_SHARED_SPI_HOST_WAKE_IRQ_HIGH {
                return Ok(true);
            }
            if timeout == 0 {
                return Ok(false);
            }
            if elapsed != timeout {
                self.delay_ms(1);
            }
        }
        Ok(false)
    }

    fn acknowledge_controller_irq(&mut self, radio: Cyw43439Radio) -> Result<(), Cyw43439Error> {
        if !self.radio_available(radio) {
            return Err(Cyw43439Error::unsupported());
        }
        if !self.transport_held(radio) {
            return Err(Cyw43439Error::state_conflict());
        }
        if radio == Cyw43439Radio::Bluetooth
            && matches!(
                self.bluetooth_transport,
                Cyw43439BluetoothTransport::BoardSharedSpiHci
            )
        {
            return Ok(());
        }
        if radio != Cyw43439Radio::Wifi
            || !matches!(self.wifi_transport, Cyw43439WlanTransport::BoardSharedSpi)
        {
            return Err(Cyw43439Error::unsupported());
        }
        Ok(())
    }

    fn write_controller_transport(
        &mut self,
        radio: Cyw43439Radio,
        payload: &[u8],
    ) -> Result<(), Cyw43439Error> {
        if !self.radio_available(radio) {
            return Err(Cyw43439Error::unsupported());
        }
        if !self.transport_held(radio) {
            return Err(Cyw43439Error::state_conflict());
        }
        if radio == Cyw43439Radio::Bluetooth
            && matches!(
                self.bluetooth_transport,
                Cyw43439BluetoothTransport::BoardSharedSpiHci
            )
        {
            if payload.is_empty() {
                return Err(Cyw43439Error::invalid());
            }
            CYW43439_BLUETOOTH_PHASE.store(20, Ordering::Release);
            self.ensure_bluetooth_shared_transport_ready()
                .inspect_err(|_| {
                    CYW43439_BLUETOOTH_LAST_ERROR.store(20, Ordering::Release);
                })?;
            CYW43439_BLUETOOTH_PHASE.store(21, Ordering::Release);
            self.bluetooth_set_awake(true).inspect_err(|_| {
                CYW43439_BLUETOOTH_LAST_ERROR.store(21, Ordering::Release);
            })?;
            self.bluetooth_wait_awake().inspect_err(|_| {
                CYW43439_BLUETOOTH_LAST_ERROR.store(22, Ordering::Release);
            })?;

            let layout = self.bluetooth_shared_buffer_layout()?;
            let indices = self.bluetooth_get_buffer_index()?;
            let body_len = payload.len() - 1;
            let total_len = 4 + body_len;
            let aligned_len = bt_shared_round_up_4(total_len);
            let space =
                Self::bluetooth_circ_buf_space(indices.host2bt_in_val, indices.host2bt_out_val);
            CYW43439_BLUETOOTH_LAST_SPACE.store(space, Ordering::Release);
            CYW43439_BLUETOOTH_PHASE.store(22, Ordering::Release);
            if aligned_len as u32 > space {
                CYW43439_BLUETOOTH_LAST_ERROR.store(23, Ordering::Release);
                return Err(Cyw43439Error::busy());
            }

            let mut header = [0_u8; 4];
            header[0] = (body_len & 0xff) as u8;
            header[1] = ((body_len >> 8) & 0xff) as u8;
            header[2] = 0;
            header[3] = payload[0];
            CYW43439_BLUETOOTH_LAST_WRITE_LEN.store(payload.len() as u32, Ordering::Release);
            CYW43439_BLUETOOTH_LAST_RING_HEADER
                .store(u32::from_le_bytes(header), Ordering::Release);
            for (index, word) in CYW43439_BLUETOOTH_LAST_WRITE_WORDS.iter().enumerate() {
                let base = index * 4;
                let mut bytes = [0_u8; 4];
                for (offset, slot) in bytes.iter_mut().enumerate() {
                    if let Some(byte) = payload.get(base + offset) {
                        *slot = *byte;
                    }
                }
                word.store(u32::from_le_bytes(bytes), Ordering::Release);
            }

            let write_offset = indices.host2bt_in_val & (CYW43439_BTSDIO_FWBUF_SIZE - 1);
            let readback_offset = self
                .bluetooth_mem_write_framed_packet(
                    layout.host2bt_buf_addr,
                    write_offset,
                    header,
                    &payload[1..],
                    aligned_len,
                )
                .inspect_err(|_| {
                    CYW43439_BLUETOOTH_LAST_ERROR.store(24, Ordering::Release);
                })?;

            let mut readback = [0_u8; 32];
            let readback_len = readback.len().min(aligned_len);
            self.bluetooth_mem_read_ring(
                layout.host2bt_buf_addr,
                readback_offset,
                &mut readback[..readback_len],
            )
            .inspect_err(|_| {
                CYW43439_BLUETOOTH_LAST_ERROR.store(25, Ordering::Release);
            })?;
            for (index, word) in CYW43439_BLUETOOTH_LAST_RING_READBACK_WORDS
                .iter()
                .enumerate()
            {
                let base = index * 4;
                let mut bytes = [0_u8; 4];
                if base < readback_len {
                    let end = (base + 4).min(readback_len);
                    bytes[..end - base].copy_from_slice(&readback[base..end]);
                }
                word.store(u32::from_le_bytes(bytes), Ordering::Release);
            }

            let new_h2b_in = indices.host2bt_in_val.wrapping_add(aligned_len as u32)
                & (CYW43439_BTSDIO_FWBUF_SIZE - 1);
            CYW43439_BLUETOOTH_PHASE.store(23, Ordering::Release);
            self.bluetooth_shared_reg_write(layout.host2bt_in_addr, new_h2b_in)
                .inspect_err(|_| {
                    CYW43439_BLUETOOTH_LAST_ERROR.store(26, Ordering::Release);
                })?;
            CYW43439_BLUETOOTH_PHASE.store(24, Ordering::Release);
            self.bluetooth_toggle_data_valid().inspect_err(|_| {
                CYW43439_BLUETOOTH_LAST_ERROR.store(27, Ordering::Release);
            })?;
            CYW43439_BLUETOOTH_PHASE.store(25, Ordering::Release);
            return Ok(());
        }
        if radio != Cyw43439Radio::Wifi
            || !matches!(self.wifi_transport, Cyw43439WlanTransport::BoardSharedSpi)
        {
            return Err(Cyw43439Error::unsupported());
        }

        if payload.len() == 4 {
            let command = Cyw43439GspiCommand::decode(u32::from_le_bytes([
                payload[0], payload[1], payload[2], payload[3],
            ]));
            if !command.write {
                self.wifi_pending_read_command = Some(command);
                return Ok(());
            }
        }

        self.wifi_pending_read_command = None;
        self.shared_bus_write_bytes(payload)
    }

    fn read_controller_transport(
        &mut self,
        radio: Cyw43439Radio,
        out: &mut [u8],
    ) -> Result<usize, Cyw43439Error> {
        if !self.radio_available(radio) {
            return Err(Cyw43439Error::unsupported());
        }
        if !self.transport_held(radio) {
            return Err(Cyw43439Error::state_conflict());
        }
        if radio == Cyw43439Radio::Bluetooth
            && matches!(
                self.bluetooth_transport,
                Cyw43439BluetoothTransport::BoardSharedSpiHci
            )
        {
            self.ensure_bluetooth_shared_transport_ready()?;
            self.bluetooth_set_awake(true)?;
            self.bluetooth_wait_awake()?;

            let layout = self.bluetooth_shared_buffer_layout()?;
            let indices = self.bluetooth_get_buffer_index()?;
            let available =
                Self::bluetooth_circ_buf_count(indices.bt2host_in_val, indices.bt2host_out_val);
            if available == 0 {
                return Ok(0);
            }

            let read_offset = indices.bt2host_out_val & (CYW43439_BTSDIO_FWBUF_SIZE - 1);
            let mut header = [0_u8; 4];
            self.bluetooth_mem_read_ring(layout.bt2host_buf_addr, read_offset, &mut header)?;
            let body_len = usize::from(header[0])
                | (usize::from(header[1]) << 8)
                | (usize::from(header[2]) << 16);
            let total_aligned = bt_shared_round_up_4(body_len);
            if out.len() < body_len + 1 {
                return Err(Cyw43439Error::resource_exhausted());
            }

            out[0] = header[3];
            if body_len != 0 {
                let body_offset = (read_offset + 4) & (CYW43439_BTSDIO_FWBUF_SIZE - 1);
                self.bluetooth_mem_read_ring(
                    layout.bt2host_buf_addr,
                    body_offset,
                    &mut out[1..1 + body_len],
                )?;
            }
            let new_b2h_out = indices
                .bt2host_out_val
                .wrapping_add(4 + total_aligned as u32)
                & (CYW43439_BTSDIO_FWBUF_SIZE - 1);
            self.bluetooth_shared_reg_write(layout.bt2host_out_addr, new_b2h_out)?;
            self.bluetooth_toggle_data_valid()?;
            return Ok(body_len + 1);
        }
        if radio != Cyw43439Radio::Wifi
            || !matches!(self.wifi_transport, Cyw43439WlanTransport::BoardSharedSpi)
        {
            return Err(Cyw43439Error::unsupported());
        }

        let Some(command) = self.wifi_pending_read_command.take() else {
            return Err(Cyw43439Error::unsupported());
        };
        let minimum_len = usize::from(command.packet_length);
        if out.len() < minimum_len {
            return Err(Cyw43439Error::resource_exhausted());
        }
        let read_len = out.len();
        self.shared_bus_transfer_read(
            &command
                .encode()
                .ok_or_else(Cyw43439Error::invalid)?
                .to_le_bytes(),
            &mut out[..read_len],
        )?;
        Ok(read_len)
    }

    fn set_driver_activity_indicator(&mut self, active: bool) -> Result<(), Cyw43439Error> {
        self.set_module_activity_indicator_internal(active)
    }

    fn wl_gpio_support(&self) -> GpioSupport {
        GpioSupport {
            caps: GpioProviderCaps::ENUMERATE
                | GpioProviderCaps::CLAIM
                | GpioProviderCaps::STATIC_TOPOLOGY
                | GpioProviderCaps::INPUT
                | GpioProviderCaps::OUTPUT,
            implementation: GpioImplementationKind::Native,
            pin_count: CYW43439_WL_GPIO_COUNT,
        }
    }

    fn wl_gpio_pins(&self) -> &'static [GpioPinDescriptor] {
        &CYW43439_WL_GPIO_PINS
    }

    fn wl_gpio_capabilities(&self, wl_gpio: u8) -> Result<GpioCapabilities, Cyw43439Error> {
        self.validate_wl_gpio(wl_gpio)
    }

    fn configure_wl_gpio_input(&mut self, wl_gpio: u8) -> Result<(), Cyw43439Error> {
        self.configure_wl_gpio_input_internal(wl_gpio)
    }

    fn read_wl_gpio(&mut self, wl_gpio: u8) -> Result<bool, Cyw43439Error> {
        self.read_wl_gpio_internal(wl_gpio)
    }

    fn configure_wl_gpio_output(
        &mut self,
        wl_gpio: u8,
        initial_high: bool,
    ) -> Result<(), Cyw43439Error> {
        self.configure_wl_gpio_output_internal(wl_gpio, initial_high)
    }

    fn set_wl_gpio_level(&mut self, wl_gpio: u8, high: bool) -> Result<(), Cyw43439Error> {
        self.set_wl_gpio_level_internal(wl_gpio, high)
    }

    fn bootstrap_read_wlan_register_swapped_u32(
        &mut self,
        register: Cyw43439GspiF0Register,
    ) -> Result<u32, Cyw43439Error> {
        if !self.wifi_available
            || !matches!(self.wifi_transport, Cyw43439WlanTransport::BoardSharedSpi)
        {
            return Err(Cyw43439Error::unsupported());
        }
        let command = Cyw43439GspiCommand::read_f0(register)
            .encode()
            .ok_or_else(Cyw43439Error::invalid)?;
        let mut response = [0_u8; 4];
        self.shared_bus_transfer_read(&Self::swap16x2_encode_u32(command), &mut response)?;
        CYW43439_SHARED_BUS_LAST_READ_RAW.store(u32::from_le_bytes(response), Ordering::Release);
        Ok(Self::swap16x2_decode_u32(response))
    }

    fn bootstrap_write_wlan_register_swapped_u32(
        &mut self,
        register: Cyw43439GspiF0Register,
        value: u32,
    ) -> Result<(), Cyw43439Error> {
        if !self.wifi_available
            || !matches!(self.wifi_transport, Cyw43439WlanTransport::BoardSharedSpi)
        {
            return Err(Cyw43439Error::unsupported());
        }
        let command = Cyw43439GspiCommand::write_f0(register, 4)
            .encode()
            .ok_or_else(Cyw43439Error::invalid)?;
        if register == Cyw43439GspiF0Register::BusControl {
            let flags = Cyw43439GspiBusControlFlags::from_bits_retain((value & 0xffff) as u16);
            self.shared_bus_high_speed = flags.contains(Cyw43439GspiBusControlFlags::HIGH_SPEED);
        }
        let mut payload = [0_u8; 8];
        payload[..4].copy_from_slice(&Self::swap16x2_encode_u32(command));
        payload[4..].copy_from_slice(&Self::swap16x2_encode_u32(value));
        self.shared_bus_write_bytes(&payload)
    }

    fn bootstrap_write_raw_bytes(&mut self, payload: &[u8]) -> Result<(), Cyw43439Error> {
        if !self.wifi_available
            || !matches!(self.wifi_transport, Cyw43439WlanTransport::BoardSharedSpi)
        {
            return Err(Cyw43439Error::unsupported());
        }
        self.shared_bus_write_bytes(payload)
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

fn map_gpio_error(error: fusion_hal::contract::drivers::bus::gpio::GpioError) -> Cyw43439Error {
    match error.kind() {
        fusion_hal::contract::drivers::bus::gpio::GpioErrorKind::Unsupported => {
            Cyw43439Error::unsupported()
        }
        fusion_hal::contract::drivers::bus::gpio::GpioErrorKind::Invalid => {
            Cyw43439Error::invalid()
        }
        fusion_hal::contract::drivers::bus::gpio::GpioErrorKind::Busy => Cyw43439Error::busy(),
        fusion_hal::contract::drivers::bus::gpio::GpioErrorKind::ResourceExhausted => {
            Cyw43439Error::resource_exhausted()
        }
        fusion_hal::contract::drivers::bus::gpio::GpioErrorKind::StateConflict => {
            Cyw43439Error::state_conflict()
        }
        fusion_hal::contract::drivers::bus::gpio::GpioErrorKind::Platform(code) => {
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
    use fusion_hal::contract::drivers::bus::gpio::{
        GpioCapabilities,
        GpioDriveStrength,
        GpioError,
        GpioFunction,
        GpioPull,
    };
    use fd_bus_gpio::GpioPin;
    use fd_bus_gpio::interface::contract::GpioHardwarePin;
    use crate::firmware::Cyw43439FirmwareAssets;
    use crate::interface::contract::Cyw43439ErrorKind;
    use crate::transport::{
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
