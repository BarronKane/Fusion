//! ACPI thermal-zone contract vocabulary.

use super::{
    AcpiComponentSupport,
    AcpiDeciKelvin,
    AcpiError,
    AcpiObjectDescriptor,
};

/// Static descriptor for one ACPI thermal zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiThermalZoneDescriptor {
    pub object: AcpiObjectDescriptor,
    pub critical_temperature: Option<AcpiDeciKelvin>,
}

/// Support summary for one ACPI thermal-zone surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiThermalSupport {
    pub component: AcpiComponentSupport,
    pub critical_temperature_present: bool,
    pub current_temperature_present: bool,
}

impl AcpiThermalSupport {
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            component: AcpiComponentSupport::unsupported(),
            critical_temperature_present: false,
            current_temperature_present: false,
        }
    }
}

/// Runtime thermal reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiThermalReading {
    pub current: AcpiDeciKelvin,
    pub critical: Option<AcpiDeciKelvin>,
}

/// Public ACPI thermal-zone contract.
pub trait AcpiThermalContract {
    /// Returns the surfaced thermal-zone descriptors.
    fn thermal_zones(&self) -> &'static [AcpiThermalZoneDescriptor];

    /// Returns the support summary for one thermal zone.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the thermal-zone index is invalid.
    fn thermal_zone_support(&self, index: u8) -> Result<AcpiThermalSupport, AcpiError>;

    /// Returns one live thermal reading when the backend can evaluate it honestly.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the thermal-zone index is invalid or runtime evaluation is
    /// not yet realized.
    fn thermal_reading(&self, index: u8) -> Result<AcpiThermalReading, AcpiError>;
}
