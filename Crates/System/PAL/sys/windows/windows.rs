#[path = "mem/mem.rs"]
/// Windows PAL memory backend implementation.
pub mod mem;
#[path = "sync/sync.rs"]
/// Windows PAL synchronization backend implementation.
pub mod sync;
#[path = "thread/thread.rs"]
/// Windows PAL thread backend implementation.
pub mod thread;
