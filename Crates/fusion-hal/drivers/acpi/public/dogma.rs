use super::{
    DriverContractKey,
    DriverDogma,
    DriverUsefulness,
};

const BATTERY_DRIVER_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("acpi.battery")];
const BUTTON_DRIVER_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("acpi.button")];
const EC_DRIVER_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("acpi.embedded_controller")];
const FAN_DRIVER_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("acpi.fan")];
const LID_DRIVER_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("acpi.lid")];
const POWER_SOURCE_DRIVER_CONTRACTS: [DriverContractKey; 1] =
    [DriverContractKey("acpi.power_source")];
const PROCESSOR_DRIVER_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("acpi.processor")];
const THERMAL_DRIVER_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("acpi.thermal")];
const NO_REQUIRED_CONTRACTS: [DriverContractKey; 0] = [];

pub const BATTERY_DRIVER_DOGMA: DriverDogma = DriverDogma {
    key: "acpi.battery",
    contracts: &BATTERY_DRIVER_CONTRACTS,
    required_contracts: &NO_REQUIRED_CONTRACTS,
    usefulness: DriverUsefulness::Standalone,
    singleton_class: None,
};

pub const BUTTON_DRIVER_DOGMA: DriverDogma = DriverDogma {
    key: "acpi.button",
    contracts: &BUTTON_DRIVER_CONTRACTS,
    required_contracts: &NO_REQUIRED_CONTRACTS,
    usefulness: DriverUsefulness::Standalone,
    singleton_class: None,
};

pub const EC_DRIVER_DOGMA: DriverDogma = DriverDogma {
    key: "acpi.embedded_controller",
    contracts: &EC_DRIVER_CONTRACTS,
    required_contracts: &NO_REQUIRED_CONTRACTS,
    usefulness: DriverUsefulness::Standalone,
    singleton_class: None,
};

pub const FAN_DRIVER_DOGMA: DriverDogma = DriverDogma {
    key: "acpi.fan",
    contracts: &FAN_DRIVER_CONTRACTS,
    required_contracts: &NO_REQUIRED_CONTRACTS,
    usefulness: DriverUsefulness::Standalone,
    singleton_class: None,
};

pub const LID_DRIVER_DOGMA: DriverDogma = DriverDogma {
    key: "acpi.lid",
    contracts: &LID_DRIVER_CONTRACTS,
    required_contracts: &NO_REQUIRED_CONTRACTS,
    usefulness: DriverUsefulness::Standalone,
    singleton_class: None,
};

pub const POWER_SOURCE_DRIVER_DOGMA: DriverDogma = DriverDogma {
    key: "acpi.power_source",
    contracts: &POWER_SOURCE_DRIVER_CONTRACTS,
    required_contracts: &NO_REQUIRED_CONTRACTS,
    usefulness: DriverUsefulness::Standalone,
    singleton_class: None,
};

pub const PROCESSOR_DRIVER_DOGMA: DriverDogma = DriverDogma {
    key: "acpi.processor",
    contracts: &PROCESSOR_DRIVER_CONTRACTS,
    required_contracts: &NO_REQUIRED_CONTRACTS,
    usefulness: DriverUsefulness::Standalone,
    singleton_class: None,
};

pub const THERMAL_DRIVER_DOGMA: DriverDogma = DriverDogma {
    key: "acpi.thermal",
    contracts: &THERMAL_DRIVER_CONTRACTS,
    required_contracts: &NO_REQUIRED_CONTRACTS,
    usefulness: DriverUsefulness::Standalone,
    singleton_class: None,
};

#[allow(dead_code)]
pub const DOGMAS: &[DriverDogma] = &[
    BATTERY_DRIVER_DOGMA,
    BUTTON_DRIVER_DOGMA,
    EC_DRIVER_DOGMA,
    FAN_DRIVER_DOGMA,
    LID_DRIVER_DOGMA,
    POWER_SOURCE_DRIVER_DOGMA,
    PROCESSOR_DRIVER_DOGMA,
    THERMAL_DRIVER_DOGMA,
];
