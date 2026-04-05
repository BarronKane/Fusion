//! FDXE module export for the CYW43439 driver family.

#[allow(dead_code)]
mod abi {
    use fusion_hal::contract::drivers::driver::DriverMetadata;
    #[cfg(test)]
    use fusion_hal::contract::drivers::driver::{
        DriverBindingSource,
        DriverClass,
        DriverContractKey,
        DriverIdentity,
    };

    include!("../../../../../fdxe/shared.rs");
}

use crate::{
    bluetooth,
    wifi,
};
use abi::{
    FdxeDriverExportV1,
    FdxeModuleV1,
    FdxeStaticModuleV1,
};

const DRIVER_EXPORTS: [FdxeDriverExportV1; 2] = [
    FdxeDriverExportV1::new(
        "net.bluetooth.infineon.cyw43439",
        bluetooth::driver_metadata,
    ),
    FdxeDriverExportV1::new("net.wifi.infineon.cyw43439", wifi::driver_metadata),
];

#[unsafe(no_mangle)]
pub static fdxe_module_v1: FdxeModuleV1 = FdxeModuleV1::new(
    "fd-net-chipset-infineon-cyw43439",
    "fd-net-chipset-infineon-cyw43439",
    &DRIVER_EXPORTS,
);

#[used]
#[unsafe(link_section = ".fdxe.modules")]
pub static FDXE_STATIC_MODULE_V1: FdxeStaticModuleV1 = FdxeStaticModuleV1::new(&fdxe_module_v1);
