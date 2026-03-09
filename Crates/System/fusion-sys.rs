#![no_std]

pub use fusion_pal::{Platform, TARGET_PLATFORM};

#[cfg(target_os = "linux")]
#[path = "mem/mem.rs"]
pub mod mem;
