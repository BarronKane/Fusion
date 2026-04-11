//! Optional Fusion hardware-abstraction and driver crate.
//!
//! `fusion-hal` owns:
//! - driver contracts layered above PAL substrate truth
//! - shared driver law and FDXE module ABI
//! - reusable peripheral/device drivers that should not live in `fusion-sys`
//!
//! Concrete driver implementations live in external `fd-*` crates such as `fd-bus-gpio`,
//! `fd-bus-pci`, `fd-bus-usb`, and `fd-net-chipset-infineon-cyw43439`. PAL implements
//! hardware-facing driver substrate contracts.

#![cfg_attr(not(feature = "std"), no_std)]

#[path = "contract/contract.rs"]
pub mod contract;

#[path = "drivers/drivers.rs"]
pub mod drivers;

#[path = "fdxe/fdxe.rs"]
pub mod fdxe;
