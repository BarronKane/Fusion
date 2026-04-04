#![cfg(all(feature = "std", not(target_os = "none")))]

#[path = "alloc/alloc.rs"]
mod alloc;
#[path = "event/event.rs"]
mod event;
#[path = "fiber/fiber.rs"]
mod fiber;
#[path = "mem/mem.rs"]
mod mem;
#[path = "power/power.rs"]
mod power;
#[path = "sync/sync.rs"]
mod sync;
#[path = "thread/thread.rs"]
mod thread;
