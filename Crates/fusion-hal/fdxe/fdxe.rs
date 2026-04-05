//! Runtime-loadable Fusion driver module ABI.

use crate::contract::drivers::driver::DriverMetadata;
#[cfg(test)]
use crate::contract::drivers::driver::{
    DriverBindingSource,
    DriverClass,
    DriverContractKey,
    DriverIdentity,
};

include!("shared.rs");
