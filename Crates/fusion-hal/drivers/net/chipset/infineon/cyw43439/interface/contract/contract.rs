//! Hardware-facing CYW43439 combo-chip contracts.

use core::fmt;

use bitflags::bitflags;

use fusion_hal::contract::drivers::bus::gpio::{
    GpioCapabilities,
    GpioPinDescriptor,
    GpioSupport,
};
use fusion_hal::contract::drivers::net::bluetooth::{
    BluetoothAdapterDescriptor,
    BluetoothSupport,
};
use fusion_hal::contract::drivers::net::wifi::{
    WifiAdapterDescriptor,
    WifiSupport,
};
use crate::transport::{
    wlan::Cyw43439GspiF0Register,
    Cyw43439BluetoothTransport,
    Cyw43439BluetoothTransportClockProfile,
    Cyw43439TransportTopology,
    Cyw43439WlanTransport,
    Cyw43439WlanTransportClockProfile,
};

bitflags! {
    /// Truthful board/controller plumbing surfaced for one CYW43439 radio facet.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Cyw43439ControllerCaps: u32 {
        const CLAIM_CONTROLLER          = 1 << 0;
        const POWER_CONTROL             = 1 << 1;
        const RESET_CONTROL             = 1 << 2;
        const WAKE_CONTROL              = 1 << 3;
        const IRQ_WAIT                  = 1 << 4;
        const IRQ_ACKNOWLEDGE           = 1 << 5;
        const TRANSPORT_WRITE           = 1 << 6;
        const TRANSPORT_READ            = 1 << 7;
        const FIRMWARE_IMAGE            = 1 << 8;
        const NVRAM_IMAGE               = 1 << 9;
        const TIMING_DELAY              = 1 << 10;
    }
}

/// Logical radio facets surfaced by one CYW43439 combo chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cyw43439Radio {
    /// Bluetooth controller facet.
    Bluetooth,
    /// Wi-Fi controller facet.
    Wifi,
}

/// Coarse error kind surfaced by the shared CYW43439 hardware interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cyw43439ErrorKind {
    Unsupported,
    Invalid,
    Busy,
    ResourceExhausted,
    StateConflict,
    Platform(i32),
}

/// Shared CYW43439 hardware-interface error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cyw43439Error {
    kind: Cyw43439ErrorKind,
}

impl Cyw43439Error {
    #[must_use]
    pub const fn new(kind: Cyw43439ErrorKind) -> Self {
        Self { kind }
    }

    #[must_use]
    pub const fn kind(self) -> Cyw43439ErrorKind {
        self.kind
    }

    #[must_use]
    pub const fn unsupported() -> Self {
        Self::new(Cyw43439ErrorKind::Unsupported)
    }

    #[must_use]
    pub const fn invalid() -> Self {
        Self::new(Cyw43439ErrorKind::Invalid)
    }

    #[must_use]
    pub const fn busy() -> Self {
        Self::new(Cyw43439ErrorKind::Busy)
    }

    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self::new(Cyw43439ErrorKind::ResourceExhausted)
    }

    #[must_use]
    pub const fn state_conflict() -> Self {
        Self::new(Cyw43439ErrorKind::StateConflict)
    }

    #[must_use]
    pub const fn platform(code: i32) -> Self {
        Self::new(Cyw43439ErrorKind::Platform(code))
    }
}

impl fmt::Display for Cyw43439ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "unsupported"),
            Self::Invalid => write!(f, "invalid"),
            Self::Busy => write!(f, "busy"),
            Self::ResourceExhausted => write!(f, "resource exhausted"),
            Self::StateConflict => write!(f, "state conflict"),
            Self::Platform(code) => write!(f, "platform error {code}"),
        }
    }
}

impl fmt::Display for Cyw43439Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

/// Shared hardware-facing contract for one CYW43439 combo-chip binding.
///
/// This is intentionally lower than the public Bluetooth and Wi-Fi contracts. The radio drivers
/// own the controller state machines and protocol semantics; PAL-backed implementations only
/// surface truthful board wiring, chip control, and transport hooks.
pub trait Cyw43439HardwareContract {
    /// Reports the truthful Bluetooth surface for this substrate.
    fn bluetooth_support(&self) -> BluetoothSupport;

