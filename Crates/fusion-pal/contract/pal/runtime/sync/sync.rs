//! Backend-neutral synchronization vocabulary and low-level fusion-pal contracts.
//!
//! The sync fusion-pal controls the contract surface for mutexes, semaphores, and higher-level
//! blocking primitives without pretending that every operating system provides identical scheduler
//! semantics. Raw atomic wait/wake belongs to the atomic substrate, not here. Anything involving
//! timeout clocks, priority inheritance, robustness, or process sharing is modeled explicitly as
//! support metadata rather than a baseline promise.

mod caps;
mod error;
mod mutex;
mod once;
mod rwlock;
mod semaphore;
mod unsupported;

pub use caps::*;
pub use error::*;
pub use mutex::*;
pub use once::*;
pub use rwlock::*;
pub use semaphore::*;
pub use unsupported::*;

/// Backend-selected synchronization support surface.
pub trait SyncBaseContract {
    /// Reports the synchronization primitives and semantics this backend can support.
    fn support(&self) -> SyncSupport;
}
