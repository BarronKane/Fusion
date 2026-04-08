//! Firmware and dynamic hardware-discovery framework layered above `fusion-pal` and `fusion-hal`.

#![cfg_attr(not(feature = "std"), no_std)]

pub use fusion_firmware_macros::fusion_firmware_main;
pub use sys::hal::runtime::{
    FirmwareBootstrapContext,
    RootCourierDescendantRequirement,
    RootCourierKeyringRequirement,
    RootCourierPolicy,
    RootCourierSecurityPolicy,
};

#[path = "contract/contract.rs"]
pub mod contract;
#[path = "module/module.rs"]
pub mod module;
#[path = "pal/pal.rs"]
pub mod pal;
#[path = "sys/sys.rs"]
pub mod sys;

#[doc(hidden)]
pub mod __fusion_pal_entry {
    pub use fusion_pal::sys::entry::*;
}
