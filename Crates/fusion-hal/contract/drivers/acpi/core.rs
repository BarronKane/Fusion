//! Shared ACPI component vocabulary.

/// Stable identity for one surfaced ACPI provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiProviderDescriptor {
    pub id: &'static str,
    pub vendor: &'static str,
    pub platform: &'static str,
    pub description: &'static str,
}

/// Coarse implementation source for one ACPI-backed surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AcpiImplementationKind {
    Unsupported,
    Firmware,
    Emulated,
    Synthetic,
}

/// Truth level currently realized by one ACPI-backed driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AcpiInteractionLevel {
    Unsupported,
    /// Namespace/object identity is known, but live AML-backed runtime interaction is not realized.
    NamespaceOnly,
    /// Live AML-backed runtime interaction is realized.
    RuntimeMethods,
}

/// Shared ACPI component support summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiComponentSupport {
    pub implementation: AcpiImplementationKind,
    pub interaction: AcpiInteractionLevel,
}

impl AcpiComponentSupport {
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            implementation: AcpiImplementationKind::Unsupported,
            interaction: AcpiInteractionLevel::Unsupported,
        }
    }

    #[must_use]
    pub const fn namespace_only() -> Self {
        Self {
            implementation: AcpiImplementationKind::Firmware,
            interaction: AcpiInteractionLevel::NamespaceOnly,
        }
    }

    #[must_use]
    pub const fn runtime_methods() -> Self {
        Self {
            implementation: AcpiImplementationKind::Firmware,
            interaction: AcpiInteractionLevel::RuntimeMethods,
        }
    }

    #[must_use]
    pub const fn is_unsupported(self) -> bool {
        matches!(self.interaction, AcpiInteractionLevel::Unsupported)
    }
}

/// Stable identity for one ACPI namespace object surfaced by a public ACPI driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiObjectDescriptor {
    pub name: &'static str,
    pub path: &'static str,
    pub hid: Option<&'static str>,
    pub uid: Option<u32>,
    pub description: &'static str,
}

/// Deci-Kelvin ACPI temperature value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AcpiDeciKelvin(pub u32);

impl AcpiDeciKelvin {
    #[must_use]
    pub const fn as_deci_kelvin(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn as_celsius_milli(self) -> i32 {
        ((self.0 as i32) * 100) - 273_150
    }
}
