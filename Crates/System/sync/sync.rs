//! System-level synchronization primitives built on top of PAL-truthful backends.
//!
//! `fusion-sys::sync` is the policy layer above the PAL sync contracts. It chooses sensible
//! local fallbacks, exposes internal-friendly locking utilities such as [`ThinMutex`], and
//! keeps the no-alloc, no-poison contract surface explicit for higher layers.

mod mutex;
mod spin;
mod thin_mutex;

pub use mutex::*;
pub use spin::*;
pub use thin_mutex::*;

pub use fusion_pal::sys::sync::{
    MutexCaps, MutexSupport, PriorityInheritanceSupport, ProcessScopeSupport, RawMutex,
    RawSemaphore, RecursionSupport, RobustnessSupport, SemaphoreCaps, SemaphoreSupport, SyncBase,
    SyncError, SyncErrorKind, SyncImplementationKind, SyncSupport, TimeoutCaps, WaitCaps,
    WaitOutcome, WaitPrimitive, WaitSupport,
};
