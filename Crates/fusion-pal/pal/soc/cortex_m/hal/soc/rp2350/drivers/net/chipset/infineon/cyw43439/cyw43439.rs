//! RP2350-selected CYW43439 combo-chip driver exports.
//!
//! # Licensed External Asset Readme
//!
//! This is the only RP2350 / Pico 2 W Rust module in Fusion that directly embeds third-party
//! CYW43439 firmware/configuration blobs.
//!
//! Embedded assets:
//! - `assets/w43439A0_7_95_49_00_combined.bin`
//! - `assets/wb43439A0_7_95_49_00_combined.bin`
//! - `assets/wifi_nvram_43439.bin`
//! - `assets/cyw43_btfw_43439.bin`
//!
//! Provenance:
//! - derived from `georgerobotics/cyw43-driver` commit
//!   `dd7568229f3bf7a37737b9e1ef250c26efe75b23`
//! - the Wi-Fi combined images and Bluetooth patch payload come from the Raspberry Pi CYW43
//!   support stack for Pico wireless boards
//! - the NVRAM payload retains upstream Broadcom-origin board configuration content
//!
//! Licensing and obligations:
//! - these embedded resources are not owned or relicensed by Fusion
//! - the upstream `cyw43-driver` project ships the relevant Pico redistribution terms in
//!   `LICENSE.RP`; a local copy is vendored adjacent to this file
//! - use/redistribution is constrained to Raspberry Pi semiconductor devices under those terms
//! - source redistribution must retain the upstream copyright/license notice
//! - binary redistribution must reproduce that notice in accompanying documentation/materials
//! - if these assets are updated, the provenance and obligation text in this file must be updated
//!   at the same time
//!
//! Boundary rule:
//! - CYW43439-specific packed-firmware layout logic belongs in the CYW43439 driver crate
//! - Pico 2 W-specific asset selection and embedding belongs here
//! - no other RP2350 or CYW43439 Rust file should directly embed or include these licensed blobs
//!   unless this boundary is deliberately reworked
//!
//! The selected RP2350 board contract currently follows Pico 2 W wiring truth. This module owns
//! the shared CYW43439 GPIO transport once and vends both Bluetooth and Wi-Fi driver families
//! over that one shared chip substrate.

use core::cell::UnsafeCell;
use core::hint::spin_loop;
use core::mem::MaybeUninit;
use core::sync::atomic::{
    AtomicBool,
    AtomicU8,
    Ordering,
};

use fusion_hal::contract::drivers::bus::gpio::{
    GpioError,
    GpioErrorKind,
};
use fusion_hal::contract::drivers::driver::{
    DriverActivationContext,
    DriverDiscoveryContext,
    DriverError,
    DriverErrorKind,
    DriverRegistry,
};
use fusion_hal::contract::drivers::net::bluetooth::{
    BluetoothAdapterId,
    BluetoothError,
    BluetoothSupport,
};
use fusion_hal::contract::drivers::net::wifi::{
    WifiAdapterId,
    WifiError,
    WifiSupport,
};
use fusion_hal::drivers::bus::gpio::GpioPin as HalGpioPin;
use fd_net_chipset_infineon_cyw43439::{
    bluetooth::{
        CYW43439 as UniversalBluetoothCYW43439,
        Cyw43439Binding as Cyw43439BluetoothBinding,
        Cyw43439Driver as Cyw43439BluetoothDriver,
        Cyw43439DriverContext as Cyw43439BluetoothDriverContext,
    },
    interface::{
        backend::{
            gpio::{
                GpioBackend as Cyw43439GpioBackend,
            },
        },
        contract::{
            Cyw43439ControllerCaps,
            Cyw43439Error,
            Cyw43439ErrorKind,
            Cyw43439HardwareContract,
            Cyw43439Radio,
        },
    },
    firmware::{
        Cyw43439FirmwareAssets,
        Cyw43439PackedWlanFirmwareImage,
    },
    transport::{
        Cyw43439BluetoothTransport,
        Cyw43439BluetoothTransportClockProfile,
        Cyw43439TransportTopology,
        Cyw43439WlanTransport,
        Cyw43439WlanTransportClockProfile,
    },
    wifi::{
        CYW43439 as UniversalWifiCYW43439,
        Cyw43439Binding as Cyw43439WifiBinding,
        Cyw43439Driver as Cyw43439WifiDriver,
        Cyw43439DriverContext as Cyw43439WifiDriverContext,
    },
};

use crate::pal::soc::cortex_m::rp2350::{
    CortexMBluetoothControllerBinding,
    CortexMBluetoothTransportBinding,
    CortexMWifiControllerBinding,
    CortexMWifiTransportBinding,
    bluetooth_controllers,
    current_sys_clock_hz,
    monotonic_raw_now,
    monotonic_tick_hz,
    wifi_controllers,
};
use crate::pal::soc::cortex_m::rp2350::drivers::bus::gpio::{
    GpioPinHardware,
    claim_board_owned_pin,
};

