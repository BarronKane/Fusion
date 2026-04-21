use super::{
    DriverContractKey,
    DriverDogma,
    DriverUsefulness,
};

const USB_DRIVER_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("bus.usb")];
const USB_DRIVER_REQUIRED_CONTRACTS: [DriverContractKey; 0] = [];

pub const USB_DRIVER_DOGMA: DriverDogma = DriverDogma {
    key: "bus.usb",
    contracts: &USB_DRIVER_CONTRACTS,
    required_contracts: &USB_DRIVER_REQUIRED_CONTRACTS,
    usefulness: DriverUsefulness::Standalone,
    singleton_class: None,
};

#[allow(dead_code)]
pub const DOGMAS: &[DriverDogma] = &[USB_DRIVER_DOGMA];
