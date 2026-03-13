//! macOS PAL thread backend stub.

use crate::pal::thread::{UnsupportedThread, UnsupportedThreadHandle};

/// Selected thread handle type for macOS builds.
pub type PlatformThreadHandle = UnsupportedThreadHandle;

/// Target-selected thread provider alias for macOS builds.
pub type PlatformThread = UnsupportedThread;

/// Returns the process-wide macOS thread provider handle.
#[must_use]
pub const fn system_thread() -> PlatformThread {
    PlatformThread::new()
}
