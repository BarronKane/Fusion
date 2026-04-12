//! ACPI battery contract vocabulary.

use super::{
    AcpiComponentSupport,
    AcpiError,
    AcpiObjectDescriptor,
};

/// ACPI battery chemistry/technology kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AcpiBatteryTechnology {
    Unknown,
    NiMH,
    LiIon,
    LiPoly,
    Vendor(&'static str),
}

/// Static descriptor for one ACPI control-method battery object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiBatteryDescriptor {
    pub object: AcpiObjectDescriptor,
    pub slot_index: u8,
    pub bay_name: &'static str,
    pub secondary: bool,
    pub technology: AcpiBatteryTechnology,
}

/// Support summary for one ACPI battery surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiBatterySupport {
    pub component: AcpiComponentSupport,
    pub information_method_present: bool,
    pub status_method_present: bool,
}

impl AcpiBatterySupport {
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            component: AcpiComponentSupport::unsupported(),
            information_method_present: false,
            status_method_present: false,
        }
    }
}

/// Runtime battery information as surfaced through `_BIF`-style control-method plumbing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiBatteryInformation {
    pub design_capacity: Option<u32>,
    pub last_full_charge_capacity: Option<u32>,
    pub design_voltage_mv: Option<u32>,
    pub cycle_count: Option<u32>,
    pub model: Option<&'static str>,
    pub serial: Option<&'static str>,
    pub oem: Option<&'static str>,
}

/// Runtime battery status as surfaced through `_BST`-style control-method plumbing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiBatteryStatus {
    pub present: bool,
    pub charging: bool,
    pub discharging: bool,
    pub remaining_capacity: Option<u32>,
    pub present_rate: Option<u32>,
    pub present_voltage_mv: Option<u32>,
}

/// Public battery contract for ACPI-backed batteries.
pub trait AcpiBatteryContract {
    /// Returns the surfaced battery descriptors.
    fn batteries(&self) -> &'static [AcpiBatteryDescriptor];

    /// Returns the support summary for one surfaced battery object.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the battery index is invalid.
    fn battery_support(&self, index: u8) -> Result<AcpiBatterySupport, AcpiError>;

    /// Returns live battery information when the backend can evaluate it honestly.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the battery is invalid or the backend cannot provide runtime
    /// information yet.
    fn battery_information(&self, index: u8) -> Result<AcpiBatteryInformation, AcpiError>;

    /// Returns live battery status when the backend can evaluate it honestly.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the battery is invalid or the backend cannot provide runtime
    /// status yet.
    fn battery_status(&self, index: u8) -> Result<AcpiBatteryStatus, AcpiError>;
}
