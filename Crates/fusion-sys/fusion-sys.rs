//! Public system-facing allocation, memory, and platform contracts for Fusion.
//!
//! `fusion-sys` is the narrow, no-std layer that turns fusion-pal-truthful memory behavior into
//! resource-oriented contracts that higher layers can compose without guessing about the
//! operating system.

#![cfg_attr(not(feature = "std"), no_std)]

/// Target platform discriminator re-exported from `fusion-pal`.
pub use fusion_pal::{
    Platform,
    TARGET_PLATFORM,
};

#[doc(hidden)]
#[path = "local/local.rs"]
pub mod __local_syscall;
#[path = "alloc/alloc.rs"]
/// fusion-sys allocation contracts and allocator-facing surfaces.
pub mod alloc;
#[path = "channel/channel.rs"]
/// fusion-sys channel transports and local channel demonstration surface.
pub mod channel;
#[path = "claims/claims.rs"]
/// fusion-sys claims vocabulary and composition-facing claim identity surface.
pub mod claims;
#[path = "context/context.rs"]
/// fusion-sys execution-context contracts and ambient local syscall surface.
pub mod context;
#[path = "courier/courier.rs"]
/// fusion-sys courier authority contracts and wrappers.
pub mod courier;
#[path = "domain/domain.rs"]
/// fusion-sys domain registry and context/courier visibility model.
pub mod domain;
#[path = "event/event.rs"]
/// fusion-sys event and reactor contracts.
pub mod event;
#[path = "fiber/fiber.rs"]
/// fusion-sys stackful execution and fiber contracts.
pub mod fiber;
#[path = "locator/locator.rs"]
/// fusion-sys qualified courier names and Fusion surface locators.
pub mod locator;
#[path = "mem/mem.rs"]
/// fusion-sys memory contracts and resource abstractions.
pub mod mem;
#[path = "pcu/pcu.rs"]
/// fusion-sys programmable control-unit composition and runtime glue.
pub mod pcu;
#[path = "sync/sync.rs"]
/// fusion-sys synchronization primitives, atomics, and wrappers.
pub mod sync;
#[path = "thread/thread.rs"]
/// fusion-sys threading contracts and wrappers.
pub mod thread;
#[path = "transport/transport.rs"]
/// fusion-sys transport-layer contracts, protocols, and wrappers.
pub mod transport;