type SharedBackend = Cyw43439GpioBackend<
    GpioPinHardware,
    GpioPinHardware,
    GpioPinHardware,
    GpioPinHardware,
    GpioPinHardware,
    GpioPinHardware,
>;

const INIT_UNINITIALIZED: u8 = 0;
const INIT_RUNNING: u8 = 1;
const INIT_READY: u8 = 2;
const PICO2W_CYW43439_WIFI_ONLY_COMBINED_FW: &[u8] =
    include_bytes!("assets/w43439A0_7_95_49_00_combined.bin");
const PICO2W_CYW43439_WIFI_BT_COMBINED_FW: &[u8] =
    include_bytes!("assets/wb43439A0_7_95_49_00_combined.bin");
const PICO2W_CYW43439_BT_PATCH: &[u8] = include_bytes!("assets/cyw43_btfw_43439.bin");
const PICO2W_CYW43439_WIFI_NVRAM: &[u8] = include_bytes!("assets/wifi_nvram_43439.bin");
const PICO2W_CYW43439_WIFI_ONLY_FW_LEN: usize = 224_190;
const PICO2W_CYW43439_WIFI_ONLY_CLM_LEN: usize = 984;
const PICO2W_CYW43439_WIFI_BT_FW_LEN: usize = 231_077;
const PICO2W_CYW43439_WIFI_BT_CLM_LEN: usize = 984;

/// Selected universal Bluetooth driver composed over the RP2350 Pico 2 W CYW43439 wiring.
pub type Bluetooth = UniversalBluetoothCYW43439<SelectedCyw43439Hardware>;
/// Selected universal Wi-Fi driver composed over the RP2350 Pico 2 W CYW43439 wiring.
pub type Wifi = UniversalWifiCYW43439<SelectedCyw43439Hardware>;

#[derive(Debug, Clone, Copy)]
struct Rp2350Cyw43439Binding {
    bluetooth_available: bool,
    wifi_available: bool,
    bluetooth_transport: Cyw43439BluetoothTransport,
    bluetooth_target_rate: Option<u32>,
    wifi_transport: Cyw43439WlanTransport,
    wifi_target_clock_hz: Option<u32>,
    transport_topology: Cyw43439TransportTopology,
    reference_clock_hz: Option<u32>,
    sleep_clock_hz: Option<u32>,
    firmware: Cyw43439FirmwareAssets,
    clock_gpio: u8,
    chip_select_gpio: u8,
    data_irq_gpio: u8,
    power_gpio: Option<u8>,
    reset_gpio: Option<u8>,
    wake_gpio: Option<u8>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SelectedCyw43439Hardware;

struct SharedCyw43439Slot {
    init: AtomicU8,
    lock: AtomicBool,
    backend: UnsafeCell<MaybeUninit<SharedBackend>>,
}

// SAFETY: access to `backend` is serialized by `lock`, and initialization is serialized by `init`.
unsafe impl Sync for SharedCyw43439Slot {}

impl SharedCyw43439Slot {
    const fn new() -> Self {
        Self {
            init: AtomicU8::new(INIT_UNINITIALIZED),
            lock: AtomicBool::new(false),
            backend: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    fn ensure_initialized(&self) -> Result<(), Cyw43439Error> {
        loop {
            match self.init.compare_exchange(
                INIT_UNINITIALIZED,
                INIT_RUNNING,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    let backend = match build_shared_backend() {
                        Ok(backend) => backend,
                        Err(error) => {
                            self.init.store(INIT_UNINITIALIZED, Ordering::Release);
                            return Err(error);
                        }
                    };
                    // SAFETY: initialization ownership is serialized by `init`.
                    unsafe { (*self.backend.get()).write(backend) };
                    self.init.store(INIT_READY, Ordering::Release);
                    return Ok(());
                }
                Err(INIT_READY) => return Ok(()),
                Err(INIT_RUNNING) => spin_loop(),
                Err(_) => spin_loop(),
            }
        }
    }

    fn with_backend<R>(&self, f: impl FnOnce(&SharedBackend) -> R) -> Result<R, Cyw43439Error> {
        self.ensure_initialized()?;
        self.lock();
        let result = {
            // SAFETY: the backend is initialized once `ensure_initialized` returns and protected by
            // the spin mutex while borrowed.
            let backend = unsafe { (*self.backend.get()).assume_init_ref() };
            f(backend)
        };
        self.unlock();
        Ok(result)
    }

    fn with_backend_mut<R>(
        &self,
        f: impl FnOnce(&mut SharedBackend) -> R,
    ) -> Result<R, Cyw43439Error> {
        self.ensure_initialized()?;
        self.lock();
        let result = {
            // SAFETY: the backend is initialized once `ensure_initialized` returns and protected by
            // the spin mutex while mutably borrowed.
            let backend = unsafe { (*self.backend.get()).assume_init_mut() };
            f(backend)
        };
        self.unlock();
        Ok(result)
    }

    fn with_backend_result<R>(
        &self,
        f: impl FnOnce(&SharedBackend) -> Result<R, Cyw43439Error>,
    ) -> Result<R, Cyw43439Error> {
        self.ensure_initialized()?;
        self.lock();
        let result = {
            // SAFETY: the backend is initialized once `ensure_initialized` returns and protected by
            // the spin mutex while borrowed.
            let backend = unsafe { (*self.backend.get()).assume_init_ref() };
            f(backend)
        };
        self.unlock();
        result
    }

    fn with_backend_mut_result<R>(
        &self,
        f: impl FnOnce(&mut SharedBackend) -> Result<R, Cyw43439Error>,
    ) -> Result<R, Cyw43439Error> {
        self.ensure_initialized()?;
        self.lock();
        let result = {
            // SAFETY: the backend is initialized once `ensure_initialized` returns and protected by
            // the spin mutex while mutably borrowed.
            let backend = unsafe { (*self.backend.get()).assume_init_mut() };
            f(backend)
        };
        self.unlock();
        result
    }

    fn lock(&self) {
        while self
            .lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            spin_loop();
        }
    }

