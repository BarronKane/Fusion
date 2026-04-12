//! Dell Latitude E6430 fan backend.

use crate::contract::drivers::acpi::{
    AcpiError,
    AcpiFanDescriptor,
    AcpiFanState,
    AcpiFanSupport,
};

use crate::drivers::acpi::public::interface::contract::AcpiFanHardware;
use crate::drivers::acpi::vendor::dell::{
    provider_valid,
    DellLatitudeE6430AcpiHardware,
};

const NO_FANS: [AcpiFanDescriptor; 0] = [];

impl AcpiFanHardware for DellLatitudeE6430AcpiHardware {
    fn fans(provider: u8) -> &'static [AcpiFanDescriptor] {
        let _ = provider_valid(provider);
        &NO_FANS
    }

    fn fan_support(_provider: u8, _index: u8) -> Result<AcpiFanSupport, AcpiError> {
        Err(AcpiError::unsupported())
    }

    fn fan_state(_provider: u8, _index: u8) -> Result<AcpiFanState, AcpiError> {
        Err(AcpiError::unsupported())
    }
}
