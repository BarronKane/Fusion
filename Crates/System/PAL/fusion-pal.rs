#![no_std]

#[path = "pal/pal.rs"]
pub mod pal;
#[path = "sys/sys.rs"]
pub mod sys;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Platform {
    Ios,
    Linux,
    MacOs,
    Windows,
}

#[cfg(target_os = "ios")]
pub const TARGET_PLATFORM: Platform = Platform::Ios;

#[cfg(target_os = "linux")]
pub const TARGET_PLATFORM: Platform = Platform::Linux;

#[cfg(target_os = "macos")]
pub const TARGET_PLATFORM: Platform = Platform::MacOs;

#[cfg(target_os = "windows")]
pub const TARGET_PLATFORM: Platform = Platform::Windows;