    fn unlock(&self) {
        self.lock.store(false, Ordering::Release);
    }
}

static CYW43439_SLOT: SharedCyw43439Slot = SharedCyw43439Slot::new();

/// Returns one shared selected CYW43439 hardware handle.
///
/// # Errors
///
/// Returns an error if the selected board does not expose the CYW43439 wiring honestly or the
/// shared reserved GPIO pins were already claimed by something else.
pub fn system_cyw43439() -> Result<SelectedCyw43439Hardware, Cyw43439Error> {
    CYW43439_SLOT.ensure_initialized()?;
    Ok(SelectedCyw43439Hardware)
}

/// Returns the selected universal Bluetooth provider over the RP2350 Pico 2 W CYW43439 wiring.
///
/// # Errors
///
/// Returns an error if the selected board does not expose the CYW43439 Bluetooth facet honestly or
/// shared combo-chip activation fails.
pub fn system_bluetooth() -> Result<Bluetooth, BluetoothError> {
    let hardware = system_cyw43439().map_err(map_cyw43439_to_bluetooth)?;
    let mut registry = DriverRegistry::<1>::new();
    let registered = registry
        .register::<Cyw43439BluetoothDriver<SelectedCyw43439Hardware>>()
        .map_err(map_driver_bluetooth)?;
    let mut driver_context = Cyw43439BluetoothDriverContext::new(hardware);
    let mut bindings = [Cyw43439BluetoothBinding {
        adapter: BluetoothAdapterId(0),
    }];

    {
        let mut discovery = DriverDiscoveryContext::new(&mut driver_context);
        let count = registered
            .enumerate_bindings(&mut discovery, &mut bindings)
            .map_err(map_driver_bluetooth)?;
        if count == 0 {
            return Err(BluetoothError::unsupported());
        }
    }

    let mut activation = DriverActivationContext::new(&mut driver_context);
    let active = registered
        .activate(&mut activation, bindings[0])
        .map_err(map_driver_bluetooth)?;
    Ok(active.into_instance())
}

/// Returns the selected universal Wi-Fi provider over the RP2350 Pico 2 W CYW43439 wiring.
///
/// # Errors
///
/// Returns an error if the selected board does not expose the CYW43439 Wi-Fi facet honestly or
/// shared combo-chip activation fails.
pub fn system_wifi() -> Result<Wifi, WifiError> {
    let hardware = system_cyw43439().map_err(map_cyw43439_to_wifi)?;
    let mut registry = DriverRegistry::<1>::new();
    let registered = registry
        .register::<Cyw43439WifiDriver<SelectedCyw43439Hardware>>()
        .map_err(map_driver_wifi)?;
    let mut driver_context = Cyw43439WifiDriverContext::new(hardware);
    let mut bindings = [Cyw43439WifiBinding {
        adapter: WifiAdapterId(0),
    }];

    {
        let mut discovery = DriverDiscoveryContext::new(&mut driver_context);
        let count = registered
            .enumerate_bindings(&mut discovery, &mut bindings)
            .map_err(map_driver_wifi)?;
        if count == 0 {
            return Err(WifiError::unsupported());
        }
    }

    let mut activation = DriverActivationContext::new(&mut driver_context);
    let active = registered
        .activate(&mut activation, bindings[0])
        .map_err(map_driver_wifi)?;
    Ok(active.into_instance())
}

impl Cyw43439HardwareContract for SelectedCyw43439Hardware {
    fn bluetooth_support(&self) -> BluetoothSupport {
        CYW43439_SLOT
            .with_backend(Cyw43439HardwareContract::bluetooth_support)
            .unwrap_or_else(|_| BluetoothSupport::unsupported())
    }

