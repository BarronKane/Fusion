//! Platform abstraction layer for low-level Fusion system contracts.
//!
//! The fusion-pal exposes the exact memory behavior and normalized memory inventory a selected
//! backend can support without synthesizing fake portability. Higher layers are expected to
//! negotiate from this truth rather than assume that every operating system behaves like
//! every other one.

#![no_std]

#[path = "hal/hal.rs"]
/// Selected hardware abstraction surface built from fusion-pal contracts and the current backend.
pub mod hal;
#[path = "pcu/pcu.rs"]
/// Backend-neutral programmable-IO vocabulary and low-level fusion-pal traits.
pub mod pcu;
#[path = "pal/pal.rs"]
/// Backend-neutral fusion-pal contracts.
pub mod pal;
#[path = "sys/sys.rs"]
/// Selected platform backend and public syscall-facing exports.
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
    /// Apple macOS and closely related desktop Darwin targets.
    MacOs,
    /// Microsoft Windows targets.
    Windows,
}

#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
/// The platform variant selected for the current compilation target.
pub const TARGET_PLATFORM: Platform = Platform::CortexM;

#[cfg(target_os = "ios")]
/// The platform variant selected for the current compilation target.
pub const TARGET_PLATFORM: Platform = Platform::Ios;

#[cfg(target_os = "linux")]
/// The platform variant selected for the current compilation target.
pub const TARGET_PLATFORM: Platform = Platform::Linux;

#[cfg(target_os = "macos")]
/// The platform variant selected for the current compilation target.
pub const TARGET_PLATFORM: Platform = Platform::MacOs;

#[cfg(target_os = "windows")]
/// The platform variant selected for the current compilation target.
pub const TARGET_PLATFORM: Platform = Platform::Windows;
