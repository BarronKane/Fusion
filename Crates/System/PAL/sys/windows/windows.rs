#[path = "context/context.rs"]
/// Windows PAL user-space context backend implementation.
pub mod context;
#[path = "event/event.rs"]
/// Windows PAL event backend implementation.
pub mod event;
#[path = "hal/hal.rs"]
/// Windows PAL hardware backend implementation.
pub mod hal;
#[path = "mem/mem.rs"]
/// Windows PAL memory backend implementation.
pub mod mem;
#[path = "sync/sync.rs"]
/// Windows PAL synchronization backend implementation.
pub mod sync;
#[path = "thread/thread.rs"]
/// Windows PAL thread backend implementation.
pub mod thread;