    /// Returns the surfaced Bluetooth adapter descriptors.
    fn bluetooth_adapters(&self) -> &'static [BluetoothAdapterDescriptor];

    /// Returns the truthful Bluetooth host-transport shape for this substrate.
    fn bluetooth_transport(&self) -> Result<Cyw43439BluetoothTransport, Cyw43439Error>;

    /// Returns the truthful Bluetooth host-transport clock/baud plan for this substrate.
    fn bluetooth_transport_clock_profile(
        &self,
    ) -> Result<Cyw43439BluetoothTransportClockProfile, Cyw43439Error>;

    /// Reports the truthful Wi-Fi surface for this substrate.
    fn wifi_support(&self) -> WifiSupport;

    /// Returns the surfaced Wi-Fi adapter descriptors.
    fn wifi_adapters(&self) -> &'static [WifiAdapterDescriptor];

    /// Returns the truthful WLAN host-transport shape for this substrate.
    fn wifi_transport(&self) -> Result<Cyw43439WlanTransport, Cyw43439Error>;

    /// Returns the truthful WLAN host-transport clock plan for this substrate.
    fn wifi_transport_clock_profile(
        &self,
    ) -> Result<Cyw43439WlanTransportClockProfile, Cyw43439Error>;

    /// Returns whether the two radio facets reach the host through split or shared transport
    /// plumbing.
    fn transport_topology(&self) -> Result<Cyw43439TransportTopology, Cyw43439Error>;

    /// Returns the truthful controller-plumbing capability surface for one radio facet.
    fn controller_caps(&self, radio: Cyw43439Radio) -> Cyw43439ControllerCaps;

    /// Claims one radio facet exclusively.
    fn claim_controller(&mut self, radio: Cyw43439Radio) -> Result<(), Cyw43439Error>;

    /// Releases one previously claimed radio facet.
    fn release_controller(&mut self, radio: Cyw43439Radio);

    /// Returns whether one logical radio facet is currently enabled.
    fn facet_enabled(&self, radio: Cyw43439Radio) -> Result<bool, Cyw43439Error>;

    /// Enables or disables one logical radio facet without pretending it owns the whole chip.
    fn set_facet_enabled(
        &mut self,
        radio: Cyw43439Radio,
        enabled: bool,
    ) -> Result<(), Cyw43439Error>;

    /// Returns whether the shared controller rail is currently powered.
    fn controller_powered(&self) -> Result<bool, Cyw43439Error>;

    /// Powers the shared controller rail on or off.
    fn set_controller_powered(&mut self, powered: bool) -> Result<(), Cyw43439Error>;

    /// Asserts or deasserts the shared controller reset line.
    fn set_controller_reset(&mut self, asserted: bool) -> Result<(), Cyw43439Error>;

    /// Asserts or deasserts the shared controller wake line.
    fn set_controller_wake(&mut self, awake: bool) -> Result<(), Cyw43439Error>;

    /// Acquires the shared controller transport for one logical radio facet.
    fn acquire_transport(&mut self, radio: Cyw43439Radio) -> Result<(), Cyw43439Error>;

    /// Releases one previously acquired shared controller transport lease.
    fn release_transport(&mut self, radio: Cyw43439Radio);

    /// Waits for one controller interrupt indication relevant to one radio facet.
    fn wait_for_controller_irq(
        &mut self,
        radio: Cyw43439Radio,
        timeout_ms: Option<u32>,
    ) -> Result<bool, Cyw43439Error>;

    /// Acknowledges one pending controller interrupt indication.
    fn acknowledge_controller_irq(&mut self, radio: Cyw43439Radio) -> Result<(), Cyw43439Error>;

    /// Writes one raw controller transport frame for one radio facet.
    fn write_controller_transport(
        &mut self,
        radio: Cyw43439Radio,
        payload: &[u8],
    ) -> Result<(), Cyw43439Error>;

