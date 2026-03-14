#[path = "context/context.rs"]
/// Backend-neutral user-space execution context vocabulary and PAL traits.
pub mod context;
#[path = "event/event.rs"]
/// Backend-neutral eventing vocabulary and PAL traits.
pub mod event;
#[path = "hal/hal.rs"]
/// Backend-neutral hardware query vocabulary and PAL HAL traits.
pub mod hal;
#[path = "mem/mem.rs"]
/// Backend-neutral memory vocabulary and unsafe PAL traits.
pub mod mem;
#[path = "sync/sync.rs"]
/// Backend-neutral synchronization vocabulary and low-level PAL traits.
pub mod sync;
#[path = "thread/thread.rs"]
/// Backend-neutral thread vocabulary and low-level PAL traits.
pub mod thread;
