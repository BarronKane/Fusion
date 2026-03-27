#![allow(clippy::module_inception)]

pub const PAL_LANE_NAME: &str = "hosted";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectedPalLane;

pub mod fiber_common;

#[cfg(feature = "sys-fusion-kn")]
#[path = "fusion_kn/fusion_kn.rs"]
/// Mediated Fusion kernel hosted platform family.
pub mod fusion_kn;

#[cfg(target_os = "ios")]
#[path = "ios/ios.rs"]
/// iOS hosted platform family.
pub mod ios;

#[cfg(target_os = "linux")]
#[path = "linux/linux.rs"]
/// Linux hosted platform family.
pub mod linux;

#[cfg(target_os = "macos")]
#[path = "macos/macos.rs"]
/// macOS hosted platform family.
pub mod macos;

#[cfg(target_os = "windows")]
#[path = "windows/windows.rs"]
/// Windows hosted platform family.
pub mod windows;