    fn bluetooth_adapters(
        &self,
    ) -> &'static [fusion_hal::contract::drivers::net::bluetooth::BluetoothAdapterDescriptor] {
        CYW43439_SLOT
            .with_backend(Cyw43439HardwareContract::bluetooth_adapters)
            .unwrap_or(&[])
    }

    fn bluetooth_transport(&self) -> Result<Cyw43439BluetoothTransport, Cyw43439Error> {
        CYW43439_SLOT.with_backend_result(Cyw43439HardwareContract::bluetooth_transport)
    }

    fn bluetooth_transport_clock_profile(
        &self,
    ) -> Result<Cyw43439BluetoothTransportClockProfile, Cyw43439Error> {
        CYW43439_SLOT.with_backend_result(|backend| {
            let host_source_clock_hz = rp2350_cyw43439_host_source_clock_hz();
            match backend.bluetooth_transport()? {
                Cyw43439BluetoothTransport::HciUartH4 | Cyw43439BluetoothTransport::HciUartH5 => {
                    Ok(Cyw43439BluetoothTransportClockProfile::HciUart {
                        target_baud: backend.bluetooth_transport_target_rate(),
                        host_source_clock_hz,
                    })
                }
                Cyw43439BluetoothTransport::BoardSharedSpiHci => {
                    Ok(Cyw43439BluetoothTransportClockProfile::BoardSharedSpiHci {
                        target_clock_hz: backend.bluetooth_transport_target_rate(),
                        host_source_clock_hz,
                    })
                }
            }
        })
    }

    fn wifi_support(&self) -> WifiSupport {
        CYW43439_SLOT
            .with_backend(Cyw43439HardwareContract::wifi_support)
            .unwrap_or_else(|_| WifiSupport::unsupported())
    }

