//! ACPI fan contract vocabulary.

use super::{
    AcpiComponentSupport,
    AcpiError,
    AcpiObjectDescriptor,
};

/// Static descriptor for one ACPI fan/cooling device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiFanDescriptor {
    pub object: AcpiObjectDescriptor,
}

/// Support summary for one ACPI fan surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiFanSupport {
    pub component: AcpiComponentSupport,
    pub state_methods_present: bool,
}

impl AcpiFanSupport {
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            component: AcpiComponentSupport::unsupported(),
            state_methods_present: false,
        }
    }
}

/// Runtime ACPI fan state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiFanState {
    pub present: bool,
    pub active: bool,
    pub controllable: bool,
}

/// Public ACPI fan contract.
pub trait AcpiFanContract {
    /// Returns the surfaced fan descriptors.
    fn fans(&self) -> &'static [AcpiFanDescriptor];

    /// Returns the support summary for one fan object.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the fan index is invalid.
    fn fan_support(&self, index: u8) -> Result<AcpiFanSupport, AcpiError>;

    /// Returns one live fan state when the backend can evaluate it honestly.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the fan index is invalid or runtime evaluation is not yet
    /// realized.
    fn fan_state(&self, index: u8) -> Result<AcpiFanState, AcpiError>;
}
