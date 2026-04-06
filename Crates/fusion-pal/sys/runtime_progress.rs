//! Best-effort cooperative runtime progress hook for long synchronous backend work.
//!
//! Some bare-metal bring-up paths are necessarily synchronous and may monopolize the current
//! thread for milliseconds while still wanting other cooperative services to breathe. This hook
//! gives higher layers one tiny escape hatch: they may register one best-effort local progress
//! callback, and backend code may invoke it opportunistically during long loops.
//!
//! This is intentionally small and policy-free:
//! - no allocation
//! - no scheduler law
//! - no platform-specific cfg leakage above `fusion-pal`
//! - if nobody installs a hook, it is a no-op

use core::sync::atomic::{
    AtomicUsize,
    Ordering,
};

type ProgressHook = fn();

static RUNTIME_PROGRESS_HOOK: AtomicUsize = AtomicUsize::new(0);

/// Installs or replaces the current best-effort progress hook.
pub fn install_runtime_progress_hook(hook: ProgressHook) {
    RUNTIME_PROGRESS_HOOK.store(hook as usize, Ordering::Release);
}

/// Clears the current best-effort progress hook.
pub fn clear_runtime_progress_hook() {
    RUNTIME_PROGRESS_HOOK.store(0, Ordering::Release);
}

/// Runs the current best-effort progress hook when one is installed.
pub fn run_runtime_progress_hook() {
    let hook = RUNTIME_PROGRESS_HOOK.load(Ordering::Acquire);
    if hook == 0 {
        return;
    }

    // SAFETY: only `fn()` pointers installed through `install_runtime_progress_hook()` are stored
    // in this slot, and `0` is reserved as the empty sentinel.
    let hook: ProgressHook = unsafe { core::mem::transmute::<usize, ProgressHook>(hook) };
    hook();
}
