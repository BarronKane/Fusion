use core::cell::UnsafeCell;
use core::future::Future;
use core::hint::spin_loop;
use core::mem::MaybeUninit;
use core::num::NonZeroUsize;
use core::sync::atomic::{
    AtomicU8,
    Ordering,
};
use core::time::Duration;

use fusion_firmware::sys::hal::runtime::{
    MAIN_CONTEXT_ID,
    MAIN_COURIER_ID,
    bootstrap_root_execution as firmware_bootstrap_root_execution,
};
use fusion_std::thread::{
    CurrentAsyncRuntime,
    ExecutorConfig,
    ExecutorError,
    FiberPoolConfig,
    GreenHandle,
    GreenPool,
    GreenReactorPolicy,
    PoolPlacement,
    TaskHandle,
    ThreadPool,
    ThreadPoolConfig,
    generated_default_fiber_stack_bytes,
};
use fusion_sys::alloc::ExtentLease;
use fusion_sys::fiber::FiberError;
use fusion_sys::thread::{
    CarrierWorkloadProfile,
    RuntimeBackingError,
    RuntimeBackingErrorKind,
    allocate_owned_runtime_slab,
    system_monotonic_time,
    system_thread,
    uses_explicit_bound_runtime_backing,
};

const BACKEND_UNINITIALIZED: u8 = 0;
const BACKEND_RUNNING: u8 = 1;
const BACKEND_READY: u8 = 2;

const RP2350_EXAMPLE_FIBER_CAPACITY: usize = 16;
const RP2350_EXAMPLE_FIBER_GROWTH_CHUNK: usize = 4;
const RP2350_EXAMPLE_ASYNC_CAPACITY: usize = 8;

struct CarrierRuntimeBackend {
    _carrier: ThreadPool,
    fibers: GreenPool,
    async_runtime: CurrentAsyncRuntime,
    _async_slab_owner: Option<ExtentLease>,
}

struct RuntimeBackendSlot {
    state: AtomicU8,
    value: UnsafeCell<MaybeUninit<CarrierRuntimeBackend>>,
}

impl RuntimeBackendSlot {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(BACKEND_UNINITIALIZED),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    fn get(&self) -> &'static CarrierRuntimeBackend {
        loop {
            match self.state.load(Ordering::Acquire) {
                BACKEND_READY => {
                    return unsafe { &*(*self.value.get()).as_ptr() };
                }
                BACKEND_UNINITIALIZED => {
                    if self
                        .state
                        .compare_exchange(
                            BACKEND_UNINITIALIZED,
                            BACKEND_RUNNING,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_err()
                    {
                        continue;
                    }

                    let backend = match build_backend() {
                        Ok(backend) => backend,
                        Err(error) => {
                            self.state.store(BACKEND_UNINITIALIZED, Ordering::Release);
                            panic!("rp2350 example runtime should initialize: {error}");
                        }
                    };

                    unsafe { (*self.value.get()).write(backend) };
                    self.state.store(BACKEND_READY, Ordering::Release);
                    return unsafe { &*(*self.value.get()).as_ptr() };
                }
                BACKEND_RUNNING => spin_loop(),
                _ => spin_loop(),
            }
        }
    }
}

unsafe impl Sync for RuntimeBackendSlot {}

static RP2350_EXAMPLE_BACKEND: RuntimeBackendSlot = RuntimeBackendSlot::new();