    fn wifi_adapters(
        &self,
    ) -> &'static [fusion_hal::contract::drivers::net::wifi::WifiAdapterDescriptor] {
        CYW43439_SLOT
            .with_backend(Cyw43439HardwareContract::wifi_adapters)
            .unwrap_or(&[])
    }

    fn wifi_transport(&self) -> Result<Cyw43439WlanTransport, Cyw43439Error> {
        CYW43439_SLOT.with_backend_result(Cyw43439HardwareContract::wifi_transport)
    }

    fn wifi_transport_clock_profile(
        &self,
    ) -> Result<Cyw43439WlanTransportClockProfile, Cyw43439Error> {
        CYW43439_SLOT.with_backend_result(|backend| {
            let host_source_clock_hz = rp2350_cyw43439_host_source_clock_hz();
            match backend.wifi_transport()? {
                Cyw43439WlanTransport::Gspi => Ok(Cyw43439WlanTransportClockProfile::Gspi {
                    target_clock_hz: backend.wifi_transport_target_clock_hz(),
                    host_source_clock_hz,
                }),
                Cyw43439WlanTransport::Sdio => Ok(Cyw43439WlanTransportClockProfile::Sdio {
                    target_clock_hz: backend.wifi_transport_target_clock_hz(),
                    host_source_clock_hz,
                }),
                Cyw43439WlanTransport::BoardSharedSpi => {
                    Ok(Cyw43439WlanTransportClockProfile::BoardSharedSpi {
                        target_clock_hz: backend.wifi_transport_target_clock_hz(),
                        host_source_clock_hz,
                    })
                }
            }
        })
    }

    fn transport_topology(&self) -> Result<Cyw43439TransportTopology, Cyw43439Error> {
        CYW43439_SLOT.with_backend_result(Cyw43439HardwareContract::transport_topology)
    }

    fn controller_caps(&self, radio: Cyw43439Radio) -> Cyw43439ControllerCaps {
        CYW43439_SLOT
            .with_backend(|backend| backend.controller_caps(radio))
            .unwrap_or_else(|_| Cyw43439ControllerCaps::empty())
    }

    fn claim_controller(&mut self, radio: Cyw43439Radio) -> Result<(), Cyw43439Error> {
        CYW43439_SLOT.with_backend_mut_result(|backend| backend.claim_controller(radio))
    }

    fn release_controller(&mut self, radio: Cyw43439Radio) {
        let result = CYW43439_SLOT.with_backend_mut(|backend| backend.release_controller(radio));
        debug_assert!(
            result.is_ok(),
            "selected CYW43439 backend should remain initialized through controller release"
        );
    }

    fn facet_enabled(&self, radio: Cyw43439Radio) -> Result<bool, Cyw43439Error> {
        CYW43439_SLOT.with_backend_result(|backend| backend.facet_enabled(radio))
    }

    fn set_facet_enabled(
        &mut self,
        radio: Cyw43439Radio,
        enabled: bool,
    ) -> Result<(), Cyw43439Error> {
        CYW43439_SLOT.with_backend_mut_result(|backend| backend.set_facet_enabled(radio, enabled))
    }

    fn controller_powered(&self) -> Result<bool, Cyw43439Error> {
        CYW43439_SLOT.with_backend_result(Cyw43439HardwareContract::controller_powered)
    }

    fn set_controller_powered(&mut self, powered: bool) -> Result<(), Cyw43439Error> {
        CYW43439_SLOT.with_backend_mut_result(|backend| backend.set_controller_powered(powered))
    }

    fn set_controller_reset(&mut self, asserted: bool) -> Result<(), Cyw43439Error> {
        CYW43439_SLOT.with_backend_mut_result(|backend| backend.set_controller_reset(asserted))
    }

    fn set_controller_wake(&mut self, awake: bool) -> Result<(), Cyw43439Error> {
        CYW43439_SLOT.with_backend_mut_result(|backend| backend.set_controller_wake(awake))
    }

    fn acquire_transport(&mut self, radio: Cyw43439Radio) -> Result<(), Cyw43439Error> {
        CYW43439_SLOT.with_backend_mut_result(|backend| backend.acquire_transport(radio))
    }

    fn release_transport(&mut self, radio: Cyw43439Radio) {
        let result = CYW43439_SLOT.with_backend_mut(|backend| backend.release_transport(radio));
        debug_assert!(
            result.is_ok(),
            "selected CYW43439 backend should remain initialized through transport release"
        );
    }

    fn wait_for_controller_irq(
        &mut self,
        radio: Cyw43439Radio,
        timeout_ms: Option<u32>,
    ) -> Result<bool, Cyw43439Error> {
        CYW43439_SLOT
            .with_backend_mut_result(|backend| backend.wait_for_controller_irq(radio, timeout_ms))
    }

    fn acknowledge_controller_irq(&mut self, radio: Cyw43439Radio) -> Result<(), Cyw43439Error> {
        CYW43439_SLOT.with_backend_mut_result(|backend| backend.acknowledge_controller_irq(radio))
    }

    fn write_controller_transport(
        &mut self,
        radio: Cyw43439Radio,
        payload: &[u8],
    ) -> Result<(), Cyw43439Error> {
        CYW43439_SLOT
            .with_backend_mut_result(|backend| backend.write_controller_transport(radio, payload))
    }

    fn read_controller_transport(
        &mut self,
        radio: Cyw43439Radio,
        out: &mut [u8],
    ) -> Result<usize, Cyw43439Error> {
        CYW43439_SLOT
            .with_backend_mut_result(|backend| backend.read_controller_transport(radio, out))
    }

    fn firmware_image(&self, radio: Cyw43439Radio) -> Result<Option<&'static [u8]>, Cyw43439Error> {
        CYW43439_SLOT.with_backend_result(|backend| backend.firmware_image(radio))
    }

    fn nvram_image(&self, radio: Cyw43439Radio) -> Result<Option<&'static [u8]>, Cyw43439Error> {
        CYW43439_SLOT.with_backend_result(|backend| backend.nvram_image(radio))
    }

    fn clm_image(&self, radio: Cyw43439Radio) -> Result<Option<&'static [u8]>, Cyw43439Error> {
        CYW43439_SLOT.with_backend_result(|backend| backend.clm_image(radio))
    }

    fn reference_clock_hz(&self) -> Result<Option<u32>, Cyw43439Error> {
        CYW43439_SLOT.with_backend_result(Cyw43439HardwareContract::reference_clock_hz)
    }

    fn sleep_clock_hz(&self) -> Result<Option<u32>, Cyw43439Error> {
        CYW43439_SLOT.with_backend_result(Cyw43439HardwareContract::sleep_clock_hz)
    }

    fn delay_ms(&self, milliseconds: u32) {
        let _ = CYW43439_SLOT.with_backend(|backend| backend.delay_ms(milliseconds));
    }
}

