use super::{
    DriverContractKey,
    DriverDogma,
    DriverUsefulness,
};

const DVI_DRIVER_CONTRACTS: [DriverContractKey; 2] = [
    DriverContractKey("display.control"),
    DriverContractKey("display.port"),
];
const DVI_DRIVER_REQUIRED_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("display.layout")];

pub const DVI_DRIVER_DOGMA: DriverDogma = DriverDogma {
    key: "display.port.dvi",
    contracts: &DVI_DRIVER_CONTRACTS,
    required_contracts: &DVI_DRIVER_REQUIRED_CONTRACTS,
    usefulness: DriverUsefulness::Standalone,
    singleton_class: None,
};

#[allow(dead_code)]
pub const DOGMAS: &[DriverDogma] = &[DVI_DRIVER_DOGMA];
