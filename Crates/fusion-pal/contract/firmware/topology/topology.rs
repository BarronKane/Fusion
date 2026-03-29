//! Firmware topology source contracts.

pub mod acpi;
pub mod devicetree;
#[path = "static_topology.rs"]
pub mod static_topology;

pub use acpi::*;
pub use devicetree::*;
pub use static_topology::*;

/// Coarse source family used to obtain hardware-topology truth.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FirmwareTopologySourceKind {
    Acpi,
    DeviceTree,
    Static,
}
