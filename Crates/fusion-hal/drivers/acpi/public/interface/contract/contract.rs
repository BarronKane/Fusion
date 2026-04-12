//! Hardware-facing ACPI substrate contracts consumed by the canonical public ACPI drivers.

use crate::contract::drivers::acpi::{
    AcpiBatteryDescriptor,
    AcpiBatteryInformation,
    AcpiBatteryStatus,
    AcpiBatterySupport,
    AcpiButtonDescriptor,
    AcpiButtonState,
    AcpiButtonSupport,
    AcpiEmbeddedControllerDescriptor,
    AcpiEmbeddedControllerSupport,
    AcpiError,
    AcpiFanDescriptor,
    AcpiFanState,
    AcpiFanSupport,
    AcpiLidDescriptor,
    AcpiLidState,
    AcpiLidSupport,
    AcpiPowerSourceDescriptor,
    AcpiPowerSourceState,
    AcpiPowerSourceSupport,
    AcpiProcessorDescriptor,
    AcpiProcessorState,
    AcpiProcessorSupport,
    AcpiProviderDescriptor,
    AcpiThermalReading,
    AcpiThermalSupport,
    AcpiThermalZoneDescriptor,
};

/// Hardware-facing contract shared by ACPI-backed provider families.
pub trait AcpiHardware {
    /// Returns the number of surfaced ACPI providers.
    fn provider_count() -> u8;

    /// Returns one stable provider descriptor when the provider exists.
    fn provider(provider: u8) -> Option<&'static AcpiProviderDescriptor>;
}

/// Hardware-facing ACPI battery substrate.
pub trait AcpiBatteryHardware: AcpiHardware {
    fn batteries(provider: u8) -> &'static [AcpiBatteryDescriptor];
    fn battery_support(provider: u8, index: u8) -> Result<AcpiBatterySupport, AcpiError>;
    fn battery_information(provider: u8, index: u8) -> Result<AcpiBatteryInformation, AcpiError>;
    fn battery_status(provider: u8, index: u8) -> Result<AcpiBatteryStatus, AcpiError>;
}

/// Hardware-facing ACPI power-source substrate.
pub trait AcpiPowerSourceHardware: AcpiHardware {
    fn power_sources(provider: u8) -> &'static [AcpiPowerSourceDescriptor];
    fn power_source_support(provider: u8, index: u8) -> Result<AcpiPowerSourceSupport, AcpiError>;
    fn power_source_state(provider: u8, index: u8) -> Result<AcpiPowerSourceState, AcpiError>;
}

/// Hardware-facing ACPI thermal-zone substrate.
pub trait AcpiThermalHardware: AcpiHardware {
    fn thermal_zones(provider: u8) -> &'static [AcpiThermalZoneDescriptor];
    fn thermal_zone_support(provider: u8, index: u8) -> Result<AcpiThermalSupport, AcpiError>;
    fn thermal_reading(provider: u8, index: u8) -> Result<AcpiThermalReading, AcpiError>;
}

/// Hardware-facing ACPI fan substrate.
pub trait AcpiFanHardware: AcpiHardware {
    fn fans(provider: u8) -> &'static [AcpiFanDescriptor];
    fn fan_support(provider: u8, index: u8) -> Result<AcpiFanSupport, AcpiError>;
    fn fan_state(provider: u8, index: u8) -> Result<AcpiFanState, AcpiError>;
}

/// Hardware-facing ACPI button/switch substrate.
pub trait AcpiButtonHardware: AcpiHardware {
    fn buttons(provider: u8) -> &'static [AcpiButtonDescriptor];
    fn button_support(provider: u8, index: u8) -> Result<AcpiButtonSupport, AcpiError>;
    fn button_state(provider: u8, index: u8) -> Result<AcpiButtonState, AcpiError>;
}

/// Hardware-facing ACPI lid substrate.
pub trait AcpiLidHardware: AcpiHardware {
    fn lids(provider: u8) -> &'static [AcpiLidDescriptor];
    fn lid_support(provider: u8, index: u8) -> Result<AcpiLidSupport, AcpiError>;
    fn lid_state(provider: u8, index: u8) -> Result<AcpiLidState, AcpiError>;
}

/// Hardware-facing ACPI embedded-controller substrate.
pub trait AcpiEmbeddedControllerHardware: AcpiHardware {
    fn embedded_controllers(provider: u8) -> &'static [AcpiEmbeddedControllerDescriptor];
    fn embedded_controller_support(
        provider: u8,
        index: u8,
    ) -> Result<AcpiEmbeddedControllerSupport, AcpiError>;
    fn embedded_controller_read(provider: u8, index: u8, register: u8) -> Result<u8, AcpiError>;
    fn embedded_controller_write(
        provider: u8,
        index: u8,
        register: u8,
        value: u8,
    ) -> Result<(), AcpiError>;
}

/// Hardware-facing ACPI processor substrate.
pub trait AcpiProcessorHardware: AcpiHardware {
    fn processors(provider: u8) -> &'static [AcpiProcessorDescriptor];
    fn processor_support(provider: u8, index: u8) -> Result<AcpiProcessorSupport, AcpiError>;
    fn processor_state(provider: u8, index: u8) -> Result<AcpiProcessorState, AcpiError>;
}
