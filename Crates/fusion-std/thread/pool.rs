//! Domain 1: public carrier thread-pool surface.

use alloc::sync::Arc;
use core::fmt;
use core::mem::{MaybeUninit, align_of, size_of};

use crate::sync::{Mutex as SyncMutex, SyncError, SyncErrorKind};
use fusion_sys::alloc::{AllocRequest, AllocationStrategy, Allocator, Slab};
use fusion_sys::thread::{
    SystemPoolPlacement, SystemResizePolicy, SystemShutdownPolicy, SystemStealBoundary,
    SystemThreadPool, SystemThreadPoolConfig, SystemThreadPoolError, SystemThreadPoolStats,
    SystemWorkItem, ThreadLogicalCpuId, ThreadSchedulerRequest, ThreadStackRequest, ThreadSupport,
    ThreadSystem,
};

/// Public placement strategy for carrier workers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoolPlacement<'a> {
    /// Inherit platform defaults with no runtime-managed affinity.
    Inherit,
    /// Attempt to place one carrier per logical CPU.
    PerCore,
    /// Attempt to place one carrier per package or socket.
    PerPackage,
    /// Pin carriers to an explicit static set of logical CPUs.
    Static(&'a [ThreadLogicalCpuId]),
    /// Allow future orchestration to resize or relocate carriers dynamically.
    Dynamic,
}

impl<'a> From<PoolPlacement<'a>> for SystemPoolPlacement<'a> {
    fn from(value: PoolPlacement<'a>) -> Self {
        match value {
            PoolPlacement::Inherit => Self::Inherit,
            PoolPlacement::PerCore => Self::PerCore,
            PoolPlacement::PerPackage => Self::PerPackage,
            PoolPlacement::Static(cpus) => Self::Static(cpus),
            PoolPlacement::Dynamic => Self::Dynamic,
        }
    }
}

/// Boundary at which work stealing is permitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StealBoundary {
    /// No stealing across worker queues.
    LocalOnly,
    /// Steal only within the same core cluster or shared-cache domain.
    SameCoreCluster,
    /// Steal only within the same package or socket.
    SamePackage,
    /// Steal only within the same NUMA node.
    SameNumaNode,
    /// Allow stealing across the whole pool.
    Global,
}

impl From<StealBoundary> for SystemStealBoundary {
    fn from(value: StealBoundary) -> Self {
        match value {
            StealBoundary::LocalOnly => Self::LocalOnly,
            StealBoundary::SameCoreCluster => Self::SameCoreCluster,
            StealBoundary::SamePackage => Self::SamePackage,
            StealBoundary::SameNumaNode => Self::SameNumaNode,
            StealBoundary::Global => Self::Global,
        }
    }
}

/// Public resize policy for the carrier pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResizePolicy {
    /// Worker count is fixed after startup.
    Fixed,
    /// Worker count may be adjusted only through explicit control calls.
    Manual,
    /// Worker count may be adjusted elastically later.
    Elastic,
}

impl From<ResizePolicy> for SystemResizePolicy {
    fn from(value: ResizePolicy) -> Self {
        match value {
            ResizePolicy::Fixed => Self::Fixed,
            ResizePolicy::Manual => Self::Manual,
            ResizePolicy::Elastic => Self::Elastic,
        }
    }
}

/// Public shutdown policy for a carrier pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShutdownPolicy {
    /// Drain queued work before shutdown completes.
    Drain,
    /// Reject new work and cancel queued-but-not-started items.
    CancelPending,
    /// Tear down at the next safe stop point.
    Immediate,
}

impl From<ShutdownPolicy> for SystemShutdownPolicy {
    fn from(value: ShutdownPolicy) -> Self {
        match value {
            ShutdownPolicy::Drain => Self::Drain,
            ShutdownPolicy::CancelPending => Self::CancelPending,
            ShutdownPolicy::Immediate => Self::Immediate,
        }
    }
}

/// Public carrier-pool configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadPoolConfig<'a> {
    /// Minimum number of carrier workers.
    pub min_threads: usize,
    /// Maximum number of carrier workers.
    pub max_threads: usize,
    /// Placement strategy for workers.
    pub placement: PoolPlacement<'a>,
    /// Stealing boundary between workers.
    pub steal_boundary: StealBoundary,
    /// Whether the pool may resize later.
    pub resize_policy: ResizePolicy,
    /// Shutdown behavior for queued and active work.
    pub shutdown_policy: ShutdownPolicy,
    /// Optional worker-name prefix.
    pub name_prefix: Option<&'a str>,
    /// Stack request applied to carriers.
    pub stack: ThreadStackRequest,
    /// Scheduler request applied to carriers.
    pub scheduler: ThreadSchedulerRequest,
}

