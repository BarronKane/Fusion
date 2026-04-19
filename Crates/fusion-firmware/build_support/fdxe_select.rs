#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverUsefulnessSpec {
    Standalone,
    MustBeConsumed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DriverSpec {
    pub key: &'static str,
    pub contracts: &'static [&'static str],
    pub required_contracts: &'static [&'static str],
    pub usefulness: DriverUsefulnessSpec,
    pub singleton_class: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleSpec {
    pub crate_name: &'static str,
    pub feature_env: &'static str,
    pub selected_by_soc_rp2350: bool,
    pub drivers: &'static [DriverSpec],
}

const BUS_GPIO_DRIVER: DriverSpec = DriverSpec {
    key: "bus.gpio",
    contracts: &["bus.gpio"],
    required_contracts: &[],
    usefulness: DriverUsefulnessSpec::Standalone,
    singleton_class: None,
};

const BUS_PCI_DRIVER: DriverSpec = DriverSpec {
    key: "bus.pci",
    contracts: &["bus.pci"],
    required_contracts: &[],
    usefulness: DriverUsefulnessSpec::Standalone,
    singleton_class: None,
};

const BUS_USB_DRIVER: DriverSpec = DriverSpec {
    key: "bus.usb",
    contracts: &["bus.usb"],
    required_contracts: &[],
    usefulness: DriverUsefulnessSpec::Standalone,
    singleton_class: None,
};

const DISPLAY_LAYOUT_DRIVER: DriverSpec = DriverSpec {
    key: "display.layout",
    contracts: &["display.layout"],
    required_contracts: &[],
    usefulness: DriverUsefulnessSpec::MustBeConsumed,
    singleton_class: Some("display.layout.machine"),
};

const DISPLAY_HDMI_DRIVER: DriverSpec = DriverSpec {
    key: "display.port.hdmi",
    contracts: &["display.control", "display.port"],
    required_contracts: &["display.layout"],
    usefulness: DriverUsefulnessSpec::Standalone,
    singleton_class: None,
};

const DISPLAY_DVI_DRIVER: DriverSpec = DriverSpec {
    key: "display.port.dvi",
    contracts: &["display.control", "display.port"],
    required_contracts: &["display.layout"],
    usefulness: DriverUsefulnessSpec::Standalone,
    singleton_class: None,
};

const DISPLAY_VGA_DRIVER: DriverSpec = DriverSpec {
    key: "display.port.vga",
    contracts: &["display.control", "display.port"],
    required_contracts: &["display.layout"],
    usefulness: DriverUsefulnessSpec::Standalone,
    singleton_class: None,
};

const DISPLAY_PORT_DRIVER: DriverSpec = DriverSpec {
    key: "display.port.display_port",
    contracts: &["display.control", "display.port"],
    required_contracts: &["display.layout"],
    usefulness: DriverUsefulnessSpec::Standalone,
    singleton_class: None,
};

const CYW43439_BLUETOOTH_DRIVER: DriverSpec = DriverSpec {
    key: "net.bluetooth.infineon.cyw43439",
    contracts: &["net.bluetooth"],
    required_contracts: &[],
    usefulness: DriverUsefulnessSpec::Standalone,
    singleton_class: None,
};

const CYW43439_WIFI_DRIVER: DriverSpec = DriverSpec {
    key: "net.wifi.infineon.cyw43439",
    contracts: &["net.wifi"],
    required_contracts: &[],
    usefulness: DriverUsefulnessSpec::Standalone,
    singleton_class: None,
};

