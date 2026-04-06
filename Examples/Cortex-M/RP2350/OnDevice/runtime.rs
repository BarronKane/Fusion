use core::future::Future;
use core::time::Duration;

use fusion_std::thread::{
    CurrentFiberAsyncSingleton,
    CurrentFiberHandle,
    ExecutorError,
    TaskHandle,
};
use fusion_sys::fiber::FiberError;
use fusion_sys::thread::{
    system_monotonic_time,
    system_thread,
};

static RP2350_EXAMPLE_BACKEND: CurrentFiberAsyncSingleton =
    CurrentFiberAsyncSingleton::new().with_fiber_capacity(8);

pub fn spawn<F, T>(job: F) -> Result<CurrentFiberHandle<T>, FiberError>
where
    F: FnOnce() -> T + Send + 'static,
    T: 'static,
{
    RP2350_EXAMPLE_BACKEND.spawn_fiber(job)
}

pub fn spawn_with_stack<const STACK_BYTES: usize, F, T>(
    job: F,
) -> Result<CurrentFiberHandle<T>, FiberError>
where
    F: FnOnce() -> T + Send + 'static,
    T: 'static,
{
    RP2350_EXAMPLE_BACKEND.spawn_fiber_with_stack::<STACK_BYTES, _, _>(job)
}

pub fn shutdown_fibers() -> Result<(), FiberError> {
    RP2350_EXAMPLE_BACKEND.shutdown_fibers()
}

pub fn spawn_async<F>(future: F) -> Result<TaskHandle<F::Output>, ExecutorError>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
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
    RP2350_EXAMPLE_BACKEND.spawn_async_with_poll_stack_bytes(poll_stack_bytes, future)
}

pub fn block_on<F>(future: F) -> Result<F::Output, ExecutorError>
where
    F: Future + 'static,
    F::Output: 'static,
{
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
    RP2350_EXAMPLE_BACKEND.block_on_with_poll_stack_bytes(poll_stack_bytes, future)
}

pub fn wait_for_runtime_progress() {
    if system_thread().yield_now().is_ok() {
        return;
    }
    let _ = system_monotonic_time().sleep_for(Duration::from_micros(250));
}