impl<'a> ThreadPoolConfig<'a> {
    /// Returns a single-worker deterministic pool configuration.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            min_threads: 1,
            max_threads: 1,
            placement: PoolPlacement::Inherit,
            steal_boundary: StealBoundary::LocalOnly,
            resize_policy: ResizePolicy::Fixed,
            shutdown_policy: ShutdownPolicy::Drain,
            name_prefix: None,
            stack: ThreadStackRequest::new(),
            scheduler: ThreadSchedulerRequest::new(),
        }
    }

    fn to_system(self) -> SystemThreadPoolConfig<'a> {
        SystemThreadPoolConfig {
            min_threads: self.min_threads,
            max_threads: self.max_threads,
            placement: self.placement.into(),
            steal_boundary: self.steal_boundary.into(),
            resize_policy: self.resize_policy.into(),
            shutdown_policy: self.shutdown_policy.into(),
            name_prefix: self.name_prefix,
            stack: self.stack,
            scheduler: self.scheduler,
        }
    }
}

impl Default for ThreadPoolConfig<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// Public snapshot of carrier-pool statistics.
pub type PoolStats = SystemThreadPoolStats;

/// Public carrier-pool error.
pub type ThreadPoolError = SystemThreadPoolError;

const THREAD_POOL_JOB_INLINE_BYTES: usize = 768;
const THREAD_POOL_JOB_SLOT_BYTES: usize = 1024;
const THREAD_POOL_JOB_SLOT_COUNT: usize = 256;

#[derive(Debug)]
struct ThreadPoolShared {
    inner: SyncMutex<Option<SystemThreadPool>>,
    jobs: SyncMutex<Slab<THREAD_POOL_JOB_SLOT_BYTES, THREAD_POOL_JOB_SLOT_COUNT>>,
}

#[repr(C, align(64))]
struct InlineThreadJobBytes {
    bytes: [u8; THREAD_POOL_JOB_INLINE_BYTES],
}

struct InlineThreadJobStorage {
    storage: MaybeUninit<InlineThreadJobBytes>,
    run: Option<unsafe fn(*mut u8)>,
    drop: Option<unsafe fn(*mut u8)>,
    occupied: bool,
}

impl fmt::Debug for InlineThreadJobStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InlineThreadJobStorage")
            .field("occupied", &self.occupied)
            .finish_non_exhaustive()
    }
}

impl InlineThreadJobStorage {
    const fn empty() -> Self {
        Self {
            storage: MaybeUninit::uninit(),
            run: None,
            drop: None,
            occupied: false,
        }
    }

    fn store<F>(&mut self, job: F) -> Result<(), ThreadPoolError>
    where
        F: FnOnce() + Send + 'static,
    {
        if self.occupied {
            return Err(ThreadPoolError::state_conflict());
        }
        if size_of::<F>() > size_of::<InlineThreadJobBytes>()
            || align_of::<F>() > align_of::<InlineThreadJobBytes>()
        {
            return Err(ThreadPoolError::resource_exhausted());
        }

        let ptr = self.storage.as_mut_ptr().cast::<F>();
        // SAFETY: size and alignment were checked above and the storage is currently vacant.
        unsafe { ptr.write(job) };
        self.run = Some(run_inline_thread_job::<F>);
        self.drop = Some(drop_inline_thread_job::<F>);
        self.occupied = true;
        Ok(())
    }

    fn take_runner(&mut self) -> Result<InlineThreadJobRunner, ThreadPoolError> {
        if !self.occupied {
            return Err(ThreadPoolError::state_conflict());
        }

        let run = self
            .run
            .take()
            .ok_or_else(ThreadPoolError::state_conflict)?;
        let drop = self.drop.take();
        self.occupied = false;
        Ok(InlineThreadJobRunner {
            ptr: self.storage.as_mut_ptr().cast::<u8>(),
            run,
            drop,
        })
    }
}

impl Drop for InlineThreadJobStorage {
    fn drop(&mut self) {
        if !self.occupied {
            return;
        }
        if let Some(drop) = self.drop.take() {
            // SAFETY: `occupied` means a valid job is present in storage.
            unsafe { drop(self.storage.as_mut_ptr().cast::<u8>()) };
        }
        self.run = None;
        self.occupied = false;
    }
}

struct InlineThreadJobRunner {
    ptr: *mut u8,
    run: unsafe fn(*mut u8),
    drop: Option<unsafe fn(*mut u8)>,
}

