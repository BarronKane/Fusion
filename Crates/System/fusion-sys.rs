//! Public system-facing memory and platform contracts for Fusion.
//!
//! `fusion-sys` is the narrow, no-std layer that turns PAL-truthful memory behavior into
//! resource-oriented contracts that higher layers can compose without guessing about the
//! operating system.

#![no_std]

/// Target platform discriminator re-exported from `fusion-pal`.
pub use fusion_pal::{Platform, TARGET_PLATFORM};

#[path = "mem/mem.rs"]
/// System memory contracts and resource abstractions.
pub mod mem;
