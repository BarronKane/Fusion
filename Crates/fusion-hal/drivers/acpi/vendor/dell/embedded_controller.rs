//! Dell Latitude E6430 embedded-controller backend.

use crate::contract::drivers::acpi::{
    AcpiComponentSupport,
    AcpiEmbeddedControllerDescriptor,
    AcpiEmbeddedControllerSupport,
    AcpiError,
    AcpiObjectDescriptor,
};

use crate::drivers::acpi::public::interface::contract::AcpiEmbeddedControllerHardware;
use crate::drivers::acpi::vendor::dell::{
    provider_valid,
    DellLatitudeE6430AcpiHardware,
};

const ECS: [AcpiEmbeddedControllerDescriptor; 1] = [AcpiEmbeddedControllerDescriptor {
    object: AcpiObjectDescriptor {
        name: "ECDV",
        path: "\\_SB.PCI0.LPCB.ECDV",
        hid: Some("PNP0C09"),
        uid: Some(0),
        description: "Embedded controller device",
    },
    data_port: 0x0930,
    command_port: 0x0934,
    gpe: Some(0x10),
}];

const EC_SUPPORT: AcpiEmbeddedControllerSupport = AcpiEmbeddedControllerSupport {
    component: AcpiComponentSupport::namespace_only(),
    raw_read_write: false,
};

impl AcpiEmbeddedControllerHardware for DellLatitudeE6430AcpiHardware {
    fn embedded_controllers(provider: u8) -> &'static [AcpiEmbeddedControllerDescriptor] {
        if provider_valid(provider) { &ECS } else { &[] }
    }

    fn embedded_controller_support(
        provider: u8,
        index: u8,
    ) -> Result<AcpiEmbeddedControllerSupport, AcpiError> {
        if !provider_valid(provider) || usize::from(index) >= ECS.len() {
            return Err(AcpiError::invalid());
        }

        Ok(EC_SUPPORT)
    }

    fn embedded_controller_read(provider: u8, index: u8, _register: u8) -> Result<u8, AcpiError> {
        if !provider_valid(provider) || usize::from(index) >= ECS.len() {
            return Err(AcpiError::invalid());
        }

        Err(AcpiError::unsupported())
    }

    fn embedded_controller_write(
        provider: u8,
        index: u8,
        _register: u8,
        _value: u8,
    ) -> Result<(), AcpiError> {
        if !provider_valid(provider) || usize::from(index) >= ECS.len() {
            return Err(AcpiError::invalid());
        }

        Err(AcpiError::unsupported())
    }
}
