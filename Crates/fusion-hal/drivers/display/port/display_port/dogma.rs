use super::{
    DriverContractKey,
    DriverDogma,
    DriverUsefulness,
};

const DISPLAY_PORT_DRIVER_CONTRACTS: [DriverContractKey; 2] = [
    DriverContractKey("display.control"),
    DriverContractKey("display.port"),
];
const DISPLAY_PORT_DRIVER_REQUIRED_CONTRACTS: [DriverContractKey; 1] =
    [DriverContractKey("display.layout")];

pub const DISPLAY_PORT_DRIVER_DOGMA: DriverDogma = DriverDogma {
    key: "display.port.display_port",
    contracts: &DISPLAY_PORT_DRIVER_CONTRACTS,
    required_contracts: &DISPLAY_PORT_DRIVER_REQUIRED_CONTRACTS,
    usefulness: DriverUsefulness::Standalone,
    singleton_class: None,
};

#[allow(dead_code)]
pub const DOGMAS: &[DriverDogma] = &[DISPLAY_PORT_DRIVER_DOGMA];
