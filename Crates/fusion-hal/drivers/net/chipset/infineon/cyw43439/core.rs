//! Internal shared CYW43439 chipset runtime helpers.

use fusion_hal::contract::drivers::net::bluetooth::{
    BluetoothAdapterDescriptor,
    BluetoothError,
    BluetoothSupport,
};
use fusion_hal::contract::drivers::net::wifi::{
    WifiAdapterDescriptor,
    WifiError,
    WifiSupport,
};
use crate::firmware::{
    Cyw43439BluetoothFirmwareAssets,
    Cyw43439WlanFirmwareAssets,
};
use crate::boot::Cyw43439Bootstrap;
use crate::interface::{
    backend::UnsupportedBackend,
    contract::{
        Cyw43439Error,
        Cyw43439ErrorKind,
        Cyw43439HardwareContract,
        Cyw43439Radio,
    },
};
use crate::transport::{
    Cyw43439BluetoothTransport,
    Cyw43439BluetoothTransportClockProfile,
    Cyw43439TransportTopology,
    Cyw43439WlanTransport,
    Cyw43439WlanTransportClockProfile,
};

/// Coarse shared-chip runtime state for one active CYW43439 chipset instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Cyw43439ChipState {
    Cold,
    Powered,
    Clocked,
    FirmwareLoaded,
    Ready,
    LowPower,
}

/// The host-side transport profile currently surfaced by one CYW43439 chipset binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Cyw43439TransportProfile {
    pub bluetooth: Option<Cyw43439BluetoothTransport>,
    pub bluetooth_clock: Option<Cyw43439BluetoothTransportClockProfile>,
    pub wifi: Option<Cyw43439WlanTransport>,
    pub wifi_clock: Option<Cyw43439WlanTransportClockProfile>,
    pub topology: Cyw43439TransportTopology,
}

/// Clock truth currently surfaced by one CYW43439 chipset binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Cyw43439ClockProfile {
    pub reference_clock_hz: Option<u32>,
    pub sleep_clock_hz: Option<u32>,
}

/// Boot-readiness truth currently surfaced by one CYW43439 chipset binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Cyw43439BootReadiness {
    pub state: Cyw43439ChipState,
    pub transport: Cyw43439TransportProfile,
    pub clocks: Cyw43439ClockProfile,
    pub bluetooth_patch_available: bool,
    pub wifi_firmware_available: bool,
    pub wifi_nvram_available: bool,
    pub wifi_clm_available: bool,
    pub can_boot_bluetooth: bool,
    pub can_boot_wifi: bool,
}

/// Shared internal CYW43439 chipset wrapper used by the Bluetooth and Wi-Fi driver facets.
#[derive(Debug)]
pub(crate) struct Cyw43439Chipset<H: Cyw43439HardwareContract = UnsupportedBackend> {
    pub(crate) hardware: H,
    state: Cyw43439ChipState,
    activity_depth: u32,
}

