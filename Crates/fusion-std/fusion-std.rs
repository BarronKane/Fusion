//! Public runtime-facing Fusion APIs.
//!
//! `fusion-std` is the ergonomic public layer above `fusion-sys`. It owns the user-facing
//! runtime vocabulary, profiles, and orchestration surfaces while keeping platform truth
//! and low-level system contracts below the `fusion-sys` boundary where they belong.

/// Public synchronization facade layered over the canonical `fusion-sys` primitives.
#[path = "sync/sync.rs"]
pub mod sync;
/// Public thread, runtime, task, and executor surfaces.
#[path = "thread/thread.rs"]
pub mod thread;
