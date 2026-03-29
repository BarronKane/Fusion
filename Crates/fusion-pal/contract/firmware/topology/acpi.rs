//! ACPI-backed topology source contracts.

/// Degree of truthful ACPI topology support.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcpiTopologySupport {
    Unsupported,
    StaticTables,
    RuntimeEnumeration,
}

/// Firmware-topology contract for ACPI-backed systems.
pub trait AcpiTopologyContract {
    fn acpi_topology_support(&self) -> AcpiTopologySupport;
}
