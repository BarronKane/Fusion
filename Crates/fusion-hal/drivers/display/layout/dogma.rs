use super::{
    DriverContractKey,
    DriverDogma,
    DriverUsefulness,
};

const DISPLAY_LAYOUT_DRIVER_CONTRACTS: [DriverContractKey; 1] =
    [DriverContractKey("display.layout")];
const DISPLAY_LAYOUT_DRIVER_REQUIRED_CONTRACTS: [DriverContractKey; 0] = [];

pub const DISPLAY_LAYOUT_DRIVER_DOGMA: DriverDogma = DriverDogma {
    key: "display.layout",
    contracts: &DISPLAY_LAYOUT_DRIVER_CONTRACTS,
    required_contracts: &DISPLAY_LAYOUT_DRIVER_REQUIRED_CONTRACTS,
    usefulness: DriverUsefulness::MustBeConsumed,
    singleton_class: Some("display.layout.machine"),
};

#[allow(dead_code)]
pub const DOGMAS: &[DriverDogma] = &[DISPLAY_LAYOUT_DRIVER_DOGMA];
