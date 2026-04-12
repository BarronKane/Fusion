//! Dell Latitude E6430 power-source backend.

use crate::contract::drivers::acpi::{
    AcpiComponentSupport,
    AcpiError,
    AcpiObjectDescriptor,
    AcpiPowerSourceDescriptor,
    AcpiPowerSourceState,
    AcpiPowerSourceSupport,
};

use crate::drivers::acpi::public::interface::contract::AcpiPowerSourceHardware;
use crate::drivers::acpi::vendor::dell::{
    provider_valid,
    DellLatitudeE6430AcpiHardware,
};

const POWER_SOURCES: [AcpiPowerSourceDescriptor; 1] = [AcpiPowerSourceDescriptor {
    object: AcpiObjectDescriptor {
        name: "AC",
        path: "\\_SB.AC",
        hid: Some("ACPI0003"),
        uid: None,
        description: "AC adapter power-source device",
    },
    consumer_count: 4,
}];

const POWER_SOURCE_SUPPORT: AcpiPowerSourceSupport = AcpiPowerSourceSupport {
    component: AcpiComponentSupport::namespace_only(),
    state_method_present: true,
};

impl AcpiPowerSourceHardware for DellLatitudeE6430AcpiHardware {
    fn power_sources(provider: u8) -> &'static [AcpiPowerSourceDescriptor] {
        if provider_valid(provider) {
            &POWER_SOURCES
        } else {
            &[]
        }
    }

    fn power_source_support(provider: u8, index: u8) -> Result<AcpiPowerSourceSupport, AcpiError> {
        if !provider_valid(provider) || usize::from(index) >= POWER_SOURCES.len() {
            return Err(AcpiError::invalid());
        }

        Ok(POWER_SOURCE_SUPPORT)
    }

    fn power_source_state(provider: u8, index: u8) -> Result<AcpiPowerSourceState, AcpiError> {
        if !provider_valid(provider) || usize::from(index) >= POWER_SOURCES.len() {
            return Err(AcpiError::invalid());
        }

        Err(AcpiError::unsupported())
    }
}
