#![cfg(all(feature = "std", not(target_os = "none")))]

use super::lock_fusion_std_tests;

mod all;
#[cfg(target_os = "linux")]
mod linux;
