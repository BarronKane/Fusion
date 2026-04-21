use super::{
    DriverContractKey,
    DriverDogma,
    DriverUsefulness,
};

const CYW43439_BLUETOOTH_DRIVER_CONTRACTS: [DriverContractKey; 1] =
    [DriverContractKey("net.bluetooth")];
const CYW43439_BLUETOOTH_REQUIRED_CONTRACTS: [DriverContractKey; 0] = [];

pub const CYW43439_BLUETOOTH_DRIVER_DOGMA: DriverDogma = DriverDogma {
    key: "net.bluetooth.infineon.cyw43439",
    contracts: &CYW43439_BLUETOOTH_DRIVER_CONTRACTS,
    required_contracts: &CYW43439_BLUETOOTH_REQUIRED_CONTRACTS,
    usefulness: DriverUsefulness::Standalone,
    singleton_class: None,
};

const CYW43439_WIFI_DRIVER_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("net.wifi")];
const CYW43439_WIFI_REQUIRED_CONTRACTS: [DriverContractKey; 0] = [];

pub const CYW43439_WIFI_DRIVER_DOGMA: DriverDogma = DriverDogma {
    key: "net.wifi.infineon.cyw43439",
    contracts: &CYW43439_WIFI_DRIVER_CONTRACTS,
    required_contracts: &CYW43439_WIFI_REQUIRED_CONTRACTS,
    usefulness: DriverUsefulness::Standalone,
    singleton_class: None,
};

#[allow(dead_code)]
pub const DOGMAS: &[DriverDogma] = &[CYW43439_BLUETOOTH_DRIVER_DOGMA, CYW43439_WIFI_DRIVER_DOGMA];
