//! Public runtime-facing Fusion APIs.
//!
//! `fusion-std` is the ergonomic public layer above `fusion-sys`. It owns the user-facing
//! runtime vocabulary, profiles, and orchestration surfaces while keeping platform truth
//! and low-level system contracts below the `fusion-sys` boundary where they belong.

#![no_std]

extern crate alloc;
#[cfg(any(test, feature = "std"))]
extern crate std;

#[cfg(all(not(feature = "std"), panic = "unwind"))]
compile_error!("fusion-std without feature `std` requires panic = \"abort\".");

/// Public synchronization facade layered over the canonical `fusion-sys` primitives.
#[path = "sync/sync.rs"]
pub mod sync;
/// Public thread, runtime, task, and executor surfaces.
#[path = "thread/thread.rs"]
pub mod thread;
