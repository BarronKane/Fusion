#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcpiSupport {
    Unsupported,
    StaticTables,
    RuntimeEnumeration,
}

pub trait AcpiFirmwareContract {
    fn acpi_support(&self) -> AcpiSupport;
}