pub const MODULE_SPECS: &[ModuleSpec] = &[
    ModuleSpec {
        crate_name: "fd-bus-gpio",
        feature_env: "CARGO_FEATURE_FD_BUS_GPIO",
        selected_by_soc_rp2350: true,
        drivers: &[BUS_GPIO_DRIVER],
    },
    ModuleSpec {
        crate_name: "fd-bus-pci",
        feature_env: "CARGO_FEATURE_FD_BUS_PCI",
        selected_by_soc_rp2350: false,
        drivers: &[BUS_PCI_DRIVER],
    },
    ModuleSpec {
        crate_name: "fd-bus-usb",
        feature_env: "CARGO_FEATURE_FD_BUS_USB",
        selected_by_soc_rp2350: true,
        drivers: &[BUS_USB_DRIVER],
    },
    ModuleSpec {
        crate_name: "fd-display-layout",
        feature_env: "CARGO_FEATURE_FD_DISPLAY_LAYOUT",
        selected_by_soc_rp2350: false,
        drivers: &[DISPLAY_LAYOUT_DRIVER],
    },
    ModuleSpec {
        crate_name: "fd-display-port-hdmi",
        feature_env: "CARGO_FEATURE_FD_DISPLAY_PORT_HDMI",
        selected_by_soc_rp2350: false,
        drivers: &[DISPLAY_HDMI_DRIVER],
    },
    ModuleSpec {
        crate_name: "fd-display-port-dvi",
        feature_env: "CARGO_FEATURE_FD_DISPLAY_PORT_DVI",
        selected_by_soc_rp2350: false,
        drivers: &[DISPLAY_DVI_DRIVER],
    },
    ModuleSpec {
        crate_name: "fd-display-port-vga",
        feature_env: "CARGO_FEATURE_FD_DISPLAY_PORT_VGA",
        selected_by_soc_rp2350: false,
        drivers: &[DISPLAY_VGA_DRIVER],
    },
    ModuleSpec {
        crate_name: "fd-display-port-display_port",
        feature_env: "CARGO_FEATURE_FD_DISPLAY_PORT_DISPLAY_PORT",
        selected_by_soc_rp2350: false,
        drivers: &[DISPLAY_PORT_DRIVER],
    },
    ModuleSpec {
        crate_name: "fd-net-chipset-infineon-cyw43439",
        feature_env: "CARGO_FEATURE_FD_NET_CHIPSET_INFINEON_CYW43439",
        selected_by_soc_rp2350: true,
        drivers: &[CYW43439_BLUETOOTH_DRIVER, CYW43439_WIFI_DRIVER],
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

    for (index, driver) in selected_drivers.iter().enumerate() {
        if let Some(singleton_class) = driver.singleton_class {
            if let Some(first) = selected_drivers[..index]
                .iter()
                .find(|candidate| candidate.singleton_class == Some(singleton_class))
            {
                return Err(format!(
                    "static FDXE selection conflict: driver '{}' conflicts with earlier driver '{}' in singleton class '{}'",
                    driver.key, first.key, singleton_class
                ));
            }
        }
    }

    for driver in &selected_drivers {
        for required in driver.required_contracts {
            let provided = selected_drivers.iter().any(|candidate| {
                candidate.key != driver.key && candidate.contracts.contains(required)
            });
            if !provided {
                return Err(format!(
                    "static FDXE selection invalid: driver '{}' requires contract '{}' but no selected module provides it",
                    driver.key, required
                ));
            }
        }
    }

    for driver in &selected_drivers {
        if driver.usefulness != DriverUsefulnessSpec::MustBeConsumed {
            continue;
        }

        let consumed = selected_drivers.iter().any(|candidate| {
            candidate.key != driver.key
                && candidate
                    .required_contracts
                    .iter()
                    .any(|required| driver.contracts.contains(required))
        });

        if !consumed {
            return Err(format!(
                "static FDXE selection invalid: driver '{}' is selected but unconsumed",
                driver.key
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        DriverSpec,
        DriverUsefulnessSpec,
        ModuleSpec,
        module_enabled_with,
        validate_selected_modules,
    };

    const LAYOUT: DriverSpec = DriverSpec {
        key: "display.layout",
        contracts: &["display.layout"],
        required_contracts: &[],
        usefulness: DriverUsefulnessSpec::MustBeConsumed,
        singleton_class: Some("display.layout.machine"),
    };

    const HDMI: DriverSpec = DriverSpec {
        key: "display.port.hdmi",
        contracts: &["display.control", "display.port"],
        required_contracts: &["display.layout"],
        usefulness: DriverUsefulnessSpec::Standalone,
        singleton_class: None,
    };

    const OTHER_LAYOUT: DriverSpec = DriverSpec {
        key: "display.layout.alt",
        contracts: &["display.layout"],
        required_contracts: &[],
        usefulness: DriverUsefulnessSpec::MustBeConsumed,
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
