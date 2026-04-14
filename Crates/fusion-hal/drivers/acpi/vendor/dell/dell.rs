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

use crate::drivers::acpi::public::interface::backend::{
    AcpiAmlAddressSpaceKind,
    AcpiAmlBackend,
    AcpiAmlFieldDescriptor,
    AcpiAmlLoweringKind,
    AcpiAmlMethodDescriptor,
    AcpiAmlNamespaceDescriptor,
    AcpiAmlOpRegionDescriptor,
};
use crate::drivers::acpi::public::interface::contract::AcpiHardware;
use crate::contract::drivers::acpi::AcpiError;

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

const DELL_LATITUDE_E6430_AML_NAMESPACE: AcpiAmlNamespaceDescriptor = AcpiAmlNamespaceDescriptor {
    root: "\\_SB",
    description: "Dell Latitude E6430 primary AML namespace root",
};

const DELL_LATITUDE_E6430_AML_METHODS: [AcpiAmlMethodDescriptor; 12] = [
    AcpiAmlMethodDescriptor {
        path: "\\_SB.AC._PSR",
        lowering: AcpiAmlLoweringKind::Command,
        description: "AC adapter online-state method",
    },
    AcpiAmlMethodDescriptor {
        path: "\\_SB.BAT0._BIF",
        lowering: AcpiAmlLoweringKind::Command,
        description: "Primary battery information method",
    },
    AcpiAmlMethodDescriptor {
        path: "\\_SB.BAT0._BST",
        lowering: AcpiAmlLoweringKind::Command,
        description: "Primary battery status method",
    },
    AcpiAmlMethodDescriptor {
        path: "\\_SB.BAT1._BIF",
        lowering: AcpiAmlLoweringKind::Command,
        description: "Secondary battery information method",
    },
    AcpiAmlMethodDescriptor {
        path: "\\_SB.BAT1._BST",
        lowering: AcpiAmlLoweringKind::Command,
        description: "Secondary battery status method",
    },
    AcpiAmlMethodDescriptor {
        path: "\\_SB.BAT2._BIF",
        lowering: AcpiAmlLoweringKind::Command,
        description: "Tertiary battery information method",
    },
    AcpiAmlMethodDescriptor {
        path: "\\_SB.BAT2._BST",
        lowering: AcpiAmlLoweringKind::Command,
        description: "Tertiary battery status method",
    },
    AcpiAmlMethodDescriptor {
        path: "\\_SB.LID0._LID",
        lowering: AcpiAmlLoweringKind::Command,
        description: "Lid state method",
    },
    AcpiAmlMethodDescriptor {
        path: "\\_TZ.THM._TMP",
        lowering: AcpiAmlLoweringKind::Command,
        description: "Thermal zone current temperature method",
    },
    AcpiAmlMethodDescriptor {
        path: "\\_SB.RBTN._STA",
        lowering: AcpiAmlLoweringKind::Command,
        description: "Dell airplane-mode switch status method",
    },
    AcpiAmlMethodDescriptor {
        path: "\\_SB.PCI0.LPCB.ECDV._REG",
        lowering: AcpiAmlLoweringKind::Command,
        description: "Embedded-controller region registration method",
    },
    AcpiAmlMethodDescriptor {
        path: "\\_SB.PCI0.LPCB.ECDV._Q66",
        lowering: AcpiAmlLoweringKind::Signal,
        description: "Embedded-controller query handler proving target",
    },
];

const DELL_LATITUDE_E6430_AML_FIELDS: [AcpiAmlFieldDescriptor; 4] = [
    AcpiAmlFieldDescriptor {
        path: "\\_SB.PCI0.LPCB.ECDV.EC00",
        description: "Dell EC status byte used by lid-state helpers",
    },
    AcpiAmlFieldDescriptor {
        path: "\\_SB.PCI0.LPCB.ECDV.EC06",
        description: "Dell EC power-source and battery-presence byte",
    },
    AcpiAmlFieldDescriptor {
        path: "\\_SB.PCI0.LPCB.ECDV.EC22",
        description: "Dell EC thermal sample byte",
    },
    AcpiAmlFieldDescriptor {
        path: "\\_SB.PCI0.LPCB.ECDV.EC29",
        description: "Dell EC battery metadata byte",
    },
];

const DELL_LATITUDE_E6430_AML_OPREGIONS: [AcpiAmlOpRegionDescriptor; 2] = [
    AcpiAmlOpRegionDescriptor {
        path: "\\_SB.PCI0.LPCB.ECDV.ECOR",
        space: AcpiAmlAddressSpaceKind::EmbeddedControl,
        description: "Dell EC operation region",
    },
    AcpiAmlOpRegionDescriptor {
        path: "\\_SB.PCI0.HBUS",
        space: AcpiAmlAddressSpaceKind::PciConfig,
        description: "Dell PCI host-bridge config-space access region",
    },
];

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

impl AcpiAmlBackend for DellLatitudeE6430AcpiHardware {
    fn aml_namespace(provider: u8) -> Result<AcpiAmlNamespaceDescriptor, AcpiError> {
        if !provider_valid(provider) {
            return Err(AcpiError::invalid());
        }

        Ok(DELL_LATITUDE_E6430_AML_NAMESPACE)
    }

    fn aml_methods(provider: u8) -> &'static [AcpiAmlMethodDescriptor] {
        if provider_valid(provider) {
            &DELL_LATITUDE_E6430_AML_METHODS
        } else {
            &[]
        }
    }

    fn aml_fields(provider: u8) -> &'static [AcpiAmlFieldDescriptor] {
        if provider_valid(provider) {
            &DELL_LATITUDE_E6430_AML_FIELDS
        } else {
            &[]
        }
    }

    fn aml_opregions(provider: u8) -> &'static [AcpiAmlOpRegionDescriptor] {
        if provider_valid(provider) {
            &DELL_LATITUDE_E6430_AML_OPREGIONS
        } else {
            &[]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::acpi::public::interface::backend::AcpiAmlBackend;
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

    #[test]
    fn dell_e6430_aml_backend_surfaces_runtime_targets() {
        let namespace = DellLatitudeE6430AcpiHardware::aml_namespace(0).unwrap();
        assert_eq!(namespace.root, "\\_SB");

        let methods = DellLatitudeE6430AcpiHardware::aml_methods(0);
        assert!(methods.iter().any(|method| method.path == "\\_SB.AC._PSR"));
        assert!(methods.iter().any(|method| method.path == "\\_TZ.THM._TMP"));

        let opregions = DellLatitudeE6430AcpiHardware::aml_opregions(0);
        assert!(
            opregions
                .iter()
                .any(|region| region.path == "\\_SB.PCI0.LPCB.ECDV.ECOR")
        );
    }
}
