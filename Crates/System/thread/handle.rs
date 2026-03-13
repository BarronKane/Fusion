//! Owned system thread handles.

use fusion_pal::sys::thread::PlatformThreadHandle;

/// Owned thread handle wrapped by `fusion-sys`.
#[derive(Debug)]
pub struct ThreadHandle {
    pub(crate) inner: PlatformThreadHandle,
}

impl ThreadHandle {
    pub(crate) const fn new(inner: PlatformThreadHandle) -> Self {
        Self { inner }
    }
}

unsafe impl Send for ThreadHandle {}
unsafe impl Sync for ThreadHandle {}
