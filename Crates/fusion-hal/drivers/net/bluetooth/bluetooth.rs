//! Bluetooth driver implementation families.
//!
//! The public consumer contract lives in `fusion-hal::contract::drivers::net::bluetooth`.
//! Concrete implementation families are organized by vendor/chip underneath this module.

#[path = "infineon/infineon.rs"]
pub mod infineon;
