//! fusion-sys-level synchronization primitives built on top of fusion-pal-truthful backends.
//!
//! `fusion-sys::sync` is the policy layer above the fusion-pal sync contracts. It chooses sensible
//! local fallbacks, exposes internal-friendly locking utilities such as [`ThinMutex`], and
//! keeps the no-alloc, no-poison contract surface explicit for higher layers.

mod mutex;
mod once;
mod rwlock;
mod semaphore;
mod spin;
mod thin_mutex;

pub use mutex::*;
pub use once::*;
pub use rwlock::*;
pub use semaphore::*;
pub use spin::*;
pub use thin_mutex::*;

pub use fusion_pal::sys::sync::{
    MutexCaps, MutexSupport, OnceBeginResult, OnceCaps, OnceState, OnceSupport,
    PriorityInheritanceSupport, ProcessScopeSupport, RawMutex, RawOnce, RawRwLock, RawSemaphore,
    RecursionSupport, RobustnessSupport, RwLockCaps, RwLockFairnessSupport, RwLockSupport,
    SemaphoreCaps, SemaphoreSupport, SyncBase, SyncError, SyncErrorKind, SyncFallbackKind,
    SyncImplementationKind, SyncSupport, TimeoutCaps, WaitCaps, WaitOutcome, WaitPrimitive,
    WaitSupport,
};
