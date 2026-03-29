//! Devicetree-backed topology source contracts.

/// Degree of truthful devicetree topology support.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceTreeTopologySupport {
    Unsupported,
    StaticBlob,
    RuntimeEnumeration,
}

/// Firmware-topology contract for devicetree-backed systems.
pub trait DeviceTreeTopologyContract {
    fn devicetree_topology_support(&self) -> DeviceTreeTopologySupport;
}
