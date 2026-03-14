//! iOS fusion-pal thread backend stub.

use crate::pal::thread::{UnsupportedThread, UnsupportedThreadHandle};

/// Selected thread handle type for iOS builds.
pub type PlatformThreadHandle = UnsupportedThreadHandle;

/// Target-selected thread provider alias for iOS builds.
pub type PlatformThread = UnsupportedThread;

/// Returns the process-wide iOS thread provider handle.
#[must_use]
pub const fn system_thread() -> PlatformThread {
    PlatformThread::new()
}
