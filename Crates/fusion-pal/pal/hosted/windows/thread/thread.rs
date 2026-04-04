//! Windows fusion-pal thread backend stub.

use crate::contract::pal::runtime::thread::{
    UnsupportedThread,
    UnsupportedThreadHandle,
};

/// Selected thread handle type for Windows builds.
pub type PlatformThreadHandle = UnsupportedThreadHandle;

/// Target-selected thread provider alias for Windows builds.
pub type PlatformThread = UnsupportedThread;

/// Returns the process-wide Windows thread provider handle.
#[must_use]
pub const fn system_thread() -> PlatformThread {
    PlatformThread::new()
}
