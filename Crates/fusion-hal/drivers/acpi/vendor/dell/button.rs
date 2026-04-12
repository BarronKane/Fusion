//! Dell Latitude E6430 button and switch backend.

use crate::contract::drivers::acpi::{
    AcpiButtonDescriptor,
    AcpiButtonKind,
    AcpiButtonState,
    AcpiButtonSupport,
    AcpiComponentSupport,
    AcpiError,
    AcpiObjectDescriptor,
};

use crate::drivers::acpi::public::interface::contract::AcpiButtonHardware;
use crate::drivers::acpi::vendor::dell::{
    provider_valid,
    DellLatitudeE6430AcpiHardware,
};

const BUTTONS: [AcpiButtonDescriptor; 3] = [
    AcpiButtonDescriptor {
        object: AcpiObjectDescriptor {
            name: "PBTN",
            path: "\\_SB.PBTN",
            hid: Some("PNP0C0C"),
            uid: None,
            description: "ACPI power button",
        },
        kind: AcpiButtonKind::Power,
    },
    AcpiButtonDescriptor {
        object: AcpiObjectDescriptor {
            name: "SBTN",
            path: "\\_SB.SBTN",
            hid: Some("PNP0C0E"),
            uid: None,
            description: "ACPI sleep button",
        },
        kind: AcpiButtonKind::Sleep,
    },
    AcpiButtonDescriptor {
        object: AcpiObjectDescriptor {
            name: "RBTN",
            path: "\\_SB.RBTN",
            hid: Some("DELLABCE"),
            uid: None,
            description: "Dell airplane mode switch",
        },
        kind: AcpiButtonKind::AirplaneMode,
    },
];

const BUTTON_SUPPORT: [AcpiButtonSupport; 3] = [
    AcpiButtonSupport {
        component: AcpiComponentSupport::namespace_only(),
        wake_control_present: true,
        state_method_present: false,
        notification_present: true,
    },
    AcpiButtonSupport {
        component: AcpiComponentSupport::namespace_only(),
        wake_control_present: false,
        state_method_present: false,
        notification_present: true,
    },
    AcpiButtonSupport {
        component: AcpiComponentSupport::namespace_only(),
        wake_control_present: false,
        state_method_present: true,
        notification_present: true,
    },
];

impl AcpiButtonHardware for DellLatitudeE6430AcpiHardware {
    fn buttons(provider: u8) -> &'static [AcpiButtonDescriptor] {
        if provider_valid(provider) {
            &BUTTONS
        } else {
            &[]
        }
    }

    fn button_support(provider: u8, index: u8) -> Result<AcpiButtonSupport, AcpiError> {
        if !provider_valid(provider) || usize::from(index) >= BUTTONS.len() {
            return Err(AcpiError::invalid());
        }

        Ok(BUTTON_SUPPORT[usize::from(index)])
    }

    fn button_state(provider: u8, index: u8) -> Result<AcpiButtonState, AcpiError> {
        if !provider_valid(provider) || usize::from(index) >= BUTTONS.len() {
            return Err(AcpiError::invalid());
        }

        Err(AcpiError::unsupported())
    }
}
