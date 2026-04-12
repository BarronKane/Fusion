//! Dell-specific ACPI backend realizers.
//!
//! The initial proving target is the Latitude E6430 dump under `PVAS-PL1`. These backends only
//! surface namespace truth that is directly visible in the AML dump; runtime method execution is
//! intentionally left unsupported until real AML evaluation exists.

pub mod battery;
pub mod button;
pub mod embedded_controller;
pub mod fan;
pub mod lid;
pub mod power_source;
pub mod processor;
pub mod thermal;

use crate::contract::drivers::acpi::AcpiProviderDescriptor;

use crate::drivers::acpi::public::interface::contract::AcpiHardware;

/// Dell Latitude E6430 ACPI backend surfaced from the captured AML namespace.
#[derive(Debug, Clone, Copy, Default)]
pub struct DellLatitudeE6430AcpiHardware;

const DELL_LATITUDE_E6430_PROVIDER: AcpiProviderDescriptor = AcpiProviderDescriptor {
    id: "dell-latitude-e6430-acpi",
    vendor: "Dell",
    platform: "Latitude E6430",
    description: "Dell Latitude E6430 ACPI namespace backend",
};

pub(crate) const fn provider_valid(provider: u8) -> bool {
    provider == 0
}

impl AcpiHardware for DellLatitudeE6430AcpiHardware {
    fn provider_count() -> u8 {
        1
    }

    fn provider(provider: u8) -> Option<&'static AcpiProviderDescriptor> {
        match provider {
            0 => Some(&DELL_LATITUDE_E6430_PROVIDER),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::acpi::public::interface::contract::{
        AcpiBatteryHardware,
        AcpiButtonHardware,
        AcpiEmbeddedControllerHardware,
        AcpiLidHardware,
        AcpiPowerSourceHardware,
        AcpiThermalHardware,
    };

    #[test]
    fn dell_e6430_provider_identity_matches_dump() {
        assert_eq!(DellLatitudeE6430AcpiHardware::provider_count(), 1);

        let provider = DellLatitudeE6430AcpiHardware::provider(0).unwrap();
        assert_eq!(provider.id, "dell-latitude-e6430-acpi");
        assert_eq!(provider.vendor, "Dell");
        assert_eq!(provider.platform, "Latitude E6430");
    }

    #[test]
    fn dell_e6430_static_namespace_surfaces_match_dump() {
        let batteries = DellLatitudeE6430AcpiHardware::batteries(0);
        assert_eq!(batteries.len(), 3);
        assert_eq!(batteries[0].object.path, "\\_SB.BAT0");
        assert_eq!(batteries[1].object.uid, Some(2));
        assert_eq!(batteries[2].object.uid, Some(3));

        let power = DellLatitudeE6430AcpiHardware::power_sources(0);
        assert_eq!(power.len(), 1);
        assert_eq!(power[0].object.path, "\\_SB.AC");
        assert_eq!(power[0].consumer_count, 4);

        let thermal = DellLatitudeE6430AcpiHardware::thermal_zones(0);
        assert_eq!(thermal.len(), 1);
        assert_eq!(thermal[0].object.path, "\\_TZ.THM");
        assert_eq!(
            thermal[0].critical_temperature.unwrap().as_deci_kelvin(),
            3802
        );

        let ec = DellLatitudeE6430AcpiHardware::embedded_controllers(0);
        assert_eq!(ec.len(), 1);
        assert_eq!(ec[0].data_port, 0x0930);
        assert_eq!(ec[0].command_port, 0x0934);
        assert_eq!(ec[0].gpe, Some(0x10));

        let buttons = DellLatitudeE6430AcpiHardware::buttons(0);
        assert_eq!(buttons.len(), 3);
        assert_eq!(buttons[2].object.hid, Some("DELLABCE"));

        let lids = DellLatitudeE6430AcpiHardware::lids(0);
        assert_eq!(lids.len(), 1);
        assert_eq!(lids[0].object.path, "\\_SB.LID0");
    }
}
