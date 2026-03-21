#[path = "context/context.rs"]
/// macOS fusion-pal user-space context backend implementation.
pub mod context;
#[path = "event/event.rs"]
/// macOS fusion-pal event backend implementation.
pub mod event;
#[path = "../unsupported/fiber.rs"]
/// macOS hosted-fiber helper surface remains unsupported for now.
pub mod fiber;
#[path = "hal/hal.rs"]
/// macOS fusion-pal hardware backend implementation.
pub mod hal;
#[path = "mem/mem.rs"]
/// macOS fusion-pal memory backend implementation.
pub mod mem;
#[path = "pcu/pcu.rs"]
/// macOS fusion-pal programmable-IO backend implementation.
pub mod pcu;
#[path = "power/power.rs"]
/// macOS fusion-pal power backend implementation.
pub mod power;
#[path = "sync/sync.rs"]
/// macOS fusion-pal synchronization backend implementation.
pub mod sync;
#[path = "thread/thread.rs"]
/// macOS fusion-pal thread backend implementation.
pub mod thread;
#[path = "../unsupported/vector.rs"]
/// macOS fusion-pal vector-ownership surface remains unsupported for now.
pub mod vector;
