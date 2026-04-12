//! Dell Latitude E6430 processor backend.

use crate::contract::drivers::acpi::{
    AcpiError,
    AcpiProcessorDescriptor,
    AcpiProcessorState,
    AcpiProcessorSupport,
};

use crate::drivers::acpi::public::interface::contract::AcpiProcessorHardware;
use crate::drivers::acpi::vendor::dell::{
    provider_valid,
    DellLatitudeE6430AcpiHardware,
};

const NO_PROCESSORS: [AcpiProcessorDescriptor; 0] = [];

impl AcpiProcessorHardware for DellLatitudeE6430AcpiHardware {
    fn processors(provider: u8) -> &'static [AcpiProcessorDescriptor] {
        let _ = provider_valid(provider);
        &NO_PROCESSORS
    }

    fn processor_support(_provider: u8, _index: u8) -> Result<AcpiProcessorSupport, AcpiError> {
        Err(AcpiError::unsupported())
    }

    fn processor_state(_provider: u8, _index: u8) -> Result<AcpiProcessorState, AcpiError> {
        Err(AcpiError::unsupported())
    }
}
