#![no_std]

use core::fmt;

/// Canonical contract-family key implemented by one driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DriverContractKey(pub &'static str);

/// Whether one driver is intrinsically useful on its own or only when something else consumes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DriverUsefulness {
    Standalone,
    MustBeConsumed,
}

/// Shared dependency/singleton dogma for one driver family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DriverDogma {
    pub key: &'static str,
    pub contracts: &'static [DriverContractKey],
    pub required_contracts: &'static [DriverContractKey],
    pub usefulness: DriverUsefulness,
    pub singleton_class: Option<&'static str>,
}

/// Canonical inactive reason for one registered driver family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DriverInopReason {
    MissingDependency(DriverContractKey),
    Unconsumed,
    SingletonConflict(&'static str),
}

/// Validated readiness state for one registered driver family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DriverAvailability {
    Unknown,
    Ready,
    Inop(DriverInopReason),
}

/// Metadata-validation failure for one driver set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DriverValidationError {
    MissingDependency {
        driver_key: &'static str,
        required_contract: DriverContractKey,
    },
    Unconsumed {
        driver_key: &'static str,
    },
    SingletonConflict {
        driver_key: &'static str,
        first_driver_key: &'static str,
        singleton_class: &'static str,
    },
}

impl fmt::Display for DriverValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::MissingDependency {
                driver_key,
                required_contract,
            } => write!(
                f,
                "driver '{}' requires contract '{}' but no selected module provides it",
                driver_key, required_contract.0
            ),
            Self::Unconsumed { driver_key } => {
                write!(f, "driver '{}' is selected but unconsumed", driver_key)
            }
            Self::SingletonConflict {
                driver_key,
                first_driver_key,
                singleton_class,
            } => write!(
                f,
                "driver '{}' conflicts with earlier driver '{}' in singleton class '{}'",
                driver_key, first_driver_key, singleton_class
            ),
        }
    }
}

/// Validates one flat driver-dogma set for singleton, dependency, and consumption correctness.
pub fn validate_driver_dogmas(drivers: &[DriverDogma]) -> Result<(), DriverValidationError> {
    for (index, driver) in drivers.iter().enumerate() {
        if let Some(singleton_class) = driver.singleton_class {
            if let Some(first) = drivers[..index]
                .iter()
                .find(|candidate| candidate.singleton_class == Some(singleton_class))
            {
                return Err(DriverValidationError::SingletonConflict {
                    driver_key: driver.key,
                    first_driver_key: first.key,
                    singleton_class,
                });
            }
        }
    }

    for driver in drivers {
        for required in driver.required_contracts {
            let provided = drivers.iter().any(|candidate| {
                candidate.key != driver.key && candidate.contracts.contains(required)
            });
            if !provided {
                return Err(DriverValidationError::MissingDependency {
                    driver_key: driver.key,
                    required_contract: *required,
                });
            }
        }
    }

    for driver in drivers {
        if driver.usefulness != DriverUsefulness::MustBeConsumed {
            continue;
        }

        let consumed = drivers.iter().any(|candidate| {
            candidate.key != driver.key
                && candidate
                    .required_contracts
                    .iter()
                    .any(|required| driver.contracts.contains(required))
        });

        if !consumed {
            return Err(DriverValidationError::Unconsumed {
                driver_key: driver.key,
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        DriverContractKey,
        DriverDogma,
        DriverUsefulness,
        DriverValidationError,
        validate_driver_dogmas,
    };

    const LAYOUT_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("display.layout")];
    const DISPLAY_ENDPOINT_CONTRACTS: [DriverContractKey; 2] = [
        DriverContractKey("display.control"),
        DriverContractKey("display.port"),
    ];
    const EMPTY_REQUIRED: [DriverContractKey; 0] = [];
    const LAYOUT_REQUIRED: [DriverContractKey; 1] = [DriverContractKey("display.layout")];

    const LAYOUT: DriverDogma = DriverDogma {
        key: "display.layout",
        contracts: &LAYOUT_CONTRACTS,
        required_contracts: &EMPTY_REQUIRED,
        usefulness: DriverUsefulness::MustBeConsumed,
        singleton_class: Some("display.layout.machine"),
    };

    const PORT: DriverDogma = DriverDogma {
        key: "display.port.hdmi",
        contracts: &DISPLAY_ENDPOINT_CONTRACTS,
        required_contracts: &LAYOUT_REQUIRED,
        usefulness: DriverUsefulness::Standalone,
        singleton_class: None,
    };

    const OTHER_LAYOUT: DriverDogma = DriverDogma {
        key: "display.layout.alt",
        contracts: &LAYOUT_CONTRACTS,
        required_contracts: &EMPTY_REQUIRED,
        usefulness: DriverUsefulness::MustBeConsumed,
        singleton_class: Some("display.layout.machine"),
    };

    #[test]
    fn validation_accepts_satisfied_stack() {
        assert!(validate_driver_dogmas(&[LAYOUT, PORT]).is_ok());
    }

    #[test]
    fn validation_rejects_missing_dependency() {
        assert_eq!(
            validate_driver_dogmas(&[PORT]),
            Err(DriverValidationError::MissingDependency {
                driver_key: PORT.key,
                required_contract: DriverContractKey("display.layout"),
            })
        );
    }

    #[test]
    fn validation_rejects_unconsumed_root() {
        assert_eq!(
            validate_driver_dogmas(&[LAYOUT]),
            Err(DriverValidationError::Unconsumed {
                driver_key: LAYOUT.key,
            })
        );
    }

    #[test]
    fn validation_rejects_singleton_conflict() {
        assert_eq!(
            validate_driver_dogmas(&[LAYOUT, OTHER_LAYOUT, PORT]),
            Err(DriverValidationError::SingletonConflict {
                driver_key: OTHER_LAYOUT.key,
                first_driver_key: LAYOUT.key,
                singleton_class: "display.layout.machine",
            })
        );
    }
}
