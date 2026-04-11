//! PCI Express-specific vocabulary.

use super::core::*;

/// PCIe capability structure version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PciExpressVersion(pub u8);

/// PCIe link-generation truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PciLinkSpeed {
    Gen1,
    Gen2,
    Gen3,
    Gen4,
    Gen5,
    Gen6,
    Other(u8),
}

/// PCIe link width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PciLinkWidth(pub u8);

/// PCIe device/port role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PciExpressDevicePortType {
    Endpoint,
    LegacyEndpoint,
    RootPort,
    UpstreamSwitchPort,
    DownstreamSwitchPort,
    PcieToPciBridge,
    PciToPcieBridge,
    RootComplexIntegratedEndpoint,
    RootComplexEventCollector,
    Reserved(u8),
}

/// PCIe capability and current link-state truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PciExpressProfile {
    pub capability_version: Option<PciExpressVersion>,
    pub device_port_type: Option<PciExpressDevicePortType>,
    pub max_link_speed: Option<PciLinkSpeed>,
    pub current_link_speed: Option<PciLinkSpeed>,
    pub max_link_width: Option<PciLinkWidth>,
    pub current_link_width: Option<PciLinkWidth>,
    pub slot_implemented: bool,
    pub hotplug_capable: bool,
    pub surprise_hotplug_capable: bool,
    pub dll_link_active_reporting_capable: bool,
    pub dll_link_active: Option<bool>,
    pub link_training: Option<bool>,
}

/// PCIe-specific lane for one PCI function.
pub trait PciExpressContract {
    /// Returns one truthful PCIe capability/profile snapshot when this function participates in
    /// PCI Express.
    fn pcie_profile(&self) -> Option<PciExpressProfile>;

    /// Returns the walked extended capability records for this function.
    fn extended_capabilities(&self) -> &[PciExtendedCapabilityRecord];
}
