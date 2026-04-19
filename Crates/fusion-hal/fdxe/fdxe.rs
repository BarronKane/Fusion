//! Runtime-loadable Fusion driver module ABI.

use crate::contract::drivers::driver::{
    DriverError,
    DriverMetadata,
};
#[cfg(test)]
use crate::contract::drivers::driver::{
    DriverBindingSource,
    DriverClass,
    DriverContractKey,
    DriverIdentity,
    DriverUsefulness,
};

include!("shared.rs");
