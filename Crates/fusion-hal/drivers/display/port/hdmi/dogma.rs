use super::{
    DriverContractKey,
    DriverDogma,
    DriverUsefulness,
};

const HDMI_DRIVER_CONTRACTS: [DriverContractKey; 2] = [
    DriverContractKey("display.control"),
    DriverContractKey("display.port"),
];
const HDMI_DRIVER_REQUIRED_CONTRACTS: [DriverContractKey; 1] =
    [DriverContractKey("display.layout")];

pub const HDMI_DRIVER_DOGMA: DriverDogma = DriverDogma {
    key: "display.port.hdmi",
    contracts: &HDMI_DRIVER_CONTRACTS,
    required_contracts: &HDMI_DRIVER_REQUIRED_CONTRACTS,
    usefulness: DriverUsefulness::Standalone,
    singleton_class: None,
};

#[allow(dead_code)]
pub const DOGMAS: &[DriverDogma] = &[HDMI_DRIVER_DOGMA];
