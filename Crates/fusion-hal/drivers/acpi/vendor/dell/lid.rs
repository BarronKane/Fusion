//! Dell Latitude E6430 lid backend.

use crate::contract::drivers::acpi::{
    AcpiComponentSupport,
    AcpiError,
    AcpiLidDescriptor,
    AcpiLidState,
    AcpiLidSupport,
    AcpiObjectDescriptor,
};

use crate::drivers::acpi::public::interface::contract::AcpiLidHardware;
use crate::drivers::acpi::vendor::dell::{
    provider_valid,
    DellLatitudeE6430AcpiHardware,
};

const LIDS: [AcpiLidDescriptor; 1] = [AcpiLidDescriptor {
    object: AcpiObjectDescriptor {
        name: "LID0",
        path: "\\_SB.LID0",
        hid: Some("PNP0C0D"),
        uid: None,
        description: "ACPI lid device",
    },
}];

const LID_SUPPORT: AcpiLidSupport = AcpiLidSupport {
    component: AcpiComponentSupport::namespace_only(),
    state_method_present: true,
    wake_control_present: true,
};

impl AcpiLidHardware for DellLatitudeE6430AcpiHardware {
    fn lids(provider: u8) -> &'static [AcpiLidDescriptor] {
        if provider_valid(provider) { &LIDS } else { &[] }
    }

    fn lid_support(provider: u8, index: u8) -> Result<AcpiLidSupport, AcpiError> {
        if !provider_valid(provider) || usize::from(index) >= LIDS.len() {
            return Err(AcpiError::invalid());
        }

        Ok(LID_SUPPORT)
    }

    fn lid_state(provider: u8, index: u8) -> Result<AcpiLidState, AcpiError> {
        if !provider_valid(provider) || usize::from(index) >= LIDS.len() {
            return Err(AcpiError::invalid());
        }

        Err(AcpiError::unsupported())
    }
}
