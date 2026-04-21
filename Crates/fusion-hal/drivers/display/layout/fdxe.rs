//! FDXE module export for the canonical display-layout driver family.

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

const DRIVER_EXPORTS: [FdxeDriverExportV1; 1] = [FdxeDriverExportV1::new(
    crate::dogma::DISPLAY_LAYOUT_DRIVER_DOGMA.key,
    crate::driver_metadata,
)];

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
