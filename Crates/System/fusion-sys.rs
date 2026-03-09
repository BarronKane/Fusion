#![no_std]

pub use fusion_pal::{Platform, TARGET_PLATFORM};

#[path = "mem/mem.rs"]
pub mod mem;