impl<H> Cyw43439Chipset<H>
where
    H: Cyw43439HardwareContract,
{
    #[must_use]
    pub(crate) fn new(hardware: H) -> Self {
        let state = match hardware.controller_powered() {
            Ok(true) => Cyw43439ChipState::Powered,
            _ => Cyw43439ChipState::Cold,
        };
        Self {
            hardware,
            state,
            activity_depth: 0,
        }
    }

    #[allow(dead_code)]
    #[must_use]
    pub(crate) fn state(&self) -> Cyw43439ChipState {
        self.state
    }

    #[allow(dead_code)]
    pub(crate) fn transport_profile(&self) -> Result<Cyw43439TransportProfile, Cyw43439Error> {
        Ok(Cyw43439TransportProfile {
            bluetooth: optional_transport_profile(self.hardware.bluetooth_transport())?,
            bluetooth_clock: optional_transport_profile(
                self.hardware.bluetooth_transport_clock_profile(),
            )?,
            wifi: optional_transport_profile(self.hardware.wifi_transport())?,
            wifi_clock: optional_transport_profile(self.hardware.wifi_transport_clock_profile())?,
            topology: self.hardware.transport_topology()?,
        })
    }

    #[allow(dead_code)]
    pub(crate) fn clock_profile(&self) -> Result<Cyw43439ClockProfile, Cyw43439Error> {
        Ok(Cyw43439ClockProfile {
            reference_clock_hz: self.hardware.reference_clock_hz()?,
            sleep_clock_hz: self.hardware.sleep_clock_hz()?,
        })
    }

    #[allow(dead_code)]
    pub(crate) fn bluetooth_firmware_assets(
        &self,
    ) -> Result<Cyw43439BluetoothFirmwareAssets, Cyw43439Error> {
        Ok(Cyw43439BluetoothFirmwareAssets {
            patch_image: self.hardware.firmware_image(Cyw43439Radio::Bluetooth)?,
        })
    }

    #[allow(dead_code)]
    pub(crate) fn wifi_firmware_assets(&self) -> Result<Cyw43439WlanFirmwareAssets, Cyw43439Error> {
        Ok(Cyw43439WlanFirmwareAssets {
            firmware_image: self.hardware.firmware_image(Cyw43439Radio::Wifi)?,
            nvram_image: self.hardware.nvram_image(Cyw43439Radio::Wifi)?,
            clm_image: self.hardware.clm_image(Cyw43439Radio::Wifi)?,
        })
    }

    #[allow(dead_code)]
    pub(crate) fn boot_readiness(&self) -> Result<Cyw43439BootReadiness, Cyw43439Error> {
        let transport = self.transport_profile()?;
        let clocks = self.clock_profile()?;
        let bluetooth_assets = self.bluetooth_firmware_assets()?;
        let wifi_assets = self.wifi_firmware_assets()?;
        let bluetooth_patch_available = bluetooth_assets.patch_image.is_some();
        let wifi_firmware_available = wifi_assets.firmware_image.is_some();
        let wifi_nvram_available = wifi_assets.nvram_image.is_some();
        let wifi_clm_available = wifi_assets.clm_image.is_some();
        let reference_clock_available = clocks.reference_clock_hz.is_some();

        Ok(Cyw43439BootReadiness {
            state: self.state,
            transport,
            clocks,
            bluetooth_patch_available,
            wifi_firmware_available,
            wifi_nvram_available,
            wifi_clm_available,
            can_boot_bluetooth: reference_clock_available,
            can_boot_wifi: reference_clock_available
                && wifi_firmware_available
                && wifi_nvram_available,
        })
    }

    fn refresh_power_state_from_hardware(&mut self) {
        self.state = match self.hardware.controller_powered() {
            Ok(true) => match self.state {
                Cyw43439ChipState::Clocked
                | Cyw43439ChipState::FirmwareLoaded
                | Cyw43439ChipState::Ready
                | Cyw43439ChipState::LowPower => self.state,
                _ => Cyw43439ChipState::Powered,
            },
            _ => Cyw43439ChipState::Cold,
        };
    }

    #[allow(dead_code)]
    pub(crate) fn mark_clocked(&mut self) {
        self.state = Cyw43439ChipState::Clocked;
    }

    #[allow(dead_code)]
    pub(crate) fn mark_firmware_loaded(&mut self) {
        self.state = Cyw43439ChipState::FirmwareLoaded;
    }

    #[allow(dead_code)]
    pub(crate) fn mark_ready(&mut self) {
        self.state = Cyw43439ChipState::Ready;
    }

    #[allow(dead_code)]
    pub(crate) fn mark_low_power(&mut self) {
        self.state = Cyw43439ChipState::LowPower;
    }

    pub(crate) fn begin_driver_activity(&mut self) {
        self.activity_depth = self.activity_depth.saturating_add(1);
        let _ = self.hardware.set_driver_activity_indicator(true);
    }

    pub(crate) fn end_driver_activity(&mut self) {
        self.activity_depth = self.activity_depth.saturating_sub(1);
        let _ = self
            .hardware
            .set_driver_activity_indicator(self.activity_depth != 0);
    }

    pub(crate) fn sync_driver_activity_indicator(&mut self) {
        let _ = self
            .hardware
            .set_driver_activity_indicator(self.activity_depth != 0);
    }

    pub(crate) fn with_driver_activity<T, E>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, E>,
    ) -> Result<T, E> {
        self.begin_driver_activity();
        let result = f(self);
        self.end_driver_activity();
        result
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

    pub(crate) fn claim_bluetooth(&mut self) -> Result<(), BluetoothError> {
        self.hardware
            .claim_controller(Cyw43439Radio::Bluetooth)
            .map_err(map_bluetooth_error)
    }

    pub(crate) fn release_bluetooth(&mut self) {
        let _ = self
            .hardware
            .set_facet_enabled(Cyw43439Radio::Bluetooth, false);
        self.hardware.release_controller(Cyw43439Radio::Bluetooth);
        self.refresh_power_state_from_hardware();
    }

    pub(crate) fn claim_wifi(&mut self) -> Result<(), WifiError> {
        self.hardware
            .claim_controller(Cyw43439Radio::Wifi)
            .map_err(map_wifi_error)
    }

    pub(crate) fn release_wifi(&mut self) {
        self.hardware
            .set_facet_enabled(Cyw43439Radio::Wifi, false)
            .ok();
        self.hardware.release_controller(Cyw43439Radio::Wifi);
        self.refresh_power_state_from_hardware();
    }

    pub(crate) fn bluetooth_enabled(&self) -> Result<bool, BluetoothError> {
        self.hardware
            .facet_enabled(Cyw43439Radio::Bluetooth)
            .map_err(map_bluetooth_error)
    }

    pub(crate) fn set_bluetooth_enabled(&mut self, enabled: bool) -> Result<(), BluetoothError> {
        self.with_driver_activity(|this| {
            this.hardware
                .set_facet_enabled(Cyw43439Radio::Bluetooth, enabled)
                .map_err(map_bluetooth_error)?;
            if enabled
                && matches!(
                    this.transport_profile()
                        .map_err(map_bluetooth_error)?
                        .bluetooth,
                    Some(Cyw43439BluetoothTransport::BoardSharedSpiHci)
                )
            {
                let runtime_ready = match Cyw43439Bootstrap::ensure_wlan_runtime_ready(this) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == Cyw43439ErrorKind::StateConflict => {
                        this.hardware
                            .claim_controller(Cyw43439Radio::Wifi)
                            .map_err(map_bluetooth_error)?;
                        let retry = Cyw43439Bootstrap::ensure_wlan_runtime_ready(this);
                        this.hardware.release_controller(Cyw43439Radio::Wifi);
                        retry
                    }
                    Err(error) => Err(error),
                };
                if let Err(error) = runtime_ready {
                    let _ = this
                        .hardware
                        .set_facet_enabled(Cyw43439Radio::Bluetooth, false);
                    this.refresh_power_state_from_hardware();
                    return Err(map_bluetooth_error(error));
                }
            }
            this.refresh_power_state_from_hardware();
            Ok(())
        })
    }

    pub(crate) fn wifi_enabled(&self) -> Result<bool, WifiError> {
        self.hardware
            .facet_enabled(Cyw43439Radio::Wifi)
            .map_err(map_wifi_error)
    }

    pub(crate) fn set_wifi_enabled(&mut self, enabled: bool) -> Result<(), WifiError> {
        self.with_driver_activity(|this| {
            this.hardware
                .set_facet_enabled(Cyw43439Radio::Wifi, enabled)
                .map_err(map_wifi_error)?;
            if enabled {
                if let Err(error) = Cyw43439Bootstrap::ensure_wlan_runtime_ready(this) {
                    let _ = this.hardware.set_facet_enabled(Cyw43439Radio::Wifi, false);
                    this.refresh_power_state_from_hardware();
                    return Err(map_wifi_error(error));
                }
            }
            this.refresh_power_state_from_hardware();
            Ok(())
        })
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

fn optional_transport_profile<T>(
    result: Result<T, Cyw43439Error>,
) -> Result<Option<T>, Cyw43439Error> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if matches!(error.kind(), Cyw43439ErrorKind::Unsupported) => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Cyw43439BootReadiness,
        Cyw43439ChipState,
        Cyw43439Chipset,
        Cyw43439ClockProfile,
    };
    use fusion_hal::contract::drivers::net::bluetooth::{
        BluetoothAdapterDescriptor,
        BluetoothSupport,
    };
    use fusion_hal::contract::drivers::net::wifi::{
        WifiAdapterDescriptor,
        WifiSupport,
    };
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

    #[derive(Debug)]
    struct FakeHardware {
        powered: bool,
        bluetooth_enabled: bool,
        wifi_enabled: bool,
        bluetooth_patch: Option<&'static [u8]>,
        wifi_firmware: Option<&'static [u8]>,
        wifi_nvram: Option<&'static [u8]>,
        wifi_clm: Option<&'static [u8]>,
        bluetooth_transport: Cyw43439BluetoothTransport,
        wifi_transport: Cyw43439WlanTransport,
        topology: Cyw43439TransportTopology,
        reference_clock_hz: Option<u32>,
        sleep_clock_hz: Option<u32>,
    }

    impl FakeHardware {
        fn new() -> Self {
            Self {
                powered: false,
                bluetooth_enabled: false,
                wifi_enabled: false,
                bluetooth_patch: Some(b"bt-patch"),
                wifi_firmware: Some(b"wifi-fw"),
                wifi_nvram: Some(b"wifi-nvram"),
                wifi_clm: Some(b"wifi-clm"),
                bluetooth_transport: Cyw43439BluetoothTransport::HciUartH4,
                wifi_transport: Cyw43439WlanTransport::Gspi,
                topology: Cyw43439TransportTopology::SplitHostTransports,
                reference_clock_hz: Some(37_400_000),
                sleep_clock_hz: None,
            }
        }
    }

    impl Cyw43439HardwareContract for FakeHardware {
        fn bluetooth_support(&self) -> BluetoothSupport {
            BluetoothSupport::unsupported()
        }

        fn bluetooth_adapters(&self) -> &'static [BluetoothAdapterDescriptor] {
            &[]
        }

        fn bluetooth_transport(&self) -> Result<Cyw43439BluetoothTransport, Cyw43439Error> {
            Ok(self.bluetooth_transport)
        }

        fn bluetooth_transport_clock_profile(
            &self,
        ) -> Result<Cyw43439BluetoothTransportClockProfile, Cyw43439Error> {
            Ok(match self.bluetooth_transport {
                Cyw43439BluetoothTransport::HciUartH4 | Cyw43439BluetoothTransport::HciUartH5 => {
                    Cyw43439BluetoothTransportClockProfile::HciUart {
                        target_baud: Some(3_000_000),
                        host_source_clock_hz: Some(150_000_000),
                    }
                }
                Cyw43439BluetoothTransport::BoardSharedSpiHci => {
                    Cyw43439BluetoothTransportClockProfile::BoardSharedSpiHci {
                        target_clock_hz: Some(31_250_000),
                        host_source_clock_hz: Some(150_000_000),
                    }
                }
            })
        }

        fn wifi_support(&self) -> WifiSupport {
            WifiSupport::unsupported()
        }

        fn wifi_adapters(&self) -> &'static [WifiAdapterDescriptor] {
            &[]
        }

        fn wifi_transport(&self) -> Result<Cyw43439WlanTransport, Cyw43439Error> {
            Ok(self.wifi_transport)
        }

        fn wifi_transport_clock_profile(
            &self,
        ) -> Result<Cyw43439WlanTransportClockProfile, Cyw43439Error> {
            Ok(match self.wifi_transport {
                Cyw43439WlanTransport::Gspi => Cyw43439WlanTransportClockProfile::Gspi {
                    target_clock_hz: Some(31_250_000),
                    host_source_clock_hz: Some(150_000_000),
                },
                Cyw43439WlanTransport::Sdio => Cyw43439WlanTransportClockProfile::Sdio {
                    target_clock_hz: Some(25_000_000),
                    host_source_clock_hz: Some(150_000_000),
                },
                Cyw43439WlanTransport::BoardSharedSpi => {
                    Cyw43439WlanTransportClockProfile::BoardSharedSpi {
                        target_clock_hz: Some(31_250_000),
                        host_source_clock_hz: Some(150_000_000),
                    }
                }
            })
        }

        fn transport_topology(&self) -> Result<Cyw43439TransportTopology, Cyw43439Error> {
            Ok(self.topology)
        }

        fn controller_caps(&self, _radio: Cyw43439Radio) -> Cyw43439ControllerCaps {
            Cyw43439ControllerCaps::empty()
        }

        fn claim_controller(&mut self, _radio: Cyw43439Radio) -> Result<(), Cyw43439Error> {
            Ok(())
        }

        fn release_controller(&mut self, _radio: Cyw43439Radio) {}

        fn facet_enabled(&self, radio: Cyw43439Radio) -> Result<bool, Cyw43439Error> {
            Ok(match radio {
                Cyw43439Radio::Bluetooth => self.bluetooth_enabled,
                Cyw43439Radio::Wifi => self.wifi_enabled,
            })
        }

        fn set_facet_enabled(
            &mut self,
            radio: Cyw43439Radio,
            enabled: bool,
        ) -> Result<(), Cyw43439Error> {
            match radio {
                Cyw43439Radio::Bluetooth => self.bluetooth_enabled = enabled,
                Cyw43439Radio::Wifi => self.wifi_enabled = enabled,
            }
            self.powered = self.bluetooth_enabled || self.wifi_enabled;
            Ok(())
        }

        fn controller_powered(&self) -> Result<bool, Cyw43439Error> {
            Ok(self.powered)
        }

        fn set_controller_powered(&mut self, powered: bool) -> Result<(), Cyw43439Error> {
            self.powered = powered;
            Ok(())
        }

        fn set_controller_reset(&mut self, _asserted: bool) -> Result<(), Cyw43439Error> {
            Ok(())
        }

        fn set_controller_wake(&mut self, _awake: bool) -> Result<(), Cyw43439Error> {
            Ok(())
        }

        fn acquire_transport(&mut self, _radio: Cyw43439Radio) -> Result<(), Cyw43439Error> {
            Ok(())
        }

        fn release_transport(&mut self, _radio: Cyw43439Radio) {}

        fn wait_for_controller_irq(
            &mut self,
            _radio: Cyw43439Radio,
            _timeout_ms: Option<u32>,
        ) -> Result<bool, Cyw43439Error> {
            Err(Cyw43439Error::unsupported())
        }

        fn acknowledge_controller_irq(
            &mut self,
            _radio: Cyw43439Radio,
        ) -> Result<(), Cyw43439Error> {
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
            radio: Cyw43439Radio,
        ) -> Result<Option<&'static [u8]>, Cyw43439Error> {
            Ok(match radio {
                Cyw43439Radio::Bluetooth => self.bluetooth_patch,
                Cyw43439Radio::Wifi => self.wifi_firmware,
            })
        }

        fn nvram_image(
            &self,
            radio: Cyw43439Radio,
        ) -> Result<Option<&'static [u8]>, Cyw43439Error> {
            Ok(match radio {
                Cyw43439Radio::Bluetooth => None,
                Cyw43439Radio::Wifi => self.wifi_nvram,
            })
        }

        fn clm_image(&self, radio: Cyw43439Radio) -> Result<Option<&'static [u8]>, Cyw43439Error> {
            Ok(match radio {
                Cyw43439Radio::Bluetooth => None,
                Cyw43439Radio::Wifi => self.wifi_clm,
            })
        }

        fn reference_clock_hz(&self) -> Result<Option<u32>, Cyw43439Error> {
            Ok(self.reference_clock_hz)
        }

        fn sleep_clock_hz(&self) -> Result<Option<u32>, Cyw43439Error> {
            Ok(self.sleep_clock_hz)
        }

        fn delay_ms(&self, _milliseconds: u32) {}
    }

    #[test]
    fn chipset_tracks_power_state_from_facet_enable() {
        let hardware = FakeHardware::new();
        let mut chipset = Cyw43439Chipset::new(hardware);

        assert_eq!(chipset.state(), Cyw43439ChipState::Cold);

        chipset.set_bluetooth_enabled(true).unwrap();
        assert_eq!(chipset.state(), Cyw43439ChipState::Powered);

        chipset.mark_clocked();
        chipset.mark_firmware_loaded();
        chipset.mark_ready();
        assert_eq!(chipset.state(), Cyw43439ChipState::Ready);

        chipset.set_bluetooth_enabled(false).unwrap();
        assert_eq!(chipset.state(), Cyw43439ChipState::Cold);
    }

    #[test]
    fn chipset_reports_transport_profile_and_assets() {
        let chipset = Cyw43439Chipset::new(FakeHardware::new());

        let profile = chipset.transport_profile().unwrap();
        assert_eq!(
            profile.bluetooth,
            Some(Cyw43439BluetoothTransport::HciUartH4)
        );
        assert_eq!(
            profile.bluetooth_clock,
            Some(Cyw43439BluetoothTransportClockProfile::HciUart {
                target_baud: Some(3_000_000),
                host_source_clock_hz: Some(150_000_000),
            })
        );
        assert_eq!(profile.wifi, Some(Cyw43439WlanTransport::Gspi));
        assert_eq!(
            profile.wifi_clock,
            Some(Cyw43439WlanTransportClockProfile::Gspi {
                target_clock_hz: Some(31_250_000),
                host_source_clock_hz: Some(150_000_000),
            })
        );
        assert_eq!(
            profile.topology,
            Cyw43439TransportTopology::SplitHostTransports
        );

        let bluetooth_assets = chipset.bluetooth_firmware_assets().unwrap();
        let wifi_assets = chipset.wifi_firmware_assets().unwrap();
        let clocks = chipset.clock_profile().unwrap();
        let readiness = chipset.boot_readiness().unwrap();

        assert_eq!(bluetooth_assets.patch_image, Some(b"bt-patch".as_slice()));
        assert_eq!(wifi_assets.firmware_image, Some(b"wifi-fw".as_slice()));
        assert_eq!(wifi_assets.nvram_image, Some(b"wifi-nvram".as_slice()));
        assert_eq!(wifi_assets.clm_image, Some(b"wifi-clm".as_slice()));
        assert_eq!(
            clocks,
            Cyw43439ClockProfile {
                reference_clock_hz: Some(37_400_000),
                sleep_clock_hz: None,
            }
        );
        assert_eq!(
            readiness,
            Cyw43439BootReadiness {
                state: Cyw43439ChipState::Cold,
                transport: profile,
                clocks,
                bluetooth_patch_available: true,
                wifi_firmware_available: true,
                wifi_nvram_available: true,
                wifi_clm_available: true,
                can_boot_bluetooth: true,
                can_boot_wifi: true,
            }
        );
    }
}
