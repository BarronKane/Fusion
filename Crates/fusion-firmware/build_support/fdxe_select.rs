use fusion_driver_dogma::{
    DriverContractKey,
    DriverDogma,
    DriverUsefulness,
    validate_driver_dogmas,
};

macro_rules! include_driver_dogma {
    ($module:ident, $suffix:literal) => {
        mod $module {
            include!(concat!(env!("CARGO_MANIFEST_DIR"), $suffix));
        }
    };
}

include_driver_dogma!(bus_gpio_dogma, "/../fusion-hal/drivers/bus/gpio/dogma.rs");
include_driver_dogma!(bus_pci_dogma, "/../fusion-hal/drivers/bus/pci/dogma.rs");
include_driver_dogma!(bus_usb_dogma, "/../fusion-hal/drivers/bus/usb/dogma.rs");
include_driver_dogma!(
    acpi_public_dogma,
    "/../fusion-hal/drivers/acpi/public/dogma.rs"
);
include_driver_dogma!(
    display_layout_dogma,
    "/../fusion-hal/drivers/display/layout/dogma.rs"
);
include_driver_dogma!(
    display_hdmi_dogma,
    "/../fusion-hal/drivers/display/port/hdmi/dogma.rs"
);
include_driver_dogma!(
    display_dvi_dogma,
    "/../fusion-hal/drivers/display/port/dvi/dogma.rs"
);
include_driver_dogma!(
    display_vga_dogma,
    "/../fusion-hal/drivers/display/port/vga/dogma.rs"
);
include_driver_dogma!(
    display_display_port_dogma,
    "/../fusion-hal/drivers/display/port/display_port/dogma.rs"
);
include_driver_dogma!(
    net_cyw43439_dogma,
    "/../fusion-hal/drivers/net/chipset/infineon/cyw43439/dogma.rs"
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleSpec {
    pub crate_name: &'static str,
    pub feature_env: &'static str,
    pub selected_by_soc_rp2350: bool,
    pub drivers: &'static [DriverDogma],
}

pub const MODULE_SPECS: &[ModuleSpec] = &[
    ModuleSpec {
        crate_name: "fd-bus-gpio",
        feature_env: "CARGO_FEATURE_FD_BUS_GPIO",
        selected_by_soc_rp2350: true,
        drivers: bus_gpio_dogma::DOGMAS,
    },
    ModuleSpec {
        crate_name: "fd-bus-pci",
        feature_env: "CARGO_FEATURE_FD_BUS_PCI",
        selected_by_soc_rp2350: false,
        drivers: bus_pci_dogma::DOGMAS,
    },
    ModuleSpec {
        crate_name: "fd-bus-usb",
        feature_env: "CARGO_FEATURE_FD_BUS_USB",
        selected_by_soc_rp2350: true,
        drivers: bus_usb_dogma::DOGMAS,
    },
    ModuleSpec {
        crate_name: "fd-acpi-public",
        feature_env: "CARGO_FEATURE_FD_ACPI_PUBLIC",
        selected_by_soc_rp2350: false,
        drivers: acpi_public_dogma::DOGMAS,
    },
    ModuleSpec {
        crate_name: "fd-display-layout",
        feature_env: "CARGO_FEATURE_FD_DISPLAY_LAYOUT",
        selected_by_soc_rp2350: false,
        drivers: display_layout_dogma::DOGMAS,
    },
    ModuleSpec {
        crate_name: "fd-display-port-hdmi",
        feature_env: "CARGO_FEATURE_FD_DISPLAY_PORT_HDMI",
        selected_by_soc_rp2350: false,
        drivers: display_hdmi_dogma::DOGMAS,
    },
    ModuleSpec {
        crate_name: "fd-display-port-dvi",
        feature_env: "CARGO_FEATURE_FD_DISPLAY_PORT_DVI",
        selected_by_soc_rp2350: false,
        drivers: display_dvi_dogma::DOGMAS,
    },
    ModuleSpec {
        crate_name: "fd-display-port-vga",
        feature_env: "CARGO_FEATURE_FD_DISPLAY_PORT_VGA",
        selected_by_soc_rp2350: false,
        drivers: display_vga_dogma::DOGMAS,
    },
    ModuleSpec {
        crate_name: "fd-display-port-display_port",
        feature_env: "CARGO_FEATURE_FD_DISPLAY_PORT_DISPLAY_PORT",
        selected_by_soc_rp2350: false,
        drivers: display_display_port_dogma::DOGMAS,
    },
    ModuleSpec {
        crate_name: "fd-net-chipset-infineon-cyw43439",
        feature_env: "CARGO_FEATURE_FD_NET_CHIPSET_INFINEON_CYW43439",
        selected_by_soc_rp2350: true,
        drivers: net_cyw43439_dogma::DOGMAS,
    },
];

#[must_use]
pub fn module_spec(crate_name: &str) -> Option<ModuleSpec> {
    MODULE_SPECS
        .iter()
        .copied()
        .find(|spec| spec.crate_name == crate_name)
}

#[must_use]
pub fn module_enabled_with<F>(
    spec: ModuleSpec,
    feature_enabled: F,
    soc_rp2350_enabled: bool,
) -> bool
where
    F: Fn(&str) -> bool,
{
    feature_enabled(spec.feature_env) || (spec.selected_by_soc_rp2350 && soc_rp2350_enabled)
}

pub fn validate_selected_modules(selected_modules: &[ModuleSpec]) -> Result<(), String> {
    let mut selected_drivers = Vec::new();
    for module in selected_modules {
        selected_drivers.extend_from_slice(module.drivers);
    }
    validate_driver_dogmas(&selected_drivers)
        .map_err(|error| format!("static FDXE selection invalid: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{
        DriverContractKey,
        DriverDogma,
        DriverUsefulness,
        ModuleSpec,
        module_enabled_with,
        validate_selected_modules,
    };

    const LAYOUT_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("display.layout")];
    const LAYOUT_REQUIRED_CONTRACTS: [DriverContractKey; 0] = [];
    const LAYOUT: DriverDogma = DriverDogma {
        key: "display.layout",
        contracts: &LAYOUT_CONTRACTS,
        required_contracts: &LAYOUT_REQUIRED_CONTRACTS,
        usefulness: DriverUsefulness::MustBeConsumed,
        singleton_class: Some("display.layout.machine"),
    };

    const HDMI_CONTRACTS: [DriverContractKey; 2] = [
        DriverContractKey("display.control"),
        DriverContractKey("display.port"),
    ];
    const HDMI_REQUIRED_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("display.layout")];
    const HDMI: DriverDogma = DriverDogma {
        key: "display.port.hdmi",
        contracts: &HDMI_CONTRACTS,
        required_contracts: &HDMI_REQUIRED_CONTRACTS,
        usefulness: DriverUsefulness::Standalone,
        singleton_class: None,
    };

    const OTHER_LAYOUT: DriverDogma = DriverDogma {
        key: "display.layout.alt",
        contracts: &LAYOUT_CONTRACTS,
        required_contracts: &LAYOUT_REQUIRED_CONTRACTS,
        usefulness: DriverUsefulness::MustBeConsumed,
        singleton_class: Some("display.layout.machine"),
    };

    const LAYOUT_MODULE: ModuleSpec = ModuleSpec {
        crate_name: "fd-display-layout",
        feature_env: "CARGO_FEATURE_FD_DISPLAY_LAYOUT",
        selected_by_soc_rp2350: false,
        drivers: &[LAYOUT],
    };

    const HDMI_MODULE: ModuleSpec = ModuleSpec {
        crate_name: "fd-display-port-hdmi",
        feature_env: "CARGO_FEATURE_FD_DISPLAY_PORT_HDMI",
        selected_by_soc_rp2350: false,
        drivers: &[HDMI],
    };

    const OTHER_LAYOUT_MODULE: ModuleSpec = ModuleSpec {
        crate_name: "fd-display-layout-alt",
        feature_env: "CARGO_FEATURE_FD_DISPLAY_LAYOUT_ALT",
        selected_by_soc_rp2350: false,
        drivers: &[OTHER_LAYOUT],
    };

    #[test]
    fn module_enablement_respects_soc_default() {
        let enabled = module_enabled_with(LAYOUT_MODULE, |_| false, false);
        let rp2350_default = module_enabled_with(
            ModuleSpec {
                selected_by_soc_rp2350: true,
                ..LAYOUT_MODULE
            },
            |_| false,
            true,
        );

        assert!(!enabled);
        assert!(rp2350_default);
    }

    #[test]
    fn static_selection_accepts_satisfied_display_stack() {
        assert!(validate_selected_modules(&[LAYOUT_MODULE, HDMI_MODULE]).is_ok());
    }

    #[test]
    fn static_selection_rejects_missing_dependency() {
        let error = validate_selected_modules(&[HDMI_MODULE]).expect_err("missing layout");
        assert!(error.contains("requires contract 'display.layout'"));
    }

    #[test]
    fn static_selection_rejects_unconsumed_root() {
        let error = validate_selected_modules(&[LAYOUT_MODULE]).expect_err("layout is unconsumed");
        assert!(error.contains("selected but unconsumed"));
    }

    #[test]
    fn static_selection_rejects_singleton_conflict() {
        let error = validate_selected_modules(&[LAYOUT_MODULE, OTHER_LAYOUT_MODULE, HDMI_MODULE])
            .expect_err("singleton conflict");
        assert!(error.contains("singleton class 'display.layout.machine'"));
    }
}
