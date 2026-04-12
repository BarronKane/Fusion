//! ACPI lid contract vocabulary.

use super::{
    AcpiComponentSupport,
    AcpiError,
    AcpiObjectDescriptor,
};

/// Static descriptor for one ACPI lid object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiLidDescriptor {
    pub object: AcpiObjectDescriptor,
}

/// Support summary for one ACPI lid surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiLidSupport {
    pub component: AcpiComponentSupport,
    pub state_method_present: bool,
    pub wake_control_present: bool,
}

impl AcpiLidSupport {
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            component: AcpiComponentSupport::unsupported(),
            state_method_present: false,
            wake_control_present: false,
        }
    }
}

/// Runtime lid state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiLidState {
    pub open: bool,
    pub wake_enabled: Option<bool>,
}

/// Public ACPI lid contract.
pub trait AcpiLidContract {
    /// Returns the surfaced lid descriptors.
    fn lids(&self) -> &'static [AcpiLidDescriptor];

    /// Returns the support summary for one lid object.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the lid index is invalid.
    fn lid_support(&self, index: u8) -> Result<AcpiLidSupport, AcpiError>;

    /// Returns one live lid state when the backend can evaluate it honestly.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the lid index is invalid or runtime evaluation is not yet
    /// realized.
    fn lid_state(&self, index: u8) -> Result<AcpiLidState, AcpiError>;
}
