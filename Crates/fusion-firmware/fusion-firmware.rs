//! Firmware and dynamic hardware-discovery framework layered above `fusion-pal` and `fusion-hal`.

#![cfg_attr(not(feature = "std"), no_std)]

#[path = "contract/contract.rs"]
pub mod contract;
#[path = "module/module.rs"]
pub mod module;
#[path = "pal/pal.rs"]
pub mod pal;
#[path = "sys/sys.rs"]
pub mod sys;