fn build_backend() -> Result<CarrierRuntimeBackend, &'static str> {
    firmware_bootstrap_root_execution().map_err(|_| "root execution should bootstrap")?;

    let mut carrier_config = ThreadPoolConfig::new()
        .with_spawned_carrier_profile(CarrierWorkloadProfile::GeneralPurpose, true)
        .map_err(|_| "carrier profile should resolve")?;
    carrier_config.placement = PoolPlacement::PerCore;
    carrier_config.name_prefix = Some("rp2350-example");
    let carrier = ThreadPool::new(&carrier_config).map_err(|_| "carrier pool should build")?;

    let fiber_stack_size = NonZeroUsize::new(
        generated_default_fiber_stack_bytes()
            .map_err(|_| "generated default fiber stack should resolve")?,
    )
    .ok_or("fiber stack size should be non-zero")?;
    let fiber_config =
        FiberPoolConfig::fixed_growing(
            fiber_stack_size,
            RP2350_EXAMPLE_FIBER_CAPACITY,
            RP2350_EXAMPLE_FIBER_GROWTH_CHUNK,
        )
        .map_err(|_| "fiber config should be valid")?
        .with_guard_pages(0)
        .with_fcfs_steal_locality(fusion_std::thread::CarrierSpawnLocalityPolicy::SameCore)
        .with_reactor_policy(GreenReactorPolicy::Disabled)
        .with_courier_id(MAIN_COURIER_ID)
        .with_context_id(MAIN_CONTEXT_ID);
    let fibers = GreenPool::new(&fiber_config, &carrier).map_err(|_| "fiber pool should build")?;

    let (async_runtime, async_slab_owner) =
        build_async_runtime().map_err(|_| "async runtime should build")?;

    Ok(CarrierRuntimeBackend {
        _carrier: carrier,
        fibers,
        async_runtime,
        _async_slab_owner: async_slab_owner,
    })
}

fn build_async_runtime() -> Result<(CurrentAsyncRuntime, Option<ExtentLease>), ExecutorError> {
    let config = ExecutorConfig::new()
        .with_capacity(RP2350_EXAMPLE_ASYNC_CAPACITY)
        .with_courier_id(MAIN_COURIER_ID)
        .with_context_id(MAIN_CONTEXT_ID);

    if uses_explicit_bound_runtime_backing() {
        let layout = CurrentAsyncRuntime::backing_plan(config)?;
        let combined = layout.combined_eager()?;
        if let Some(slab) = allocate_owned_runtime_slab(combined.slab.bytes, combined.slab.align)
            .map_err(executor_error_from_runtime_backing)?
        {
            let runtime = CurrentAsyncRuntime::from_bound_slab(config, slab.handle)?;
            return Ok((runtime, Some(slab.lease)));
        }
    }

    Ok((CurrentAsyncRuntime::with_executor_config(config), None))
}

const fn executor_error_from_runtime_backing(error: RuntimeBackingError) -> ExecutorError {
    match error.kind() {
        RuntimeBackingErrorKind::Unsupported => ExecutorError::Unsupported,
        RuntimeBackingErrorKind::Invalid => {
            ExecutorError::Sync(fusion_std::sync::SyncErrorKind::Invalid)
        }
        RuntimeBackingErrorKind::StateConflict => {
            ExecutorError::Sync(fusion_std::sync::SyncErrorKind::Busy)
        }
        RuntimeBackingErrorKind::ResourceExhausted => {
            ExecutorError::Sync(fusion_std::sync::SyncErrorKind::Overflow)
        }
    }
}

fn backend() -> &'static CarrierRuntimeBackend {
    RP2350_EXAMPLE_BACKEND.get()
}

pub fn ensure_runtime_ready() {
    let _ = backend();
}

pub fn request_runtime_dispatch() {}

pub fn spawn<F, T>(job: F) -> Result<GreenHandle<T>, FiberError>
where
    F: FnOnce() -> T + Send + 'static,
    T: 'static,
{
    backend().fibers.spawn(job)
}

pub fn shutdown_fibers() -> Result<(), FiberError> {
    backend().fibers.shutdown()
}

pub fn spawn_async<F>(future: F) -> Result<TaskHandle<F::Output>, ExecutorError>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    backend().async_runtime.spawn(future)
}

pub fn block_on<F>(future: F) -> Result<F::Output, ExecutorError>
where
    F: Future + 'static,
    F::Output: 'static,
{
    backend().async_runtime.block_on(future)
}

pub fn wait_for_runtime_progress() {
    if system_thread().yield_now().is_ok() {
        return;
    }
    let _ = system_monotonic_time().sleep_for(Duration::from_micros(250));
}
