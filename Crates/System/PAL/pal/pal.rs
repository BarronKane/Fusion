#[path = "mem/mem.rs"]
/// Backend-neutral memory vocabulary and unsafe PAL traits.
pub mod mem;
#[path = "sync/sync.rs"]
/// Backend-neutral synchronization vocabulary and low-level PAL traits.
pub mod sync;
#[path = "thread/thread.rs"]
/// Backend-neutral thread vocabulary and low-level PAL traits.
pub mod thread;
