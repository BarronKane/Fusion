#[path = "context/context.rs"]
/// iOS fusion-pal user-space context backend implementation.
pub mod context;
#[path = "event/event.rs"]
/// iOS fusion-pal event backend implementation.
pub mod event;
#[path = "../../unsupported/fiber.rs"]
/// iOS hosted-fiber helper surface remains unsupported for now.
pub mod fiber;
#[path = "hal/hal.rs"]
/// iOS fusion-pal hardware backend implementation.
pub mod hal;
#[path = "mem/mem.rs"]
/// iOS fusion-pal memory backend implementation.
pub mod mem;
#[path = "pcu/pcu.rs"]
/// iOS fusion-pal programmable-IO backend implementation.
pub mod pcu;
#[path = "power/power.rs"]
/// iOS fusion-pal power backend implementation.
pub mod power;
#[path = "sync/sync.rs"]
/// iOS fusion-pal synchronization backend implementation.
pub mod sync;
#[path = "thread/thread.rs"]
/// iOS fusion-pal thread backend implementation.
pub mod thread;
#[path = "../../unsupported/vector.rs"]
/// iOS fusion-pal vector-ownership surface remains unsupported for now.
pub mod vector;
