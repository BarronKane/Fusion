//! Platform abstraction layer for low-level Fusion system contracts.
//!
//! The PAL exposes the exact memory behavior and normalized memory inventory a selected
//! backend can support without synthesizing fake portability. Higher layers are expected to
//! negotiate from this truth rather than assume that every operating system behaves like
//! every other one.

#![no_std]

#[path = "hal/hal.rs"]
/// Selected hardware abstraction surface built from PAL contracts and the current backend.
pub mod hal;
#[path = "pal/pal.rs"]
/// Backend-neutral PAL contracts.
pub mod pal;
#[path = "sys/sys.rs"]
/// Selected platform backend and public syscall-facing exports.
pub mod sys;

/// Enumeration of operating systems currently modeled by the PAL.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Platform {
    /// Apple iOS and closely related Darwin mobile targets.
    Ios,
    /// Linux and Linux-compatible userspace environments.
    Linux,
    /// Apple macOS and closely related desktop Darwin targets.
    MacOs,
    /// Microsoft Windows targets.
    Windows,
}

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
