//! Contract-first Fusion platform abstraction layer.
//!
//! This top-level crate owns:
//! - the readable contract tree
//! - build-time PAL lane composition
//! - the selected implementation surface

#![cfg_attr(not(feature = "std"), no_std)]

#[path = "contract/contract.rs"]
pub mod contract;
#[path = "pal/pal.rs"]
pub mod pal;

#[path = "pcu/pcu.rs"]
/// Backend-neutral PCU vocabulary now owned by the top-level fusion-pal crate.
pub mod pcu;

#[path = "sys/sys.rs"]
pub mod sys;

/// Enumeration of platforms currently modeled by the fusion-pal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Platform {
    /// ARM Cortex-M bare-metal targets (no OS).
    CortexM,
    /// Apple iOS and closely related Darwin mobile targets.
    Ios,
    /// Linux and Linux-compatible userspace environments.
    Linux,
    /// Apple MacOS and closely related desktop Darwin targets.
    MacOs,
    /// Microsoft Windows targets.
    Windows,
}

#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
pub const TARGET_PLATFORM: Platform = Platform::CortexM;

#[cfg(target_os = "ios")]
pub const TARGET_PLATFORM: Platform = Platform::Ios;

#[cfg(target_os = "linux")]
pub const TARGET_PLATFORM: Platform = Platform::Linux;

#[cfg(target_os = "macos")]
pub const TARGET_PLATFORM: Platform = Platform::MacOs;

#[cfg(target_os = "windows")]
pub const TARGET_PLATFORM: Platform = Platform::Windows;
