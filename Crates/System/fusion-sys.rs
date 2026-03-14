//! Public system-facing memory and platform contracts for Fusion.
//!
//! `fusion-sys` is the narrow, no-std layer that turns PAL-truthful memory behavior into
//! resource-oriented contracts that higher layers can compose without guessing about the
//! operating system.

#![no_std]

/// Target platform discriminator re-exported from `fusion-pal`.
pub use fusion_pal::{Platform, TARGET_PLATFORM};

#[path = "event/event.rs"]
/// System event and reactor contracts.
pub mod event;
#[path = "fiber/fiber.rs"]
/// System stackful execution and fiber contracts.
pub mod fiber;
#[path = "mem/mem.rs"]
/// System memory contracts and resource abstractions.
pub mod mem;
#[path = "sync/sync.rs"]
/// System synchronization primitives and wrappers.
pub mod sync;
#[path = "thread/thread.rs"]
/// System threading contracts and wrappers.
pub mod thread;
