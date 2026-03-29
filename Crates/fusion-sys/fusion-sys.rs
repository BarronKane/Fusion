//! Public system-facing allocation, memory, and platform contracts for Fusion.
//!
//! `fusion-sys` is the narrow, no-std layer that turns fusion-pal-truthful memory behavior into
//! resource-oriented contracts that higher layers can compose without guessing about the
//! operating system.

#![cfg_attr(not(feature = "std"), no_std)]

/// Target platform discriminator re-exported from `fusion-pal`.
pub use fusion_pal::{Platform, TARGET_PLATFORM};

#[path = "alloc/alloc.rs"]
/// fusion-sys allocation contracts and allocator-facing surfaces.
pub mod alloc;
#[path = "atomic/atomic.rs"]
/// fusion-sys atomic substrate contracts and wrappers.
pub mod atomic;
#[path = "channel/channel.rs"]
/// fusion-sys channel transports and local channel demonstration surface.
pub mod channel;
#[path = "context/context.rs"]
/// fusion-sys native visible-context contracts and wrappers.
pub mod context;
#[path = "courier/courier.rs"]
/// fusion-sys courier authority contracts and wrappers.
pub mod courier;
#[path = "dma/dma.rs"]
/// fusion-sys DMA descriptors and consumer-side policy helpers.
pub mod dma;
#[path = "domain/domain.rs"]
/// fusion-sys domain registry and visibility model.
pub mod domain;
#[path = "event/event.rs"]
/// fusion-sys event and reactor contracts.
pub mod event;
#[path = "fiber/fiber.rs"]
/// fusion-sys stackful execution and fiber contracts.
pub mod fiber;
#[path = "gpio/gpio.rs"]
/// fusion-sys GPIO ownership and capability surfaces.
pub mod gpio;
#[path = "hardware/hardware.rs"]
/// fusion-sys truthful hardware composition surfaces.
pub mod hardware;
#[path = "mem/mem.rs"]
/// fusion-sys memory contracts and resource abstractions.
pub mod mem;
#[path = "pcu/pcu.rs"]
/// fusion-sys programmable-IO planning, IR, and wrapper contracts.
pub mod pcu;
#[path = "power/power.rs"]
/// fusion-sys power-management contracts and wrappers.
pub mod power;
#[path = "protocol/protocol.rs"]
/// fusion-sys protocol contracts and wrappers.
pub mod protocol;
#[path = "sync/sync.rs"]
/// fusion-sys synchronization primitives and wrappers.
pub mod sync;
#[path = "thread/thread.rs"]
/// fusion-sys threading contracts and wrappers.
pub mod thread;
#[path = "transport/transport.rs"]
/// fusion-sys transport-layer contracts and wrappers.
pub mod transport;
#[path = "vector/vector.rs"]
/// fusion-sys interrupt-vector ownership contracts and wrappers.
pub mod vector;
