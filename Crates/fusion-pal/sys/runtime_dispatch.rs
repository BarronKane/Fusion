//! Backend-owned deferred runtime dispatch broker.
//!
//! Higher layers can register courier/runtime callbacks here without learning how the selected
//! backend actually schedules them. On Cortex-M this is realized through reserved PendSV
//! dispatch. Backends without a truthful deferred-dispatch substrate fall back to synchronous
//! local dispatch so callers never have to regress to manual pump vocabulary just to stay alive.

use core::sync::atomic::{
    AtomicBool,
    AtomicUsize,
    Ordering,
};

use crate::contract::pal::{
    HardwareError,
};
use crate::sys::vector::{
    ensure_runtime_reserved_wake_vectors_best_effort,
    request_reserved_pendsv_dispatch,
    VectorErrorKind,
};

const RUNTIME_DISPATCH_REGISTRY_CAPACITY: usize = 32;

/// Opaque runtime-dispatch cookie returned by the PAL broker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RuntimeDispatchCookie(pub u32);

type RuntimeDispatchCallback = fn(usize);

static NEXT_RUNTIME_DISPATCH_COOKIE: AtomicUsize = AtomicUsize::new(1);
static RUNTIME_DISPATCH_CALLBACKS: [AtomicUsize; RUNTIME_DISPATCH_REGISTRY_CAPACITY] =
    [const { AtomicUsize::new(0) }; RUNTIME_DISPATCH_REGISTRY_CAPACITY];
static RUNTIME_DISPATCH_CONTEXTS: [AtomicUsize; RUNTIME_DISPATCH_REGISTRY_CAPACITY] =
    [const { AtomicUsize::new(0) }; RUNTIME_DISPATCH_REGISTRY_CAPACITY];
static RUNTIME_DISPATCH_PENDING: [AtomicUsize; RUNTIME_DISPATCH_REGISTRY_CAPACITY] =
    [const { AtomicUsize::new(0) }; RUNTIME_DISPATCH_REGISTRY_CAPACITY];
static RUNTIME_DISPATCH_RUNNING: AtomicBool = AtomicBool::new(false);

/// Registers one runtime-dispatch callback with one opaque caller context.
///
/// # Errors
///
/// Returns `ResourceExhausted` when the fixed broker registry is full.
pub fn register_runtime_dispatch_callback(
    callback: RuntimeDispatchCallback,
    context: usize,
) -> Result<RuntimeDispatchCookie, HardwareError> {
    let cookie = NEXT_RUNTIME_DISPATCH_COOKIE.fetch_add(1, Ordering::AcqRel);
    if cookie == 0 {
        return Err(HardwareError::resource_exhausted());
    }
    let index = cookie
        .checked_sub(1)
        .filter(|index| *index < RUNTIME_DISPATCH_REGISTRY_CAPACITY)
        .ok_or_else(HardwareError::resource_exhausted)?;

    let callback_slot = &RUNTIME_DISPATCH_CALLBACKS[index];
    if callback_slot
        .compare_exchange(0, callback as usize, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(HardwareError::state_conflict());
    }
    RUNTIME_DISPATCH_CONTEXTS[index].store(context, Ordering::Release);
    Ok(RuntimeDispatchCookie(cookie as u32))
}

/// Unregisters one previously registered runtime-dispatch callback.
///
/// # Errors
///
/// Returns `Invalid` for malformed cookies and `StateConflict` when the cookie is not bound.
pub fn unregister_runtime_dispatch_callback(
    cookie: RuntimeDispatchCookie,
) -> Result<(), HardwareError> {
    let index = runtime_dispatch_cookie_index(cookie)?;
    let previous = RUNTIME_DISPATCH_CALLBACKS[index].swap(0, Ordering::AcqRel);
    if previous == 0 {
        return Err(HardwareError::state_conflict());
    }
    RUNTIME_DISPATCH_CONTEXTS[index].store(0, Ordering::Release);
    RUNTIME_DISPATCH_PENDING[index].store(0, Ordering::Release);
    Ok(())
}

