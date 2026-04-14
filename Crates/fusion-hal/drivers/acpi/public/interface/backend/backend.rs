//! ACPI public-driver backends composed over other drivers.
//!
//! Vendor-specific ACPI realizers live under `drivers::acpi::vendor::*`.

use crate::contract::drivers::acpi::AcpiError;

use crate::drivers::acpi::public::interface::contract::AcpiHardware;

/// AML lowering lane expected for one ACPI object or method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AcpiAmlLoweringKind {
    Interpret,
    Command,
    Signal,
    Transaction,
    Dispatch,
}

/// AML operation-region address-space kind surfaced to ACPI backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AcpiAmlAddressSpaceKind {
    SystemMemory,
    SystemIo,
    PciConfig,
    EmbeddedControl,
    SmBus,
    Gpio,
    GenericSerialBus,
    Other(u8),
}

/// Stable method descriptor required by an ACPI backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiAmlMethodDescriptor {
    pub path: &'static str,
    pub lowering: AcpiAmlLoweringKind,
    pub description: &'static str,
}

/// Stable field descriptor required by an ACPI backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiAmlFieldDescriptor {
    pub path: &'static str,
    pub description: &'static str,
}

/// Stable operation-region descriptor required by an ACPI backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiAmlOpRegionDescriptor {
    pub path: &'static str,
    pub space: AcpiAmlAddressSpaceKind,
    pub description: &'static str,
}

/// Stable namespace-root descriptor surfaced by one ACPI backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiAmlNamespaceDescriptor {
    pub root: &'static str,
    pub description: &'static str,
}

/// AML-facing backend seam consumed by the canonical public ACPI driver families.
///
/// This does not evaluate AML itself. It declares which namespace roots, methods, fields, and
/// opregions a backend expects the firmware AML lane to realize.
pub trait AcpiAmlBackend: AcpiHardware {
    fn aml_namespace(provider: u8) -> Result<AcpiAmlNamespaceDescriptor, AcpiError>;
    fn aml_methods(provider: u8) -> &'static [AcpiAmlMethodDescriptor];
    fn aml_fields(provider: u8) -> &'static [AcpiAmlFieldDescriptor];
    fn aml_opregions(provider: u8) -> &'static [AcpiAmlOpRegionDescriptor];
}
