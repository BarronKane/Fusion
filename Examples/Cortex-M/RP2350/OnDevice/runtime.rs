use core::future::Future;
use core::sync::atomic::{AtomicBool, Ordering};

use fusion_pal::sys::runtime_progress::install_runtime_progress_hook;
use fusion_std::thread::{
    CurrentFiberAsyncSingleton,
    CurrentFiberHandle,
    ExecutorError,
    TaskHandle,
};
use fusion_sys::fiber::FiberError;

static RP2350_EXAMPLE_BACKEND: CurrentFiberAsyncSingleton =
    CurrentFiberAsyncSingleton::new().with_fiber_capacity(8);
static RP2350_PROGRESS_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
static RP2350_PROGRESS_HOOK_RUNNING: AtomicBool = AtomicBool::new(false);

fn rp2350_example_progress_hook() {
    if RP2350_PROGRESS_HOOK_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let _ = RP2350_EXAMPLE_BACKEND.drive_once();
    RP2350_PROGRESS_HOOK_RUNNING.store(false, Ordering::Release);
}

fn ensure_progress_hook_installed() {
    if RP2350_PROGRESS_HOOK_INSTALLED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        install_runtime_progress_hook(rp2350_example_progress_hook);
    }
}

pub fn spawn<F, T>(job: F) -> Result<CurrentFiberHandle<T>, FiberError>
where
    F: FnOnce() -> T + Send + 'static,
    T: 'static,
{
    ensure_progress_hook_installed();
    RP2350_EXAMPLE_BACKEND.spawn_fiber(job)
}

pub fn spawn_with_stack<const STACK_BYTES: usize, F, T>(
    job: F,
) -> Result<CurrentFiberHandle<T>, FiberError>
where
    F: FnOnce() -> T + Send + 'static,
    T: 'static,
{
    ensure_progress_hook_installed();
    RP2350_EXAMPLE_BACKEND.spawn_fiber_with_stack::<STACK_BYTES, _, _>(job)
}

pub fn drive_once() -> Result<bool, FiberError> {
    ensure_progress_hook_installed();
    RP2350_EXAMPLE_BACKEND.drive_once()
}

pub fn shutdown_fibers() -> Result<(), FiberError> {
    RP2350_EXAMPLE_BACKEND.shutdown_fibers()
}

pub fn spawn_async<F>(future: F) -> Result<TaskHandle<F::Output>, ExecutorError>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    ensure_progress_hook_installed();
    RP2350_EXAMPLE_BACKEND.spawn_async(future)
}

pub fn spawn_async_with_poll_stack_bytes<F>(
    poll_stack_bytes: usize,
    future: F,
) -> Result<TaskHandle<F::Output>, ExecutorError>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    ensure_progress_hook_installed();
    RP2350_EXAMPLE_BACKEND.spawn_async_with_poll_stack_bytes(poll_stack_bytes, future)
}

pub fn block_on<F>(future: F) -> Result<F::Output, ExecutorError>
where
    F: Future + 'static,
    F::Output: 'static,
{
    ensure_progress_hook_installed();
    RP2350_EXAMPLE_BACKEND.block_on(future)
}

pub fn block_on_with_poll_stack_bytes<F>(
    poll_stack_bytes: usize,
    future: F,
) -> Result<F::Output, ExecutorError>
where
    F: Future + 'static,
    F::Output: 'static,
{
    ensure_progress_hook_installed();
    RP2350_EXAMPLE_BACKEND.block_on_with_poll_stack_bytes(poll_stack_bytes, future)
}
