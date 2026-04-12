//! Dell Latitude E6430 thermal backend.

use crate::contract::drivers::acpi::{
    AcpiComponentSupport,
    AcpiDeciKelvin,
    AcpiError,
    AcpiObjectDescriptor,
    AcpiThermalReading,
    AcpiThermalSupport,
    AcpiThermalZoneDescriptor,
};

use crate::drivers::acpi::public::interface::contract::AcpiThermalHardware;
use crate::drivers::acpi::vendor::dell::{
    provider_valid,
    DellLatitudeE6430AcpiHardware,
};

const THERMAL_ZONES: [AcpiThermalZoneDescriptor; 1] = [AcpiThermalZoneDescriptor {
    object: AcpiObjectDescriptor {
        name: "THM",
        path: "\\_TZ.THM",
        hid: None,
        uid: None,
        description: "Primary ACPI thermal zone",
    },
    critical_temperature: Some(AcpiDeciKelvin(3802)),
}];

const THERMAL_SUPPORT: AcpiThermalSupport = AcpiThermalSupport {
    component: AcpiComponentSupport::namespace_only(),
    critical_temperature_present: true,
    current_temperature_present: true,
};

impl AcpiThermalHardware for DellLatitudeE6430AcpiHardware {
    fn thermal_zones(provider: u8) -> &'static [AcpiThermalZoneDescriptor] {
        if provider_valid(provider) {
            &THERMAL_ZONES
        } else {
            &[]
        }
    }

    fn thermal_zone_support(provider: u8, index: u8) -> Result<AcpiThermalSupport, AcpiError> {
        if !provider_valid(provider) || usize::from(index) >= THERMAL_ZONES.len() {
            return Err(AcpiError::invalid());
        }

        Ok(THERMAL_SUPPORT)
    }

    fn thermal_reading(provider: u8, index: u8) -> Result<AcpiThermalReading, AcpiError> {
        if !provider_valid(provider) || usize::from(index) >= THERMAL_ZONES.len() {
            return Err(AcpiError::invalid());
        }

        Err(AcpiError::unsupported())
    }
}
