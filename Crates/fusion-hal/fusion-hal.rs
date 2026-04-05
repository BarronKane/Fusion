//! Optional Fusion hardware-abstraction and driver crate.
//!
//! `fusion-hal` owns:
//! - driver contracts layered above PAL substrate truth
//! - selected generic driver implementations
//! - reusable peripheral/device drivers that should not live in `fusion-sys`
//!
//! `fusion-hal` does not own hardware-specific implementations. Those live in external crates
//! such as `fusion-pal`, which implement the driver contracts surfaced here.

#![cfg_attr(not(feature = "std"), no_std)]

#[path = "contract/contract.rs"]
pub mod contract;

#[path = "drivers/drivers.rs"]
pub mod drivers;

#[path = "fdxe/fdxe.rs"]
pub mod fdxe;
