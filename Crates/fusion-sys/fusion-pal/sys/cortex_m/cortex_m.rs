#[path = "context/context.rs"]
/// Cortex-M bare-metal context backend implementation.
pub mod context;
#[path = "event/event.rs"]
/// Cortex-M bare-metal event backend implementation.
pub mod event;
#[path = "../unsupported/fiber.rs"]
/// Cortex-M hosted-fiber surface (unsupported — fibers are managed directly).
pub mod fiber;
#[path = "hal/hal.rs"]
/// Cortex-M hardware backend implementation.
pub mod hal;
#[path = "mem/mem.rs"]
/// Cortex-M bare-metal memory backend implementation.
pub mod mem;
#[path = "pcu/pcu.rs"]
/// Cortex-M bare-metal programmable-IO backend implementation.
pub mod pcu;
#[path = "power/power.rs"]
/// Cortex-M bare-metal power backend implementation.
pub mod power;
#[path = "sync/sync.rs"]
/// Cortex-M bare-metal synchronization backend implementation.
pub mod sync;
#[path = "thread/thread.rs"]
/// Cortex-M bare-metal thread backend implementation.
pub mod thread;
#[path = "vector/vector.rs"]
/// Cortex-M bare-metal vector-ownership backend implementation.
pub mod vector;
