//! FDXE module export for the canonical public ACPI driver families.

#[allow(dead_code)]
mod abi {
    use fusion_hal::contract::drivers::driver::{
        DriverError,
        DriverMetadata,
    };
    #[cfg(test)]
    use fusion_hal::contract::drivers::driver::{
        DriverBindingSource,
        DriverClass,
        DriverContractKey,
        DriverIdentity,
        DriverUsefulness,
    };

    include!(concat!(env!("OUT_DIR"), "/fdxe_shared.rs"));
}

use abi::{
    FdxeDriverExportV1,
    FdxeModuleV1,
    FdxeStaticModuleV1,
};

const DRIVER_EXPORTS: [FdxeDriverExportV1; 8] = [
    FdxeDriverExportV1::new(
        crate::drivers::acpi::public::dogma::BATTERY_DRIVER_DOGMA.key,
        crate::drivers::acpi::public::battery::driver_metadata,
    ),
    FdxeDriverExportV1::new(
        crate::drivers::acpi::public::dogma::BUTTON_DRIVER_DOGMA.key,
        crate::drivers::acpi::public::button::driver_metadata,
    ),
    FdxeDriverExportV1::new(
        crate::drivers::acpi::public::dogma::EC_DRIVER_DOGMA.key,
        crate::drivers::acpi::public::embedded_controller::driver_metadata,
    ),
    FdxeDriverExportV1::new(
        crate::drivers::acpi::public::dogma::FAN_DRIVER_DOGMA.key,
        crate::drivers::acpi::public::fan::driver_metadata,
    ),
    FdxeDriverExportV1::new(
        crate::drivers::acpi::public::dogma::LID_DRIVER_DOGMA.key,
        crate::drivers::acpi::public::lid::driver_metadata,
    ),
    FdxeDriverExportV1::new(
        crate::drivers::acpi::public::dogma::POWER_SOURCE_DRIVER_DOGMA.key,
        crate::drivers::acpi::public::power_source::driver_metadata,
    ),
    FdxeDriverExportV1::new(
        crate::drivers::acpi::public::dogma::PROCESSOR_DRIVER_DOGMA.key,
        crate::drivers::acpi::public::processor::driver_metadata,
    ),
    FdxeDriverExportV1::new(
        crate::drivers::acpi::public::dogma::THERMAL_DRIVER_DOGMA.key,
        crate::drivers::acpi::public::thermal::driver_metadata,
    ),
];

static FDXE_MODULE_HEADER_V1: FdxeModuleV1 = FdxeModuleV1::new(
    env!("CARGO_PKG_NAME"),
    env!("FUSION_FDXE_TARGET_NAME"),
    &DRIVER_EXPORTS,
);

#[cfg(feature = "fdxe-module")]
#[allow(non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static fdxe_module_v1: FdxeModuleV1 = FdxeModuleV1::new(
    env!("CARGO_PKG_NAME"),
    env!("FUSION_FDXE_TARGET_NAME"),
    &DRIVER_EXPORTS,
);

#[cfg(target_os = "none")]
#[used]
#[unsafe(link_section = ".fdxe.modules")]
pub static FDXE_STATIC_MODULE_V1: FdxeStaticModuleV1 =
    FdxeStaticModuleV1::new(&FDXE_MODULE_HEADER_V1);
