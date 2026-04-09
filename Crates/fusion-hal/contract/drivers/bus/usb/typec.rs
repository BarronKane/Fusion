//! USB Type-C connector and role vocabulary.

use super::core::*;
use super::error::*;

/// Physical orientation of one USB Type-C attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsbTypecOrientation {
    Normal,
    Flipped,
    Unknown,
}

/// Data role on one Type-C link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsbDataRole {
    Host,
    Device,
    DualRole,
    Unknown,
}

/// Power role on one Type-C link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsbPowerRole {
    Source,
    Sink,
    DualRole,
    Unknown,
}

/// One visible alternate-mode lane on a Type-C partner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbTypecAlternateMode {
    pub svid: u16,
    pub mode: u8,
}

/// Partner identity visible through Type-C discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbTypecPartnerIdentity {
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub product_type_vdo1: Option<u32>,
    pub product_type_vdo2: Option<u32>,
    pub usb4_capable: bool,
    pub thunderbolt_compatible: bool,
}

/// Current Type-C port status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbTypecPortStatus<'a> {
    pub connected: bool,
    pub orientation: UsbTypecOrientation,
    pub data_role: UsbDataRole,
    pub power_role: UsbPowerRole,
    pub partner: Option<UsbTypecPartnerIdentity>,
    pub alternate_modes: &'a [UsbTypecAlternateMode],
}

/// Shared Type-C connector-policy surface.
pub trait UsbTypecPortContract: UsbCoreContract {
    /// Returns one truthful Type-C status snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error when the port cannot surface Type-C state.
    fn typec_status(&self) -> Result<UsbTypecPortStatus<'static>, UsbError>;
}
