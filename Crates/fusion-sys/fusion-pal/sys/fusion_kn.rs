//! Mediated Fusion kernel backend surface.
//!
//! This module exposes the shared `fusion-kn` contract and client helpers through the
//! `fusion-pal::sys` namespace. Transport-specific implementations live below this level so
//! the public backend family stays generic even when a given target currently talks over a
//! Linux character device.

#[cfg(target_os = "linux")]
#[path = "linux/linux.rs"]
mod linux_platform;

pub use fusion_kn::client::*;
pub use fusion_kn::contract::*;

#[cfg(target_os = "linux")]
pub use linux_platform::{context, event, fiber, hal, mem, power, sync, thread};

#[cfg(target_os = "linux")]
#[path = "fusion_kn/linux.rs"]
/// Linux transport adapters for the mediated Fusion kernel backend.
pub mod linux;