impl InlineThreadJobRunner {
    fn run(mut self) {
        // SAFETY: `take_runner` only produces a runner for initialized storage.
        unsafe { (self.run)(self.ptr) };
        self.drop = None;
    }
}

impl Drop for InlineThreadJobRunner {
    fn drop(&mut self) {
        if let Some(drop) = self.drop.take() {
            // SAFETY: the runner still owns the stored job when `drop` remains present.
            unsafe { drop(self.ptr) };
        }
    }
}

unsafe fn run_inline_thread_job<F>(ptr: *mut u8)
where
    F: FnOnce() + Send + 'static,
{
    // SAFETY: the storage guarantees `ptr` names a valid `F`, and we consume it exactly once.
    let job = unsafe { ptr.cast::<F>().read() };
    job();
}

unsafe fn drop_inline_thread_job<F>(ptr: *mut u8)
where
    F: FnOnce() + Send + 'static,
{
    // SAFETY: the storage guarantees `ptr` names a valid `F` when this drop hook is present.
    unsafe { ptr.cast::<F>().drop_in_place() };
}

#[derive(Debug)]
struct ThreadJobRecord {
    shared: Arc<ThreadPoolShared>,
    allocation: Option<fusion_sys::alloc::AllocResult>,
    job: InlineThreadJobStorage,
}

impl ThreadJobRecord {
    const fn new(
        shared: Arc<ThreadPoolShared>,
        allocation: fusion_sys::alloc::AllocResult,
        job: InlineThreadJobStorage,
    ) -> Self {
        Self {
            shared,
            allocation: Some(allocation),
            job,
        }
    }

    fn run_contained(mut self) {
        if let Ok(runner) = self.job.take_runner() {
            run_inline_job_contained(runner);
        }
        if let Some(allocation) = self.allocation.take() {
            let _ = self
                .shared
                .jobs
                .lock()
                .map_err(thread_pool_error_from_sync)
                .and_then(|slab| {
                    slab.deallocate(allocation)
                        .map_err(thread_pool_error_from_alloc)
                });
        }
    }
}

/// Public carrier thread-pool wrapper.
#[derive(Debug, Clone)]
pub struct ThreadPool {
    shared: Arc<ThreadPoolShared>,
}

impl ThreadPool {
    /// Reports the underlying system-thread support driving the carrier pool.
    #[must_use]
    pub fn support() -> ThreadSupport {
        SystemThreadPool::support(&ThreadSystem::new())
    }

    /// Creates a public carrier thread pool.
    ///
    /// # Errors
    ///
    /// Returns any honest lower-level configuration or support failure.
    pub fn new(config: &ThreadPoolConfig<'_>) -> Result<Self, ThreadPoolError> {
        let inner = SystemThreadPool::new(ThreadSystem::new(), &config.to_system())?;
        let allocator =
            Allocator::<1, 1>::system_default().map_err(thread_pool_error_from_alloc)?;
        let default_domain = allocator
            .default_domain()
            .ok_or_else(ThreadPoolError::state_conflict)?;
        let jobs = allocator
            .slab::<THREAD_POOL_JOB_SLOT_BYTES, THREAD_POOL_JOB_SLOT_COUNT>(default_domain)
            .map_err(thread_pool_error_from_alloc)?;
        Ok(Self {
            shared: Arc::new(ThreadPoolShared {
                inner: SyncMutex::new(Some(inner)),
                jobs: SyncMutex::new(jobs),
            }),
        })
    }

    /// Returns the current pool statistics snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if the internal pool coordination state cannot be observed honestly.
    pub fn stats(&self) -> Result<PoolStats, ThreadPoolError> {
        let guard = self
            .shared
            .inner
            .lock()
            .map_err(thread_pool_error_from_sync)?;
        guard.as_ref().map_or(
            Ok(PoolStats {
                min_threads: 0,
                max_threads: 0,
                active_workers: 0,
                queued_items: 0,
            }),
            SystemThreadPool::stats,
        )
    }

    /// Returns the current active worker count.
    ///
    /// # Errors
    ///
    /// Returns an error if the internal pool coordination state cannot be observed honestly.
    pub fn worker_count(&self) -> Result<usize, ThreadPoolError> {
        Ok(self.stats()?.active_workers)
    }