/// Requests one deferred runtime-dispatch callback execution.
///
/// When the backend can surface a truthful deferred-dispatch substrate, this schedules the
/// callback there. Otherwise it runs the pending dispatch synchronously on the current caller
/// thread without leaking that distinction upward.
///
/// # Errors
///
/// Returns `Invalid` for malformed cookies and `StateConflict` when the cookie is not bound.
pub fn request_runtime_dispatch(cookie: RuntimeDispatchCookie) -> Result<(), HardwareError> {
    let index = runtime_dispatch_cookie_index(cookie)?;
    if RUNTIME_DISPATCH_CALLBACKS[index].load(Ordering::Acquire) == 0 {
        return Err(HardwareError::state_conflict());
    }
    RUNTIME_DISPATCH_PENDING[index].store(1, Ordering::Release);
    schedule_runtime_dispatch();
    Ok(())
}

/// Runs one batch of currently pending runtime-dispatch callbacks.
///
/// This is the backend-facing path invoked from reserved deferred-dispatch handlers such as
/// Cortex-M PendSV. Callers outside the PAL should use [`request_runtime_dispatch()`] instead.
pub fn dispatch_pending_runtime_callbacks() {
    if RUNTIME_DISPATCH_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    loop {
        dispatch_pending_runtime_callback_batch();

        if !runtime_dispatch_pending_any() {
            break;
        }
    }

    RUNTIME_DISPATCH_RUNNING.store(false, Ordering::Release);

    if runtime_dispatch_pending_any() {
        schedule_runtime_dispatch();
    }
}

fn runtime_dispatch_cookie_index(cookie: RuntimeDispatchCookie) -> Result<usize, HardwareError> {
    if cookie.0 == 0 {
        return Err(HardwareError::invalid());
    }
    let index = usize::try_from(cookie.0 - 1).map_err(|_| HardwareError::invalid())?;
    if index >= RUNTIME_DISPATCH_REGISTRY_CAPACITY {
        return Err(HardwareError::invalid());
    }
    Ok(index)
}

fn runtime_dispatch_pending_any() -> bool {
    RUNTIME_DISPATCH_PENDING
        .iter()
        .any(|pending| pending.load(Ordering::Acquire) != 0)
}

fn dispatch_pending_runtime_callback_batch() {
    for index in 0..RUNTIME_DISPATCH_REGISTRY_CAPACITY {
        if RUNTIME_DISPATCH_PENDING[index].swap(0, Ordering::AcqRel) == 0 {
            continue;
        }

        let callback = RUNTIME_DISPATCH_CALLBACKS[index].load(Ordering::Acquire);
        if callback == 0 {
            continue;
        }
        let context = RUNTIME_DISPATCH_CONTEXTS[index].load(Ordering::Acquire);

        // SAFETY: registry slots only store callbacks registered through
        // `register_runtime_dispatch_callback()`, and zero is reserved as the empty sentinel.
        let callback: RuntimeDispatchCallback =
            unsafe { core::mem::transmute::<usize, RuntimeDispatchCallback>(callback) };
        callback(context);
    }
}

fn dispatch_pending_runtime_callbacks_once() {
    if RUNTIME_DISPATCH_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    dispatch_pending_runtime_callback_batch();
    RUNTIME_DISPATCH_RUNNING.store(false, Ordering::Release);
}

fn schedule_runtime_dispatch() {
    ensure_runtime_reserved_wake_vectors_best_effort();
    match request_reserved_pendsv_dispatch() {
        Ok(()) => {}
        Err(error) if error.kind() == VectorErrorKind::Unsupported => {
            dispatch_pending_runtime_callbacks_once();
        }
        Err(_) => {
            dispatch_pending_runtime_callbacks_once();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{
        AtomicUsize,
        Ordering,
    };

    static TEST_HITS: AtomicUsize = AtomicUsize::new(0);

    fn test_callback(context: usize) {
        TEST_HITS.fetch_add(context, Ordering::AcqRel);
    }

    #[test]
    fn requested_runtime_dispatch_runs_registered_callback() {
        TEST_HITS.store(0, Ordering::Release);
        let cookie = register_runtime_dispatch_callback(test_callback, 1)
            .expect("runtime-dispatch callback should register");
        request_runtime_dispatch(cookie).expect("runtime-dispatch request should succeed");
        assert_eq!(TEST_HITS.load(Ordering::Acquire), 1);
        unregister_runtime_dispatch_callback(cookie)
            .expect("runtime-dispatch callback should unregister");
    }
}
