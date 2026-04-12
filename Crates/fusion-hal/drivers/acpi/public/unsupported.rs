//! Unsupported ACPI hardware-facing backend placeholders.

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

use crate::drivers::acpi::public::interface::contract::{
    AcpiBatteryHardware,
    AcpiButtonHardware,
    AcpiEmbeddedControllerHardware,
    AcpiFanHardware,
    AcpiHardware,
    AcpiLidHardware,
    AcpiPowerSourceHardware,
    AcpiProcessorHardware,
    AcpiThermalHardware,
};

/// Unsupported ACPI backend placeholder used as the default type parameter for public ACPI
/// drivers. It surfaces no providers and returns honest unsupported errors for runtime queries.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedAcpiHardware;

const NO_BATTERIES: [AcpiBatteryDescriptor; 0] = [];
const NO_POWER_SOURCES: [AcpiPowerSourceDescriptor; 0] = [];
const NO_THERMAL_ZONES: [AcpiThermalZoneDescriptor; 0] = [];
const NO_FANS: [AcpiFanDescriptor; 0] = [];
const NO_BUTTONS: [AcpiButtonDescriptor; 0] = [];
const NO_LIDS: [AcpiLidDescriptor; 0] = [];
const NO_ECS: [AcpiEmbeddedControllerDescriptor; 0] = [];
const NO_PROCESSORS: [AcpiProcessorDescriptor; 0] = [];

impl AcpiHardware for UnsupportedAcpiHardware {
    fn provider_count() -> u8 {
        0
    }

    fn provider(_provider: u8) -> Option<&'static AcpiProviderDescriptor> {
        None
    }
}

impl AcpiBatteryHardware for UnsupportedAcpiHardware {
    fn batteries(_provider: u8) -> &'static [AcpiBatteryDescriptor] {
        &NO_BATTERIES
    }

    fn battery_support(_provider: u8, _index: u8) -> Result<AcpiBatterySupport, AcpiError> {
        Err(AcpiError::unsupported())
    }

    fn battery_information(_provider: u8, _index: u8) -> Result<AcpiBatteryInformation, AcpiError> {
        Err(AcpiError::unsupported())
    }

    fn battery_status(_provider: u8, _index: u8) -> Result<AcpiBatteryStatus, AcpiError> {
        Err(AcpiError::unsupported())
    }
}

impl AcpiPowerSourceHardware for UnsupportedAcpiHardware {
    fn power_sources(_provider: u8) -> &'static [AcpiPowerSourceDescriptor] {
        &NO_POWER_SOURCES
    }

    fn power_source_support(
        _provider: u8,
        _index: u8,
    ) -> Result<AcpiPowerSourceSupport, AcpiError> {
        Err(AcpiError::unsupported())
    }

    fn power_source_state(_provider: u8, _index: u8) -> Result<AcpiPowerSourceState, AcpiError> {
        Err(AcpiError::unsupported())
    }
}

impl AcpiThermalHardware for UnsupportedAcpiHardware {
    fn thermal_zones(_provider: u8) -> &'static [AcpiThermalZoneDescriptor] {
        &NO_THERMAL_ZONES
    }

    fn thermal_zone_support(_provider: u8, _index: u8) -> Result<AcpiThermalSupport, AcpiError> {
        Err(AcpiError::unsupported())
    }

    fn thermal_reading(_provider: u8, _index: u8) -> Result<AcpiThermalReading, AcpiError> {
        Err(AcpiError::unsupported())
    }
}

impl AcpiFanHardware for UnsupportedAcpiHardware {
    fn fans(_provider: u8) -> &'static [AcpiFanDescriptor] {
        &NO_FANS
    }

    fn fan_support(_provider: u8, _index: u8) -> Result<AcpiFanSupport, AcpiError> {
        Err(AcpiError::unsupported())
    }

    fn fan_state(_provider: u8, _index: u8) -> Result<AcpiFanState, AcpiError> {
        Err(AcpiError::unsupported())
    }
}

impl AcpiButtonHardware for UnsupportedAcpiHardware {
    fn buttons(_provider: u8) -> &'static [AcpiButtonDescriptor] {
        &NO_BUTTONS
    }

    fn button_support(_provider: u8, _index: u8) -> Result<AcpiButtonSupport, AcpiError> {
        Err(AcpiError::unsupported())
    }

    fn button_state(_provider: u8, _index: u8) -> Result<AcpiButtonState, AcpiError> {
        Err(AcpiError::unsupported())
    }
}

impl AcpiLidHardware for UnsupportedAcpiHardware {
    fn lids(_provider: u8) -> &'static [AcpiLidDescriptor] {
        &NO_LIDS
    }

    fn lid_support(_provider: u8, _index: u8) -> Result<AcpiLidSupport, AcpiError> {
        Err(AcpiError::unsupported())
    }

    fn lid_state(_provider: u8, _index: u8) -> Result<AcpiLidState, AcpiError> {
        Err(AcpiError::unsupported())
    }
}

impl AcpiEmbeddedControllerHardware for UnsupportedAcpiHardware {
    fn embedded_controllers(_provider: u8) -> &'static [AcpiEmbeddedControllerDescriptor] {
        &NO_ECS
    }

    fn embedded_controller_support(
        _provider: u8,
        _index: u8,
    ) -> Result<AcpiEmbeddedControllerSupport, AcpiError> {
        Err(AcpiError::unsupported())
    }

    fn embedded_controller_read(_provider: u8, _index: u8, _register: u8) -> Result<u8, AcpiError> {
        Err(AcpiError::unsupported())
    }

    fn embedded_controller_write(
        _provider: u8,
        _index: u8,
        _register: u8,
        _value: u8,
    ) -> Result<(), AcpiError> {
        Err(AcpiError::unsupported())
    }
}

impl AcpiProcessorHardware for UnsupportedAcpiHardware {
    fn processors(_provider: u8) -> &'static [AcpiProcessorDescriptor] {
        &NO_PROCESSORS
    }

    fn processor_support(_provider: u8, _index: u8) -> Result<AcpiProcessorSupport, AcpiError> {
        Err(AcpiError::unsupported())
    }

    fn processor_state(_provider: u8, _index: u8) -> Result<AcpiProcessorState, AcpiError> {
        Err(AcpiError::unsupported())
    }
}