fn build_shared_backend() -> Result<SharedBackend, Cyw43439Error> {
    let binding = cyw43439_binding()?;
    let clock =
        HalGpioPin::from_inner(claim_board_owned_pin(binding.clock_gpio).map_err(map_gpio_error)?);
    let chip_select = HalGpioPin::from_inner(
        claim_board_owned_pin(binding.chip_select_gpio).map_err(map_gpio_error)?,
    );
    let data_irq = HalGpioPin::from_inner(
        claim_board_owned_pin(binding.data_irq_gpio).map_err(map_gpio_error)?,
    );
    let power = claim_optional_pin(binding.power_gpio)?;
    let reset = claim_optional_pin(binding.reset_gpio)?;
    let wake = claim_optional_pin(binding.wake_gpio)?;

    Ok(SharedBackend::new(
        clock,
        chip_select,
        data_irq,
        power,
        reset,
        wake,
        binding.bluetooth_transport,
        binding.bluetooth_target_rate,
        binding.wifi_transport,
        binding.wifi_target_clock_hz,
        binding.transport_topology,
        None,
        binding.reference_clock_hz,
        binding.sleep_clock_hz,
        rp2350_delay_ms,
        binding.firmware,
        binding.bluetooth_available,
        binding.wifi_available,
    ))
}

fn claim_optional_pin(
    pin: Option<u8>,
) -> Result<Option<HalGpioPin<GpioPinHardware>>, Cyw43439Error> {
    pin.map(|pin| claim_board_owned_pin(pin).map(HalGpioPin::from_inner))
        .transpose()
        .map_err(map_gpio_error)
}

fn cyw43439_binding() -> Result<Rp2350Cyw43439Binding, Cyw43439Error> {
    let bluetooth = bluetooth_controllers()
        .iter()
        .copied()
        .find(|binding| binding.chip == "CYW43439");
    let wifi = wifi_controllers()
        .iter()
        .copied()
        .find(|binding| binding.chip == "CYW43439");

    let bluetooth_available = bluetooth.is_some();
    let wifi_available = wifi.is_some();
    if !bluetooth_available && !wifi_available {
        return Err(Cyw43439Error::unsupported());
    }

    let bluetooth_transport = bluetooth
        .map(bluetooth_transport_kind)
        .transpose()?
        .unwrap_or(Cyw43439BluetoothTransport::BoardSharedSpiHci);
    let bluetooth_target_rate = bluetooth.and_then(bluetooth_transport_target_rate);
    let wifi_transport = wifi
        .map(wifi_transport_kind)
        .transpose()?
        .unwrap_or(Cyw43439WlanTransport::BoardSharedSpi);
    let wifi_target_clock_hz = wifi.and_then(wifi_transport_target_clock_hz);
    let transport = match (
        bluetooth.and_then(bluetooth_transport_pins),
        wifi.and_then(wifi_transport_pins),
    ) {
        (Some(transport), Some(other)) if transport == other => transport,
        (Some(_), Some(_)) => return Err(Cyw43439Error::unsupported()),
        (Some(transport), None) => transport,
        (None, Some(transport)) => transport,
        (None, None) => return Err(Cyw43439Error::unsupported()),
    };

    let power_gpio = merge_optional_pin(
        bluetooth.and_then(|binding| binding.power_gpio),
        wifi.and_then(|binding| binding.power_gpio),
    )?;
    let reset_gpio = merge_optional_pin(
        bluetooth.and_then(|binding| binding.reset_gpio),
        wifi.and_then(|binding| binding.reset_gpio),
    )?;
    let wake_gpio = merge_optional_pin(
        bluetooth.and_then(|binding| binding.wake_gpio),
        wifi.and_then(|binding| binding.wake_gpio),
    )?;
    let reference_clock_hz = merge_optional_u32(
        bluetooth.and_then(|binding| binding.clock.reference_clock_hz),
        wifi.and_then(|binding| binding.clock.reference_clock_hz),
    )?;
    let sleep_clock_hz = merge_optional_u32(
        bluetooth.and_then(|binding| binding.clock.sleep_clock_hz),
        wifi.and_then(|binding| binding.clock.sleep_clock_hz),
    )?;
    let transport_topology = match (bluetooth_transport, wifi_transport) {
        (Cyw43439BluetoothTransport::BoardSharedSpiHci, Cyw43439WlanTransport::BoardSharedSpi) => {
            Cyw43439TransportTopology::SharedBoardTransport
        }
        _ => Cyw43439TransportTopology::SplitHostTransports,
    };

    Ok(Rp2350Cyw43439Binding {
        bluetooth_available,
        wifi_available,
        bluetooth_transport,
        bluetooth_target_rate,
        wifi_transport,
        wifi_target_clock_hz,
        transport_topology,
        reference_clock_hz,
        sleep_clock_hz,
        firmware: rp2350_pico2w_firmware_assets(bluetooth_available, wifi_available),
        clock_gpio: transport.0,
        chip_select_gpio: transport.1,
        data_irq_gpio: transport.2,
        power_gpio,
        reset_gpio,
        wake_gpio,
    })
}

