//! Selected platform backend wiring for the PAL.
//!
//! Each supported target has a private `sys::<platform>` module that implements the PAL
//! contracts. The public `sys::<module>` exports re-export the chosen backend through a
//! uniform path such as `fusion_pal::sys::mem`.

#[cfg(not(any(
    target_os = "ios",
    target_os = "linux",
    target_os = "macos",
    target_os = "windows"
)))]
compile_error!("fusion-pal currently supports only Linux, Windows, macOS, and iOS targets.");

#[cfg(target_os = "ios")]
#[path = "ios/ios.rs"]
mod ios;
#[cfg(target_os = "ios")]
use ios as platform;

#[cfg(target_os = "linux")]
#[path = "linux/linux.rs"]
mod linux;
#[cfg(target_os = "linux")]
use linux as platform;

#[cfg(target_os = "macos")]
#[path = "macos/macos.rs"]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;

#[cfg(target_os = "windows")]
#[path = "windows/windows.rs"]
mod windows;
#[cfg(target_os = "windows")]
use windows as platform;

/// Public memory module re-exported from the selected private platform backend.
pub mod mem;
