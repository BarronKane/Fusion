//! Runtime capability probing for Darwin-hosted backends.
//!
//! This module probes selected platform behaviors once per process and caches the results.
//! Probes are intentionally conservative and drive truthful support reporting elsewhere.

use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

/// Snapshot of Darwin runtime-probed capability facts used by hosted macOS backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DarwinRuntimeCapabilities {
    /// `madvise(MADV_FREE)` accepted on this runtime.
    pub madv_free: bool,
    /// Unnamed POSIX semaphores via `sem_init` are usable.
    pub unnamed_posix_semaphore: bool,
}

impl DarwinRuntimeCapabilities {
    /// Performs one-shot runtime probing of selected Darwin capability facts.
    fn probe() -> Self {
        Self {
            madv_free: probe_madv_free(),
            unnamed_posix_semaphore: probe_unnamed_posix_semaphore(),
        }
    }
}

const CAPS_STATE_UNINIT: u8 = 0;
const CAPS_STATE_INITING: u8 = 1;
const CAPS_STATE_READY: u8 = 2;

const CAPS_FLAG_MADV_FREE: u32 = 1 << 0;
const CAPS_FLAG_UNNAMED_POSIX_SEMAPHORE: u32 = 1 << 1;

/// One-time runtime probe state machine for this process.
static DARWIN_CAPS_STATE: AtomicU8 = AtomicU8::new(CAPS_STATE_UNINIT);
/// Bit-packed runtime capability facts derived from probing.
static DARWIN_CAPS_FLAGS: AtomicU32 = AtomicU32::new(0);

/// Returns the cached Darwin runtime capability snapshot for this process.
#[must_use]
pub fn runtime_capabilities() -> DarwinRuntimeCapabilities {
    loop {
        match DARWIN_CAPS_STATE.load(Ordering::Acquire) {
            CAPS_STATE_READY => return decode_caps(DARWIN_CAPS_FLAGS.load(Ordering::Acquire)),
            CAPS_STATE_UNINIT => {
                if DARWIN_CAPS_STATE
                    .compare_exchange(
                        CAPS_STATE_UNINIT,
                        CAPS_STATE_INITING,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    let caps = DarwinRuntimeCapabilities::probe();
                    DARWIN_CAPS_FLAGS.store(encode_caps(caps), Ordering::Release);
                    DARWIN_CAPS_STATE.store(CAPS_STATE_READY, Ordering::Release);
                    return caps;
                }
            }
            // Another thread is probing; wait for it to publish the final snapshot.
            _ => core::hint::spin_loop(),
        }
    }
}

const fn encode_caps(caps: DarwinRuntimeCapabilities) -> u32 {
    let mut flags = 0_u32;
    if caps.madv_free {
        flags |= CAPS_FLAG_MADV_FREE;
    }
    if caps.unnamed_posix_semaphore {
        flags |= CAPS_FLAG_UNNAMED_POSIX_SEMAPHORE;
    }
    flags
}

const fn decode_caps(flags: u32) -> DarwinRuntimeCapabilities {
    DarwinRuntimeCapabilities {
        madv_free: (flags & CAPS_FLAG_MADV_FREE) != 0,
        unnamed_posix_semaphore: (flags & CAPS_FLAG_UNNAMED_POSIX_SEMAPHORE) != 0,
    }
}

fn page_size() -> Option<usize> {
    // Darwin guarantees positive page size when `_SC_PAGESIZE` is supported.
    let raw = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    usize::try_from(raw).ok().filter(|size| *size != 0)
}

fn probe_madv_free() -> bool {
    let Some(page) = page_size() else {
        return false;
    };

    let ptr = unsafe {
        libc::mmap(
            core::ptr::null_mut(),
            page,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANON,
            -1,
            0,
        )
    };

    if ptr == libc::MAP_FAILED {
        return false;
    }

    // Probe acceptance only; this does not claim semantic equivalence with other kernels.
    let madvise_ok = unsafe { libc::madvise(ptr, page, libc::MADV_FREE) == 0 };
    let _ = unsafe { libc::munmap(ptr, page) };
    madvise_ok
}

fn probe_unnamed_posix_semaphore() -> bool {
    let mut sem = MaybeUninit::<libc::sem_t>::uninit();
    #[allow(deprecated)]
    let init_ok = unsafe { libc::sem_init(sem.as_mut_ptr(), 0, 1) == 0 };
    if !init_ok {
        return false;
    }

    let sem_ptr = sem.as_mut_ptr();
    #[allow(deprecated)]
    // Report usable only if lifecycle setup and teardown both succeed.
    let destroy_ok = unsafe { libc::sem_destroy(sem_ptr) == 0 };
    destroy_ok
}

#[cfg(all(test, feature = "std", not(target_os = "none")))]
mod tests {
    extern crate std;

    use self::std::thread;
    use super::*;

    #[test]
    fn runtime_capabilities_snapshot_is_stable_across_calls() {
        let first = runtime_capabilities();
        let second = runtime_capabilities();
        assert_eq!(first, second);
    }

    #[test]
    fn runtime_capabilities_are_consistent_across_threads() {
        let expected = runtime_capabilities();
        let mut workers = self::std::vec::Vec::new();

        for _ in 0..8 {
            workers.push(thread::spawn(runtime_capabilities));
        }

        for worker in workers {
            let observed = worker.join().expect("probe worker should complete");
            assert_eq!(observed, expected);
        }
    }
}
