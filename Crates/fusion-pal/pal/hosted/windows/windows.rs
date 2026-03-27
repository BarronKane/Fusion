#[path = "context/context.rs"]
/// Windows fusion-pal user-space context backend implementation.
pub mod context;
#[path = "event/event.rs"]
/// Windows fusion-pal event backend implementation.
pub mod event;
#[path = "../../unsupported/fiber.rs"]
/// Windows hosted-fiber helper surface remains unsupported for now.
pub mod fiber;
#[path = "hal/hal.rs"]
/// Windows fusion-pal hardware backend implementation.
pub mod hal;
#[path = "mem/mem.rs"]
/// Windows fusion-pal memory backend implementation.
pub mod mem;
#[path = "pcu/pcu.rs"]
/// Windows fusion-pal programmable-IO backend implementation.
pub mod pcu;
#[path = "power/power.rs"]
/// Windows fusion-pal power backend implementation.
pub mod power;
#[path = "sync/sync.rs"]
/// Windows fusion-pal synchronization backend implementation.
pub mod sync;
#[path = "thread/thread.rs"]
/// Windows fusion-pal thread backend implementation.
pub mod thread;
#[path = "../../unsupported/vector.rs"]
/// Windows fusion-pal vector-ownership surface remains unsupported for now.
pub mod vector;
