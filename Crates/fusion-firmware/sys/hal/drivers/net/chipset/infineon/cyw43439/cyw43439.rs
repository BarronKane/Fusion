//! Firmware-orchestrated RP2350/Pico 2 W CYW43439 driver binding.
//!
//! This lives in `fusion-firmware`, not `fusion-pal`, because:
//! - PAL owns the CYW43439 hardware substrate truth
//! - firmware owns module selection, crawlability, and driver binding policy
//! - the backend is RP2350/Pico 2 W specific, but the activated drivers are still selected by
//!   canonical driver keys surfaced through FDXE metadata

use fd_net_chipset_infineon_cyw43439::{
    bluetooth::{
        Cyw43439Binding as Cyw43439BluetoothBinding,
        Cyw43439Driver as Cyw43439BluetoothDriver,
        Cyw43439DriverContext as Cyw43439BluetoothDriverContext,
        driver_metadata as cyw43439_bluetooth_driver_metadata,
    },
    interface::contract::{
        Cyw43439Error,
        Cyw43439ErrorKind,
    },
    wifi::{
        Cyw43439Binding as Cyw43439WifiBinding,
        Cyw43439Driver as Cyw43439WifiDriver,
        Cyw43439DriverContext as Cyw43439WifiDriverContext,
        driver_metadata as cyw43439_wifi_driver_metadata,
    },
};
use fusion_hal::contract::drivers::driver::{
    DriverActivationContext,
    DriverDiscoveryContext,
    DriverError,
    DriverErrorKind,
    DriverRegistry,
};
use fusion_hal::contract::drivers::net::bluetooth::{
    BluetoothControlContract,
    BluetoothError,
};
use fusion_hal::contract::drivers::net::wifi::{
    WifiControlContract,
    WifiError,
};
use fusion_pal::sys::soc::drivers::net::chipset::infineon::cyw43439::{
    SelectedCyw43439Hardware,
    system_cyw43439,
};

use crate::module::{
    StackBluetoothAdapter,
    StackDriverSlot,
    StackWifiAdapter,
    bind_bluetooth_adapter,
    bind_wifi_adapter,
    requested_driver_by_key,
};

const CYW43439_BLUETOOTH_DRIVER_KEY: &str = "net.bluetooth.infineon.cyw43439";
const CYW43439_WIFI_DRIVER_KEY: &str = "net.wifi.infineon.cyw43439";

/// Binds the selected CYW43439 Bluetooth driver into caller-owned stack storage and returns the
/// canonical public Bluetooth adapter contract surface.
///
/// # Errors
///
/// Returns an error if the CYW43439 driver module was not selected into this firmware image, the
/// RP2350 board cannot surface the shared CYW43439 hardware honestly, or driver activation fails.
pub fn system_bluetooth<'a>(
    slot: StackDriverSlot<'a>,
) -> Result<StackBluetoothAdapter<'a>, BluetoothError> {
    ensure_driver_selected(CYW43439_BLUETOOTH_DRIVER_KEY).map_err(map_driver_bluetooth)?;

    let hardware = system_cyw43439().map_err(map_cyw43439_to_bluetooth)?;
    let mut registry = DriverRegistry::<1>::new();
    let registered = registry
        .register::<Cyw43439BluetoothDriver<SelectedCyw43439Hardware>>()
        .map_err(map_driver_bluetooth)?;
    let mut driver_context = Cyw43439BluetoothDriverContext::new(hardware);
    let mut bindings = [Cyw43439BluetoothBinding {
        adapter: fusion_hal::contract::drivers::net::bluetooth::BluetoothAdapterId(0),
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
    let mut provider = registered
        .activate(&mut activation, bindings[0])
        .map_err(map_driver_bluetooth)?
        .into_instance();
    let adapter = provider.open_adapter(bindings[0].adapter)?;

    bind_bluetooth_adapter(slot, cyw43439_bluetooth_driver_metadata(), adapter)
        .map_err(map_driver_bluetooth)
}

/// Binds the selected CYW43439 Wi-Fi driver into caller-owned stack storage and returns the
/// canonical public Wi-Fi adapter contract surface.
///
/// # Errors
///
/// Returns an error if the CYW43439 driver module was not selected into this firmware image, the
/// RP2350 board cannot surface the shared CYW43439 hardware honestly, or driver activation fails.
pub fn system_wifi<'a>(slot: StackDriverSlot<'a>) -> Result<StackWifiAdapter<'a>, WifiError> {
    ensure_driver_selected(CYW43439_WIFI_DRIVER_KEY).map_err(map_driver_wifi)?;

    let hardware = system_cyw43439().map_err(map_cyw43439_to_wifi)?;
    let mut registry = DriverRegistry::<1>::new();
    let registered = registry
        .register::<Cyw43439WifiDriver<SelectedCyw43439Hardware>>()
        .map_err(map_driver_wifi)?;
    let mut driver_context = Cyw43439WifiDriverContext::new(hardware);
    let mut bindings = [Cyw43439WifiBinding {
        adapter: fusion_hal::contract::drivers::net::wifi::WifiAdapterId(0),
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
    let mut provider = registered
        .activate(&mut activation, bindings[0])
        .map_err(map_driver_wifi)?
        .into_instance();
    let adapter = provider.open_adapter(bindings[0].adapter)?;

    bind_wifi_adapter(slot, cyw43439_wifi_driver_metadata(), adapter).map_err(map_driver_wifi)
}

fn ensure_driver_selected(key: &str) -> Result<(), DriverError> {
    requested_driver_by_key(key).map(|_| ())
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
