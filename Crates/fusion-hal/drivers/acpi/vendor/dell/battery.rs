//! Dell Latitude E6430 battery backend.

use crate::contract::drivers::acpi::{
    AcpiBatteryDescriptor,
    AcpiBatteryInformation,
    AcpiBatteryStatus,
    AcpiBatterySupport,
    AcpiBatteryTechnology,
    AcpiComponentSupport,
    AcpiError,
    AcpiObjectDescriptor,
};

use crate::drivers::acpi::public::interface::contract::AcpiBatteryHardware;
use crate::drivers::acpi::vendor::dell::{
    provider_valid,
    DellLatitudeE6430AcpiHardware,
};

const BATTERIES: [AcpiBatteryDescriptor; 3] = [
    AcpiBatteryDescriptor {
        object: AcpiObjectDescriptor {
            name: "BAT0",
            path: "\\_SB.BAT0",
            hid: Some("PNP0C0A"),
            uid: Some(1),
            description: "Primary ACPI control-method battery",
        },
        slot_index: 0,
        bay_name: "battery-bay-0",
        secondary: false,
        technology: AcpiBatteryTechnology::Unknown,
    },
    AcpiBatteryDescriptor {
        object: AcpiObjectDescriptor {
            name: "BAT1",
            path: "\\_SB.BAT1",
            hid: Some("PNP0C0A"),
            uid: Some(2),
            description: "Secondary ACPI control-method battery",
        },
        slot_index: 1,
        bay_name: "battery-bay-1",
        secondary: true,
        technology: AcpiBatteryTechnology::Unknown,
    },
    AcpiBatteryDescriptor {
        object: AcpiObjectDescriptor {
            name: "BAT2",
            path: "\\_SB.BAT2",
            hid: Some("PNP0C0A"),
            uid: Some(3),
            description: "Tertiary ACPI control-method battery",
        },
        slot_index: 2,
        bay_name: "battery-bay-2",
        secondary: true,
        technology: AcpiBatteryTechnology::Unknown,
    },
];

const BATTERY_SUPPORT: AcpiBatterySupport = AcpiBatterySupport {
    component: AcpiComponentSupport::namespace_only(),
    information_method_present: true,
    status_method_present: true,
};

impl AcpiBatteryHardware for DellLatitudeE6430AcpiHardware {
    fn batteries(provider: u8) -> &'static [AcpiBatteryDescriptor] {
        if provider_valid(provider) {
            &BATTERIES
        } else {
            &[]
        }
    }

    fn battery_support(provider: u8, index: u8) -> Result<AcpiBatterySupport, AcpiError> {
        if !provider_valid(provider) || usize::from(index) >= BATTERIES.len() {
            return Err(AcpiError::invalid());
        }

        Ok(BATTERY_SUPPORT)
    }

    fn battery_information(provider: u8, index: u8) -> Result<AcpiBatteryInformation, AcpiError> {
        if !provider_valid(provider) || usize::from(index) >= BATTERIES.len() {
            return Err(AcpiError::invalid());
        }

        Err(AcpiError::unsupported())
    }

    fn battery_status(provider: u8, index: u8) -> Result<AcpiBatteryStatus, AcpiError> {
        if !provider_valid(provider) || usize::from(index) >= BATTERIES.len() {
            return Err(AcpiError::invalid());
        }

        Err(AcpiError::unsupported())
    }
}