fn rp2350_pico2w_firmware_assets(
    bluetooth_available: bool,
    wifi_available: bool,
) -> Cyw43439FirmwareAssets {
    let packed_wifi = if wifi_available {
        Some(if bluetooth_available {
            Cyw43439PackedWlanFirmwareImage {
                image: PICO2W_CYW43439_WIFI_BT_COMBINED_FW,
                firmware_len: PICO2W_CYW43439_WIFI_BT_FW_LEN,
                clm_len: PICO2W_CYW43439_WIFI_BT_CLM_LEN,
            }
        } else {
            Cyw43439PackedWlanFirmwareImage {
                image: PICO2W_CYW43439_WIFI_ONLY_COMBINED_FW,
                firmware_len: PICO2W_CYW43439_WIFI_ONLY_FW_LEN,
                clm_len: PICO2W_CYW43439_WIFI_ONLY_CLM_LEN,
            }
        })
    } else {
        None
    };

    Cyw43439FirmwareAssets {
        bluetooth: fd_net_chipset_infineon_cyw43439::firmware::Cyw43439BluetoothFirmwareAssets {
            patch_image: bluetooth_available.then_some(PICO2W_CYW43439_BT_PATCH),
        },
        wifi: fd_net_chipset_infineon_cyw43439::firmware::Cyw43439WlanFirmwareAssets {
            firmware_image: packed_wifi.and_then(Cyw43439PackedWlanFirmwareImage::firmware_image),
            nvram_image: wifi_available.then_some(PICO2W_CYW43439_WIFI_NVRAM),
            clm_image: packed_wifi.and_then(Cyw43439PackedWlanFirmwareImage::clm_image),
        },
    }
}

fn bluetooth_transport_pins(binding: CortexMBluetoothControllerBinding) -> Option<(u8, u8, u8)> {
    match binding.transport {
        CortexMBluetoothTransportBinding::Spi3WireSharedDataIrq {
            clock_gpio,
            chip_select_gpio,
            data_irq_gpio,
            ..
        } => Some((clock_gpio, chip_select_gpio, data_irq_gpio)),
        _ => None,
    }
}

fn wifi_transport_pins(binding: CortexMWifiControllerBinding) -> Option<(u8, u8, u8)> {
    match binding.transport {
        CortexMWifiTransportBinding::Spi3WireSharedDataIrq {
            clock_gpio,
            chip_select_gpio,
            data_irq_gpio,
            ..
        } => Some((clock_gpio, chip_select_gpio, data_irq_gpio)),
        _ => None,
    }
}

fn bluetooth_transport_kind(
    binding: CortexMBluetoothControllerBinding,
) -> Result<Cyw43439BluetoothTransport, Cyw43439Error> {
    match binding.transport {
        CortexMBluetoothTransportBinding::Uart {
            cts_gpio, rts_gpio, ..
        } => {
            if cts_gpio.is_some() && rts_gpio.is_some() {
                Ok(Cyw43439BluetoothTransport::HciUartH4)
            } else {
                Ok(Cyw43439BluetoothTransport::HciUartH5)
            }
        }
        CortexMBluetoothTransportBinding::Spi3WireSharedDataIrq { .. }
        | CortexMBluetoothTransportBinding::Spi4Wire { .. } => {
            Ok(Cyw43439BluetoothTransport::BoardSharedSpiHci)
        }
    }
}

fn bluetooth_transport_target_rate(binding: CortexMBluetoothControllerBinding) -> Option<u32> {
    match binding.transport {
        CortexMBluetoothTransportBinding::Spi3WireSharedDataIrq {
            target_clock_hz, ..
        }
        | CortexMBluetoothTransportBinding::Spi4Wire {
            target_clock_hz, ..
        } => target_clock_hz,
        CortexMBluetoothTransportBinding::Uart { target_baud, .. } => target_baud,
    }
}

fn wifi_transport_kind(
    binding: CortexMWifiControllerBinding,
) -> Result<Cyw43439WlanTransport, Cyw43439Error> {
    match binding.transport {
        CortexMWifiTransportBinding::Sdio { .. } => Ok(Cyw43439WlanTransport::Sdio),
        CortexMWifiTransportBinding::Spi4Wire { .. } => Ok(Cyw43439WlanTransport::Gspi),
        CortexMWifiTransportBinding::Spi3WireSharedDataIrq { .. } => {
            Ok(Cyw43439WlanTransport::BoardSharedSpi)
        }
    }
}

fn wifi_transport_target_clock_hz(binding: CortexMWifiControllerBinding) -> Option<u32> {
    match binding.transport {
        CortexMWifiTransportBinding::Spi3WireSharedDataIrq {
            target_clock_hz, ..
        }
        | CortexMWifiTransportBinding::Spi4Wire {
            target_clock_hz, ..
        }
        | CortexMWifiTransportBinding::Sdio {
            target_clock_hz, ..
        } => target_clock_hz,
    }
}