    /// Submits one raw work item to the carrier pool.
    ///
    /// # Errors
    ///
    /// Returns any honest lower-level submission failure.
    pub fn submit_raw(&self, work: SystemWorkItem) -> Result<(), ThreadPoolError> {
        let guard = self
            .shared
            .inner
            .lock()
            .map_err(thread_pool_error_from_sync)?;
        let Some(inner) = guard.as_ref() else {
            return Err(fusion_sys::thread::ThreadError::state_conflict());
        };
        inner.submit(work)
    }

    /// Submits one `Send` closure to the carrier pool.
    ///
    /// # Errors
    ///
    /// Returns any honest lower-level submission failure.
    pub fn submit<F>(&self, work: F) -> Result<(), ThreadPoolError>
    where
        F: FnOnce() + Send + 'static,
    {
        unsafe fn run_inline_job(context: *mut ()) {
            // SAFETY: the context points at a slab-backed record written by `submit`.
            let record = unsafe { context.cast::<ThreadJobRecord>().read() };
            record.run_contained();
        }

        let mut storage = InlineThreadJobStorage::empty();
        storage.store(work)?;

        let allocation = {
            let slab = self
                .shared
                .jobs
                .lock()
                .map_err(thread_pool_error_from_sync)?;
            slab.allocate(&AllocRequest {
                len: size_of::<ThreadJobRecord>(),
                align: align_of::<ThreadJobRecord>(),
                zeroed: false,
            })
            .map_err(thread_pool_error_from_alloc)?
        };
        let context = allocation.ptr.cast::<ThreadJobRecord>();
        let record = ThreadJobRecord::new(Arc::clone(&self.shared), allocation, storage);
        // SAFETY: the slab allocation reserves enough space for one `ThreadJobRecord` and is
        // uniquely owned until the worker consumes and recycles it.
        unsafe { context.as_ptr().write(record) };
        let item = SystemWorkItem::new(run_inline_job, context.cast::<()>().as_ptr());

        match self.submit_raw(item) {
            Ok(()) => Ok(()),
            Err(error) => {
                // SAFETY: submission failed, so no worker can observe the record. Read it back,
                // run its normal drop/recycle path, and return the original error.
                unsafe { context.as_ptr().read().run_contained() };
                Err(error)
            }
        }
    }

    /// Shuts the carrier pool down according to its configured shutdown policy.
    ///
    /// # Errors
    ///
    /// Returns any honest lower-level shutdown failure.
    #[allow(clippy::needless_pass_by_value)]
    pub fn shutdown(self) -> Result<(), ThreadPoolError> {
        let Some(inner) = self
            .shared
            .inner
            .lock()
            .map_err(thread_pool_error_from_sync)?
            .take()
        else {
            return Ok(());
        };
        inner.shutdown()
    }
}

fn run_inline_job_contained(job: InlineThreadJobRunner) {
    #[cfg(feature = "std")]
    {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        let _ = catch_unwind(AssertUnwindSafe(|| job.run()));
    }

    #[cfg(not(feature = "std"))]
    {
        job.run();
    }
}

const fn thread_pool_error_from_sync(error: SyncError) -> ThreadPoolError {
    match error.kind {
        SyncErrorKind::Unsupported => ThreadPoolError::unsupported(),
        SyncErrorKind::Invalid | SyncErrorKind::Overflow => ThreadPoolError::invalid(),
        SyncErrorKind::Busy | SyncErrorKind::PermissionDenied | SyncErrorKind::Platform(_) => {
            ThreadPoolError::state_conflict()
        }
    }
}

const fn thread_pool_error_from_alloc(error: fusion_sys::alloc::AllocError) -> ThreadPoolError {
    match error.kind {
        fusion_sys::alloc::AllocErrorKind::Unsupported => ThreadPoolError::unsupported(),
        fusion_sys::alloc::AllocErrorKind::InvalidRequest
        | fusion_sys::alloc::AllocErrorKind::InvalidDomain => ThreadPoolError::invalid(),
        fusion_sys::alloc::AllocErrorKind::PolicyDenied => ThreadPoolError::state_conflict(),
        fusion_sys::alloc::AllocErrorKind::MetadataExhausted
        | fusion_sys::alloc::AllocErrorKind::CapacityExhausted
        | fusion_sys::alloc::AllocErrorKind::OutOfMemory => ThreadPoolError::resource_exhausted(),
        fusion_sys::alloc::AllocErrorKind::ResourceFailure(_)
        | fusion_sys::alloc::AllocErrorKind::PoolFailure(_)
        | fusion_sys::alloc::AllocErrorKind::SynchronizationFailure(_) => {
            ThreadPoolError::state_conflict()
        }
    }
}

pub use fusion_sys::thread::WorkerId;
