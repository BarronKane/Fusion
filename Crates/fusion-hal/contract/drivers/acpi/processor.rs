//! ACPI processor contract vocabulary.

use super::{
    AcpiComponentSupport,
    AcpiError,
    AcpiObjectDescriptor,
};

/// Static descriptor for one ACPI processor object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiProcessorDescriptor {
    pub object: AcpiObjectDescriptor,
    pub acpi_processor_id: u8,
    pub logical_index: u8,
}

/// Support summary for one ACPI processor surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiProcessorSupport {
    pub component: AcpiComponentSupport,
    pub performance_states_present: bool,
    pub idle_states_present: bool,
}

impl AcpiProcessorSupport {
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            component: AcpiComponentSupport::unsupported(),
            performance_states_present: false,
            idle_states_present: false,
        }
    }
}

/// Runtime ACPI processor state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiProcessorState {
    pub online: bool,
}

/// Public ACPI processor contract.
pub trait AcpiProcessorContract {
    /// Returns the surfaced processor descriptors.
    fn processors(&self) -> &'static [AcpiProcessorDescriptor];

    /// Returns the support summary for one processor object.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the processor index is invalid.
    fn processor_support(&self, index: u8) -> Result<AcpiProcessorSupport, AcpiError>;

    /// Returns one live processor state when the backend can evaluate it honestly.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the processor index is invalid or runtime evaluation is not
    /// yet realized.
    fn processor_state(&self, index: u8) -> Result<AcpiProcessorState, AcpiError>;
}