fn merge_optional_pin(left: Option<u8>, right: Option<u8>) -> Result<Option<u8>, Cyw43439Error> {
    match (left, right) {
        (Some(left), Some(right)) if left != right => Err(Cyw43439Error::unsupported()),
        (Some(pin), _) | (_, Some(pin)) => Ok(Some(pin)),
        (None, None) => Ok(None),
    }
}

fn merge_optional_u32(left: Option<u32>, right: Option<u32>) -> Result<Option<u32>, Cyw43439Error> {
    match (left, right) {
        (Some(left), Some(right)) if left != right => Err(Cyw43439Error::unsupported()),
        (Some(value), _) | (_, Some(value)) => Ok(Some(value)),
        (None, None) => Ok(None),
    }
}

fn rp2350_delay_ms(milliseconds: u32) {
    if milliseconds == 0 {
        return;
    }

    let Ok(start) = monotonic_raw_now() else {
        return;
    };
    let Some(ticks_per_second) = monotonic_tick_hz() else {
        return;
    };
    let delay_ticks = (u64::from(milliseconds).saturating_mul(ticks_per_second)) / 1_000;
    let deadline = start.saturating_add(delay_ticks.max(1));

    loop {
        let Ok(now) = monotonic_raw_now() else {
            break;
        };
        if now >= deadline {
            break;
        }
        spin_loop();
    }
}

fn rp2350_cyw43439_host_source_clock_hz() -> Option<u64> {
    current_sys_clock_hz()
}

fn map_gpio_error(error: GpioError) -> Cyw43439Error {
    match error.kind() {
        GpioErrorKind::Unsupported => Cyw43439Error::unsupported(),
        GpioErrorKind::Invalid => Cyw43439Error::invalid(),
        GpioErrorKind::Busy => Cyw43439Error::busy(),
        GpioErrorKind::ResourceExhausted => Cyw43439Error::resource_exhausted(),
        GpioErrorKind::StateConflict => Cyw43439Error::state_conflict(),
        GpioErrorKind::Platform(code) => Cyw43439Error::platform(code),
    }
}

fn map_cyw43439_to_bluetooth(error: Cyw43439Error) -> BluetoothError {
    match error.kind() {
        Cyw43439ErrorKind::Unsupported => BluetoothError::unsupported(),
        Cyw43439ErrorKind::Invalid => BluetoothError::invalid(),
        Cyw43439ErrorKind::Busy => BluetoothError::busy(),
        Cyw43439ErrorKind::ResourceExhausted => BluetoothError::resource_exhausted(),
        Cyw43439ErrorKind::StateConflict => BluetoothError::state_conflict(),
        Cyw43439ErrorKind::Platform(code) => BluetoothError::platform(code),
    }
}

fn map_cyw43439_to_wifi(error: Cyw43439Error) -> WifiError {
    match error.kind() {
        Cyw43439ErrorKind::Unsupported => WifiError::unsupported(),
        Cyw43439ErrorKind::Invalid => WifiError::invalid(),
        Cyw43439ErrorKind::Busy => WifiError::busy(),
        Cyw43439ErrorKind::ResourceExhausted => WifiError::resource_exhausted(),
        Cyw43439ErrorKind::StateConflict => WifiError::state_conflict(),
        Cyw43439ErrorKind::Platform(code) => WifiError::platform(code),
    }
}

fn map_driver_bluetooth(error: DriverError) -> BluetoothError {
    match error.kind() {
        DriverErrorKind::Unsupported => BluetoothError::unsupported(),
        DriverErrorKind::Invalid => BluetoothError::invalid(),
        DriverErrorKind::Busy => BluetoothError::busy(),
        DriverErrorKind::ResourceExhausted => BluetoothError::resource_exhausted(),
        DriverErrorKind::StateConflict => BluetoothError::state_conflict(),
        DriverErrorKind::MissingContext | DriverErrorKind::WrongContextType => {
            BluetoothError::state_conflict()
        }
        DriverErrorKind::AlreadyRegistered => BluetoothError::state_conflict(),
        DriverErrorKind::Platform(code) => BluetoothError::platform(code),
    }
}

fn map_driver_wifi(error: DriverError) -> WifiError {
    match error.kind() {
        DriverErrorKind::Unsupported => WifiError::unsupported(),
        DriverErrorKind::Invalid => WifiError::invalid(),
        DriverErrorKind::Busy => WifiError::busy(),
        DriverErrorKind::ResourceExhausted => WifiError::resource_exhausted(),
        DriverErrorKind::StateConflict => WifiError::state_conflict(),
        DriverErrorKind::MissingContext | DriverErrorKind::WrongContextType => {
            WifiError::state_conflict()
        }
        DriverErrorKind::AlreadyRegistered => WifiError::state_conflict(),
        DriverErrorKind::Platform(code) => WifiError::platform(code),
    }
}
