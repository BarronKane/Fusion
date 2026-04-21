use super::{
    DriverContractKey,
    DriverDogma,
    DriverUsefulness,
};

const PCI_DRIVER_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("bus.pci")];
const PCI_DRIVER_REQUIRED_CONTRACTS: [DriverContractKey; 0] = [];

pub const PCI_DRIVER_DOGMA: DriverDogma = DriverDogma {
    key: "bus.pci",
    contracts: &PCI_DRIVER_CONTRACTS,
    required_contracts: &PCI_DRIVER_REQUIRED_CONTRACTS,
    usefulness: DriverUsefulness::Standalone,
    singleton_class: None,
};

#[allow(dead_code)]
pub const DOGMAS: &[DriverDogma] = &[PCI_DRIVER_DOGMA];
