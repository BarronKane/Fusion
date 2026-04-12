//! ACPI power-source contract vocabulary.

use super::{
    AcpiComponentSupport,
    AcpiError,
    AcpiObjectDescriptor,
};

/// Static descriptor for one ACPI power-source object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiPowerSourceDescriptor {
    pub object: AcpiObjectDescriptor,
    pub consumer_count: u8,
}

/// Support summary for one ACPI power-source surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiPowerSourceSupport {
    pub component: AcpiComponentSupport,
    pub state_method_present: bool,
}

impl AcpiPowerSourceSupport {
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            component: AcpiComponentSupport::unsupported(),
            state_method_present: false,
        }
    }
}

/// Runtime power-source state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiPowerSourceState {
    pub online: bool,
}

/// Public ACPI power-source contract.
pub trait AcpiPowerSourceContract {
    /// Returns the surfaced power-source descriptors.
    fn power_sources(&self) -> &'static [AcpiPowerSourceDescriptor];

    /// Returns the support summary for one power-source object.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the power-source index is invalid.
    fn power_source_support(&self, index: u8) -> Result<AcpiPowerSourceSupport, AcpiError>;

    /// Returns live AC-present state when the backend can evaluate it honestly.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the power-source is invalid or runtime evaluation is not yet
    /// realized.
    fn power_source_state(&self, index: u8) -> Result<AcpiPowerSourceState, AcpiError>;
}
