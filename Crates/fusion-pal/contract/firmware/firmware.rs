pub mod acpi;
pub mod devicetree;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FirmwareDiscoveryKind {
    Acpi,
    DeviceTree,
    StaticComposition,
}
