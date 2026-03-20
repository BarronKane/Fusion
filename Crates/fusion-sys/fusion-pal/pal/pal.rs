/// Shared capability vocabulary reused across fusion-pal contract domains.
pub mod caps;
#[path = "context/context.rs"]
/// Backend-neutral user-space execution context vocabulary and fusion-pal traits.
pub mod context;
#[path = "event/event.rs"]
/// Backend-neutral eventing vocabulary and fusion-pal traits.
pub mod event;
#[path = "hal/hal.rs"]
/// Backend-neutral hardware query vocabulary and fusion-pal HAL traits.
pub mod hal;
#[path = "mem/mem.rs"]
/// Backend-neutral memory vocabulary and unsafe fusion-pal traits.
pub mod mem;
#[path = "power/power.rs"]
/// Backend-neutral power-management vocabulary and fusion-pal traits.
pub mod power;
#[path = "sync/sync.rs"]
/// Backend-neutral synchronization vocabulary and low-level fusion-pal traits.
pub mod sync;
#[path = "thread/thread.rs"]
/// Backend-neutral thread vocabulary and low-level fusion-pal traits.
pub mod thread;
