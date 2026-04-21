use super::{
    DriverContractKey,
    DriverDogma,
    DriverUsefulness,
};

const VGA_DRIVER_CONTRACTS: [DriverContractKey; 2] = [
    DriverContractKey("display.control"),
    DriverContractKey("display.port"),
];
const VGA_DRIVER_REQUIRED_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("display.layout")];

pub const VGA_DRIVER_DOGMA: DriverDogma = DriverDogma {
    key: "display.port.vga",
    contracts: &VGA_DRIVER_CONTRACTS,
    required_contracts: &VGA_DRIVER_REQUIRED_CONTRACTS,
    usefulness: DriverUsefulness::Standalone,
    singleton_class: None,
};

#[allow(dead_code)]
pub const DOGMAS: &[DriverDogma] = &[VGA_DRIVER_DOGMA];
