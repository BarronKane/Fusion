//! ACPI button/switch contract vocabulary.

use super::{
    AcpiComponentSupport,
    AcpiError,
    AcpiObjectDescriptor,
};

/// Kind of ACPI button/switch surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AcpiButtonKind {
    Power,
    Sleep,
    AirplaneMode,
    Vendor(&'static str),
}

/// Static descriptor for one ACPI button or switch object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiButtonDescriptor {
    pub object: AcpiObjectDescriptor,
    pub kind: AcpiButtonKind,
}

/// Support summary for one ACPI button/switch surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiButtonSupport {
    pub component: AcpiComponentSupport,
    pub wake_control_present: bool,
    pub state_method_present: bool,
    pub notification_present: bool,
}

impl AcpiButtonSupport {
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            component: AcpiComponentSupport::unsupported(),
            wake_control_present: false,
            state_method_present: false,
            notification_present: false,
        }
    }
}

/// Runtime button/switch state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiButtonState {
    pub pressed: Option<bool>,
    pub wake_enabled: Option<bool>,
}

/// Public ACPI button/switch contract.
pub trait AcpiButtonContract {
    /// Returns the surfaced button descriptors.
    fn buttons(&self) -> &'static [AcpiButtonDescriptor];

    /// Returns the support summary for one button object.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the button index is invalid.
    fn button_support(&self, index: u8) -> Result<AcpiButtonSupport, AcpiError>;

    /// Returns one live button/switch state when the backend can evaluate it honestly.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the button index is invalid or runtime evaluation is not yet
    /// realized.
    fn button_state(&self, index: u8) -> Result<AcpiButtonState, AcpiError>;
}