    /// Reads one raw controller transport frame into caller-owned storage.
    fn read_controller_transport(
        &mut self,
        radio: Cyw43439Radio,
        out: &mut [u8],
    ) -> Result<usize, Cyw43439Error>;

    /// Best-effort driver activity indicator surfaced by the board/module when one exists.
    ///
    /// This is intentionally below the public radio contracts. Boards may choose to surface a
    /// controller-internal activity LED or similar witness, and the driver may toggle it to show
    /// "driver is currently doing work" without pretending this is part of Bluetooth or Wi-Fi
    /// protocol law.
    fn set_driver_activity_indicator(&mut self, _active: bool) -> Result<(), Cyw43439Error> {
        Ok(())
    }

    /// Reports the truthful WL GPIO surface exposed by this substrate.
    fn wl_gpio_support(&self) -> GpioSupport {
        GpioSupport::unsupported()
    }

    /// Returns the surfaced WL GPIO pin descriptors.
    fn wl_gpio_pins(&self) -> &'static [GpioPinDescriptor] {
        &[]
    }

    /// Returns the truthful capability snapshot for one WL GPIO line.
    fn wl_gpio_capabilities(&self, wl_gpio: u8) -> Result<GpioCapabilities, Cyw43439Error> {
        self.wl_gpio_pins()
            .iter()
            .find(|descriptor| descriptor.pin == wl_gpio)
            .map(|descriptor| descriptor.capabilities)
            .ok_or_else(Cyw43439Error::invalid)
    }

    /// Configures one WL GPIO line for input sampling.
    fn configure_wl_gpio_input(&mut self, _wl_gpio: u8) -> Result<(), Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    /// Reads one WL GPIO input level.
    fn read_wl_gpio(&mut self, _wl_gpio: u8) -> Result<bool, Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    /// Configures one WL GPIO line for output and drives the initial level.
    fn configure_wl_gpio_output(
        &mut self,
        _wl_gpio: u8,
        _initial_high: bool,
    ) -> Result<(), Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    /// Drives one WL GPIO line to the requested level.
    fn set_wl_gpio_level(&mut self, _wl_gpio: u8, _high: bool) -> Result<(), Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    /// Gives the host one best-effort chance to progress local cooperative runtime work while
    /// the driver is inside a long synchronous operation.
    fn progress_host_runtime(&self) {}

    /// Reads one bootstrap-phase WLAN F0 register before the host has switched the shared bus into
    /// the normal 32-bit transport mode.
    fn bootstrap_read_wlan_register_swapped_u32(
        &mut self,
        _register: Cyw43439GspiF0Register,
    ) -> Result<u32, Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    /// Writes one bootstrap-phase WLAN F0 register before the shared bus has switched into the
    /// normal 32-bit transport mode.
    fn bootstrap_write_wlan_register_swapped_u32(
        &mut self,
        _register: Cyw43439GspiF0Register,
        _value: u32,
    ) -> Result<(), Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    fn bootstrap_write_raw_bytes(&mut self, _payload: &[u8]) -> Result<(), Cyw43439Error> {
        Err(Cyw43439Error::unsupported())
    }

    /// Returns one optional controller firmware image for one radio facet.
    fn firmware_image(&self, radio: Cyw43439Radio) -> Result<Option<&'static [u8]>, Cyw43439Error>;

    /// Returns one optional controller NVRAM/config image for one radio facet.
    fn nvram_image(&self, radio: Cyw43439Radio) -> Result<Option<&'static [u8]>, Cyw43439Error>;

    /// Returns one optional controller CLM/regulatory image for one radio facet.
    fn clm_image(&self, radio: Cyw43439Radio) -> Result<Option<&'static [u8]>, Cyw43439Error>;

    /// Returns the selected controller reference clock frequency when the board surfaces one.
    fn reference_clock_hz(&self) -> Result<Option<u32>, Cyw43439Error>;

    /// Returns the selected controller external sleep clock frequency when the board surfaces one.
    fn sleep_clock_hz(&self) -> Result<Option<u32>, Cyw43439Error>;

    /// Sleeps for one board-truthful delay interval.
    fn delay_ms(&self, milliseconds: u32);
}
