//! USB topology and port-model vocabulary.

use super::core::*;
use super::error::*;
use super::typec::*;

/// Canonical host-visible USB device address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbDeviceAddress(pub u8);

/// Canonical port identifier relative to one controller or hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbPortId {
    pub parent_device: Option<UsbDeviceAddress>,
    pub port_number: u8,
}

/// Human-visible connector family for one port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsbConnectorKind {
    TypeA,
    TypeB,
    TypeC,
    MiniA,
    MiniB,
    MicroA,
    MicroB,
    Internal,
    Captive,
    Proprietary,
    Unknown,
}

/// Canonical topology status for one USB port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbPortStatus {
    pub connected: bool,
    pub enabled: bool,
    pub powered: bool,
    pub overcurrent: bool,
    pub reset_in_progress: bool,
    pub suspended: bool,
    pub connector: UsbConnectorKind,
    pub negotiated_speed: Option<UsbSpeed>,
    pub typec_orientation: Option<UsbTypecOrientation>,
    pub data_role: Option<UsbDataRole>,
    pub power_role: Option<UsbPowerRole>,
    pub usb4_capable: bool,
    pub thunderbolt_compatible: bool,
}

/// Shared topology surface for host-visible ports and hubs.
pub trait UsbTopologyContract: UsbCoreContract {
    /// Returns the truthful number of visible downstream ports.
    fn port_count(&self) -> usize;

    /// Returns one truthful status snapshot for the requested port.
    ///
    /// # Errors
    ///
    /// Returns an error when the port is invalid or cannot be characterized.
    fn port_status(&self, port: UsbPortId) -> Result<UsbPortStatus, UsbError>;
}
