//! USB4 routed/fabric vocabulary.

use super::core::*;
use super::error::*;

/// Observable USB4 router lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Usb4RouterState {
    Disabled,
    Enabled,
    Configured,
    Suspended,
    Error,
}

/// USB4 routed/fabric capability truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Usb4Capabilities {
    pub usb3_tunneling: bool,
    pub pcie_tunneling: bool,
    pub displayport_tunneling: bool,
    pub host_router: bool,
    pub device_router: bool,
    pub asymmetric_link: bool,
}

/// USB4 routed/fabric metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Usb4Metadata {
    pub revision: Option<UsbSpecRevision>,
    pub maximum_link_gbps: Option<u16>,
    pub capabilities: Usb4Capabilities,
}

/// Shared USB4 router/fabric surface.
pub trait Usb4Contract: UsbCoreContract {
    /// Returns the current USB4 metadata snapshot.
    fn usb4_metadata(&self) -> Usb4Metadata;

    /// Returns the current router/fabric state.
    ///
    /// # Errors
    ///
    /// Returns an error when the state cannot be observed.
    fn usb4_state(&self) -> Result<Usb4RouterState, UsbError>;
}
