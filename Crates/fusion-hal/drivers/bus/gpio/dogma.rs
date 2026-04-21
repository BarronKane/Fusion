use super::{
    DriverContractKey,
    DriverDogma,
    DriverUsefulness,
};

const GPIO_DRIVER_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("bus.gpio")];
const GPIO_DRIVER_REQUIRED_CONTRACTS: [DriverContractKey; 0] = [];

pub const GPIO_DRIVER_DOGMA: DriverDogma = DriverDogma {
    key: "bus.gpio",
    contracts: &GPIO_DRIVER_CONTRACTS,
    required_contracts: &GPIO_DRIVER_REQUIRED_CONTRACTS,
    usefulness: DriverUsefulness::Standalone,
    singleton_class: None,
};

#[allow(dead_code)]
pub const DOGMAS: &[DriverDogma] = &[GPIO_DRIVER_DOGMA];
