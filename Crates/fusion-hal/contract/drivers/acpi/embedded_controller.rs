//! ACPI embedded-controller contract vocabulary.

use super::{
    AcpiComponentSupport,
    AcpiError,
    AcpiObjectDescriptor,
};

/// Static descriptor for one ACPI embedded-controller object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiEmbeddedControllerDescriptor {
    pub object: AcpiObjectDescriptor,
    pub data_port: u16,
    pub command_port: u16,
    pub gpe: Option<u8>,
}

/// Support summary for one ACPI EC surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiEmbeddedControllerSupport {
    pub component: AcpiComponentSupport,
    pub raw_read_write: bool,
}

impl AcpiEmbeddedControllerSupport {
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            component: AcpiComponentSupport::unsupported(),
            raw_read_write: false,
        }
    }
}

/// Public ACPI embedded-controller contract.
pub trait AcpiEmbeddedControllerContract {
    /// Returns the surfaced EC descriptors.
    fn embedded_controllers(&self) -> &'static [AcpiEmbeddedControllerDescriptor];

    /// Returns the support summary for one EC object.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the EC index is invalid.
    fn embedded_controller_support(
        &self,
        index: u8,
    ) -> Result<AcpiEmbeddedControllerSupport, AcpiError>;

    /// Reads one raw EC register when the backend can do so honestly.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the EC index is invalid or runtime interaction is not yet
    /// realized.
    fn embedded_controller_read(&self, index: u8, register: u8) -> Result<u8, AcpiError>;

    /// Writes one raw EC register when the backend can do so honestly.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the EC index is invalid or runtime interaction is not yet
    /// realized.
    fn embedded_controller_write(
        &mut self,
        index: u8,
        register: u8,
        value: u8,
    ) -> Result<(), AcpiError>;
}
