//! Public synchronization facade.
//!
//! `fusion-std::sync` is intentionally a thin façade over [`fusion_sys::sync`]. The
//! canonical primitive implementations remain in `fusion-sys` because they are foundational,
//! `no_std`-friendly, and used by higher layers internally. This module exists so public
//! consumers do not need to reach below the `fusion-std` boundary just to access the core
//! synchronization surface.
//!
//! Today this module is only a re-export façade. That is deliberate, not lazy theater.
//! Higher-level synchronization constructs that lean on `alloc`/`std` ergonomics or runtime
//! policy are expected to grow here later without relocating the primitive source of truth.
//! Likely candidates include:
//! - `Barrier`
//! - `WaitGroup`
//! - `Notify`
//! - `Condvar`
//! - bounded queues, channels, or mailboxes
//! - async-aware synchronization adapters

pub use fusion_sys::sync::*;
