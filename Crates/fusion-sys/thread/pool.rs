//! Low-level bounded carrier-pool primitive.
//!
//! This pool stays intentionally narrow:
//! - fixed worker count only for now
//! - bounded raw work queue
//! - deterministic shutdown policy over the queued work already admitted
//! - no hidden allocation
//!
//! Higher layers can build richer closure-, future-, or graph-facing dispatch on top of this
//! raw substrate once they are ready to own the extra state and lifetime machinery.

use core::array;
use core::cell::UnsafeCell;
use core::fmt;

use fusion_pal::contract::pal::HardwareTopologyQueryContract as _;
use fusion_pal::sys::cpu::system_cpu;
use fusion_pal::sys::thread::{
    ThreadConstraintMode,
    ThreadMigrationPolicy,
    ThreadPlacementPhase,
    ThreadPlacementTarget,
    ThreadStartMode,
};

use crate::sync::{
    OnceInitError,
    OnceLock,
    Semaphore,
    SyncError,
    SyncErrorKind,
    ThinMutex,
};
use crate::thread::handle::ThreadHandle;
use super::{
    CarrierObservation,
    CarrierSpawnLocalityPolicy,
    RawThreadEntry,
    ThreadConfig,
    ThreadCoreClassId,
    ThreadError,
    ThreadErrorKind,
    ThreadLifecycleCaps,
    ThreadLogicalCpuId,
    ThreadSchedulerRequest,
    ThreadStackRequest,
    ThreadSupport,
    ThreadSystem,
    carrier_spawn_locality_rank,
    system_carrier,
};

#[cfg(feature = "sys-cortex-m")]
const MAX_POOL_SLOTS: usize = 1;
#[cfg(not(feature = "sys-cortex-m"))]
const MAX_POOL_SLOTS: usize = 4;

#[cfg(feature = "sys-cortex-m")]
const MAX_POOL_WORKERS: usize = 1;
#[cfg(not(feature = "sys-cortex-m"))]
const MAX_POOL_WORKERS: usize = 32;

#[cfg(feature = "sys-cortex-m")]
const MAX_POOL_QUEUE_ITEMS: usize = 1;
#[cfg(not(feature = "sys-cortex-m"))]
const MAX_POOL_QUEUE_ITEMS: usize = 256;
const ZERO_LOGICAL_CPU: ThreadLogicalCpuId = ThreadLogicalCpuId {
    group: fusion_pal::sys::thread::ThreadProcessorGroupId(0),
    index: 0,
};

#[derive(Clone, Copy)]
enum WorkerPlacement<'a> {
    LogicalCpus([ThreadLogicalCpuId; MAX_POOL_WORKERS]),
    CoreClasses(&'a [ThreadCoreClassId]),
}

#[derive(Debug, Clone, Copy)]
struct QueueLink {
    item: Option<SystemWorkItem>,
    next: Option<u16>,
}

impl QueueLink {
    const fn empty() -> Self {
        Self {
            item: None,
            next: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct WorkerQueue {
    head: Option<u16>,
    tail: Option<u16>,
    queued_items: usize,
}

impl WorkerQueue {
    const fn empty() -> Self {
        Self {
            head: None,
            tail: None,
            queued_items: 0,
        }
    }
}

/// Pool worker identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkerId(pub u16);

/// Raw submitted work entry executed by a carrier worker.
pub type SystemWorkEntry = unsafe fn(*mut ());
/// Cleanup hook for a work item that is canceled before execution.
pub type SystemWorkCancel = unsafe fn(*mut ());

/// One raw work item executed by a carrier worker.
#[derive(Debug, Clone, Copy)]
pub struct SystemWorkItem {
    /// Entry function executed by the worker.
    pub entry: SystemWorkEntry,
    /// Opaque caller-owned context passed to the entry function.
    pub context: *mut (),
    /// Optional cleanup hook invoked if the item is canceled before execution.
    pub cancel: Option<SystemWorkCancel>,
}

impl SystemWorkItem {
    /// Creates one raw work item.
    #[must_use]
    pub const fn new(entry: SystemWorkEntry, context: *mut ()) -> Self {
        Self {
            entry,
            context,
            cancel: None,
        }
    }

    /// Creates one raw work item with an explicit cancellation cleanup hook.
    #[must_use]
    pub const fn with_cancel(
        entry: SystemWorkEntry,
        context: *mut (),
        cancel: SystemWorkCancel,
    ) -> Self {
        Self {
            entry,
            context,
            cancel: Some(cancel),
        }
    }
}

// SAFETY: a work item is only a function pointer plus an opaque context pointer; callers who
// submit work are responsible for ensuring the referenced context may cross thread boundaries.
unsafe impl Send for SystemWorkItem {}
// SAFETY: shared references to a work item do not permit execution or mutation by themselves.
unsafe impl Sync for SystemWorkItem {}

/// Placement strategy for carrier threads in the system thread pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SystemPoolPlacement<'a> {
    /// Inherit platform defaults with no pool-managed affinity.
    Inherit,
    /// Attempt to place one carrier per logical CPU.
    PerCore,
    /// Prefer carriers on the supplied heterogeneous core classes.
    CoreClasses(&'a [ThreadCoreClassId]),
    /// Attempt to place one carrier per package or socket.
    PerPackage,
    /// Pin carriers to an explicit static set of logical CPUs.
    Static(&'a [ThreadLogicalCpuId]),
    /// Allow later orchestration to grow or shrink carriers dynamically.
    Dynamic,
}

/// Locality boundary for work stealing between carrier workers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SystemStealBoundary {
    /// Do not steal across workers.
    LocalOnly,
    /// Allow stealing only within the same core cluster or shared-cache domain.
    SameCoreCluster,
    /// Allow stealing only within the same package or socket.
    SamePackage,
    /// Allow stealing within the same NUMA node.
    SameNumaNode,
    /// Allow stealing across the full carrier pool.
    Global,
}

/// Resize policy for the system thread pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SystemResizePolicy {
    /// Worker count is fixed after startup.
    Fixed,
    /// Worker count may be adjusted only through explicit management calls.
    Manual,
    /// Worker count may be adjusted elastically later.
    Elastic,
}

/// Shutdown policy for a carrier pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SystemShutdownPolicy {
    /// Drain queued work before shutdown completes.
    Drain,
    /// Reject new work and cancel queued-but-not-started items.
    CancelPending,
    /// Tear down immediately once workers reach a safe stop point.
    Immediate,
}

/// Static configuration for a low-level system thread pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SystemThreadPoolConfig<'a> {
    /// Minimum number of carrier workers.
    pub min_threads: usize,
    /// Maximum number of carrier workers.
    pub max_threads: usize,
    /// Carrier placement strategy.
    pub placement: SystemPoolPlacement<'a>,
    /// Boundary at which work stealing is allowed.
    pub steal_boundary: SystemStealBoundary,
    /// Locality bias when admitting new raw work into carrier queues.
    pub spawn_locality_policy: CarrierSpawnLocalityPolicy,
    /// Whether the carrier count may change later.
    pub resize_policy: SystemResizePolicy,
    /// Shutdown behavior for existing workers and queued work.
    pub shutdown_policy: SystemShutdownPolicy,
    /// Optional worker-name prefix.
    pub name_prefix: Option<&'a str>,
    /// Stack request applied to workers.
    pub stack: ThreadStackRequest,
    /// Scheduler request applied to workers.
    pub scheduler: ThreadSchedulerRequest,
}

impl SystemThreadPoolConfig<'_> {
    /// Returns a minimal fixed single-worker carrier configuration.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            min_threads: 1,
            max_threads: 1,
            placement: SystemPoolPlacement::Inherit,
            steal_boundary: SystemStealBoundary::LocalOnly,
            spawn_locality_policy: CarrierSpawnLocalityPolicy::SameCore,
            resize_policy: SystemResizePolicy::Fixed,
            shutdown_policy: SystemShutdownPolicy::Drain,
            name_prefix: None,
            stack: ThreadStackRequest::new(),
            scheduler: ThreadSchedulerRequest::new(),
        }
    }
}

impl Default for SystemThreadPoolConfig<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// Observable low-level pool statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SystemThreadPoolStats {
    /// Configured minimum worker count.
    pub min_threads: usize,
    /// Configured maximum worker count.
    pub max_threads: usize,
    /// Current active worker count.
    pub active_workers: usize,
    /// Queued work items.
    pub queued_items: usize,
}

/// Low-level system thread pool error.
pub type SystemThreadPoolError = ThreadError;

/// Bounded carrier-pool primitive backed by system threads.
#[derive(Debug)]
pub struct SystemThreadPool {
    system: ThreadSystem,
    min_threads: usize,
    max_threads: usize,
    slot_index: Option<usize>,
}

#[derive(Debug)]
struct PoolRegistryStorage {
    lock: ThinMutex,
    slots: UnsafeCell<[PoolSlot; MAX_POOL_SLOTS]>,
}

#[derive(Debug)]
struct PoolSlot {
    queue_lock: ThinMutex,
    work_ready: [Option<Semaphore>; MAX_POOL_WORKERS],
    queue_slots: [QueueLink; MAX_POOL_QUEUE_ITEMS],
    worker_queues: [WorkerQueue; MAX_POOL_WORKERS],
    workers: [Option<ThreadHandle>; MAX_POOL_WORKERS],
    worker_observations: [Option<CarrierObservation>; MAX_POOL_WORKERS],
    allocated: bool,
    accepting: bool,
    shutting_down: bool,
    shutdown_policy: SystemShutdownPolicy,
    steal_boundary: SystemStealBoundary,
    spawn_locality_policy: CarrierSpawnLocalityPolicy,
    min_threads: usize,
    max_threads: usize,
    worker_count: usize,
    queued_items: usize,
    free_head: Option<u16>,
    next_worker: usize,
}

impl PoolRegistryStorage {
    fn new() -> Self {
        Self {
            lock: ThinMutex::new(),
            slots: UnsafeCell::new(array::from_fn(|_| PoolSlot::new())),
        }
    }
}

// SAFETY: access to `slots` is serialized through the registry lock and each slot's own queue
// lock before mutable access is taken.
unsafe impl Sync for PoolRegistryStorage {}

impl PoolSlot {
    fn new() -> Self {
        Self {
            queue_lock: ThinMutex::new(),
            work_ready: array::from_fn(|_| None),
            queue_slots: [QueueLink::empty(); MAX_POOL_QUEUE_ITEMS],
            worker_queues: [WorkerQueue::empty(); MAX_POOL_WORKERS],
            workers: array::from_fn(|_| None),
            worker_observations: [None; MAX_POOL_WORKERS],
            allocated: false,
            accepting: false,
            shutting_down: false,
            shutdown_policy: SystemShutdownPolicy::Drain,
            steal_boundary: SystemStealBoundary::LocalOnly,
            spawn_locality_policy: CarrierSpawnLocalityPolicy::SameCore,
            min_threads: 0,
            max_threads: 0,
            worker_count: 0,
            queued_items: 0,
            free_head: None,
            next_worker: 0,
        }
    }

    fn reset(&mut self) {
        let work_ready = core::mem::replace(&mut self.work_ready, array::from_fn(|_| None));
        *self = Self::new();
        self.work_ready = work_ready;
        self.drain_work_ready();
    }

    fn initialize_free_list(&mut self) {
        self.free_head = None;
        for index in (0..self.queue_slots.len()).rev() {
            let next = self.free_head;
            let Some(next_index) = u16::try_from(index).ok() else {
                continue;
            };
            self.queue_slots[index] = QueueLink { item: None, next };
            self.free_head = Some(next_index);
        }
    }

    fn configure_runtime(&mut self, config: &SystemThreadPoolConfig<'_>, worker_count: usize) {
        self.accepting = true;
        self.shutting_down = false;
        self.shutdown_policy = config.shutdown_policy;
        self.steal_boundary = config.steal_boundary;
        self.spawn_locality_policy = config.spawn_locality_policy;
        self.min_threads = config.min_threads;
        self.max_threads = config.max_threads;
        self.worker_count = worker_count;
        self.queued_items = 0;
        self.next_worker = 0;
        self.worker_queues = [WorkerQueue::empty(); MAX_POOL_WORKERS];
        self.worker_observations = [None; MAX_POOL_WORKERS];
        self.initialize_free_list();
    }

    fn install_worker_semaphores(&mut self, worker_count: usize) -> Result<(), ThreadError> {
        let semaphore_max = u32::try_from(MAX_POOL_QUEUE_ITEMS + MAX_POOL_WORKERS)
            .map_err(|_| ThreadError::resource_exhausted())?;
        for worker_index in 0..worker_count {
            if self.work_ready[worker_index].is_none() {
                self.work_ready[worker_index] =
                    Some(Semaphore::new(0, semaphore_max).map_err(thread_error_from_sync)?);
            }
        }
        self.drain_work_ready();
        Ok(())
    }

    fn enqueue(
        &mut self,
        item: SystemWorkItem,
        preferred_worker: Option<usize>,
    ) -> Result<usize, ThreadError> {
        if !self.accepting || self.shutting_down {
            return Err(ThreadError::state_conflict());
        }
        if self.queued_items == self.queue_slots.len() {
            return Err(ThreadError::resource_exhausted());
        }

        let worker_index = self.select_worker_for_submission(preferred_worker)?;
        let slot_index = self
            .free_head
            .take()
            .ok_or_else(ThreadError::resource_exhausted)?;
        let slot = &mut self.queue_slots[usize::from(slot_index)];
        self.free_head = slot.next.take();
        slot.item = Some(item);

        let queue = &mut self.worker_queues[worker_index];
        if let Some(tail_index) = queue.tail {
            self.queue_slots[usize::from(tail_index)].next = Some(slot_index);
        } else {
            queue.head = Some(slot_index);
        }
        queue.tail = Some(slot_index);
        queue.queued_items += 1;
        self.queued_items += 1;
        Ok(worker_index)
    }

    fn dequeue_for_worker(&mut self, worker_index: usize) -> Option<SystemWorkItem> {
        let queue = self.worker_queues.get_mut(worker_index)?;
        let head_index = queue.head?;
        let slot = &mut self.queue_slots[usize::from(head_index)];
        let item = slot.item.take()?;
        let next = slot.next.take();
        queue.head = next;
        if queue.head.is_none() {
            queue.tail = None;
        }
        queue.queued_items = queue.queued_items.saturating_sub(1);
        slot.next = self.free_head;
        self.free_head = Some(head_index);
        self.queued_items = self.queued_items.saturating_sub(1);
        Some(item)
    }

    fn clear_queue(&mut self) {
        while self.queued_items != 0 {
            let mut progressed = false;
            for worker_index in 0..self.worker_count {
                let Some(item) = self.dequeue_for_worker(worker_index) else {
                    continue;
                };
                progressed = true;
                if let Some(cancel) = item.cancel {
                    // SAFETY: cancellation only receives the caller-owned opaque context that
                    // would otherwise have been passed to the work entry.
                    unsafe { cancel(item.context) };
                }
            }
            if !progressed {
                break;
            }
        }
    }

    fn select_worker_for_submission(
        &mut self,
        preferred_worker: Option<usize>,
    ) -> Result<usize, ThreadError> {
        if self.worker_count == 0 {
            return Err(ThreadError::state_conflict());
        }
        if let Some(worker_index) = preferred_worker
            && worker_index < self.worker_count
        {
            return Ok(worker_index);
        }
        let worker_index = self.next_worker % self.worker_count;
        self.next_worker = (self.next_worker + 1) % self.worker_count;
        Ok(worker_index)
    }

    fn publish_worker_observation(&mut self, worker_index: usize, observation: CarrierObservation) {
        if worker_index < self.worker_observations.len() {
            self.worker_observations[worker_index] = Some(observation);
        }
    }

    fn preferred_worker_for_current(&self) -> Option<usize> {
        let origin = system_carrier().observe_current().ok()?;
        let mut best: Option<(u8, usize)> = None;
        for worker_index in 0..self.worker_count {
            let Some(candidate) = self.worker_observations[worker_index] else {
                continue;
            };
            if candidate.thread_id == origin.thread_id {
                return Some(worker_index);
            }
            let Some(rank) = carrier_spawn_locality_rank(
                self.spawn_locality_policy,
                origin.location,
                candidate.location,
            ) else {
                continue;
            };
            if best.is_none_or(|(best_rank, _)| rank < best_rank) {
                best = Some((rank, worker_index));
            }
        }
        best.map(|(_, worker_index)| worker_index)
    }

    fn companion_worker_for_submission(&self, worker_index: usize) -> Option<usize> {
        if matches!(self.steal_boundary, SystemStealBoundary::LocalOnly) || self.worker_count <= 1 {
            return None;
        }
        let origin = self
            .worker_observations
            .get(worker_index)
            .copied()
            .flatten();
        for offset in 1..self.worker_count {
            let candidate_index = (worker_index + offset) % self.worker_count;
            let candidate = self
                .worker_observations
                .get(candidate_index)
                .copied()
                .flatten();
            if steal_boundary_allows(self.steal_boundary, origin, candidate) {
                return Some(candidate_index);
            }
        }
        None
    }

    fn steal_for_worker(&mut self, worker_index: usize) -> Option<SystemWorkItem> {
        if matches!(self.steal_boundary, SystemStealBoundary::LocalOnly) || self.worker_count <= 1 {
            return None;
        }

        let origin = self
            .worker_observations
            .get(worker_index)
            .copied()
            .flatten();
        for offset in 1..self.worker_count {
            let victim_index = (worker_index + offset) % self.worker_count;
            if self.worker_queues[victim_index].queued_items == 0 {
                continue;
            }
            let victim = self
                .worker_observations
                .get(victim_index)
                .copied()
                .flatten();
            if !steal_boundary_allows(self.steal_boundary, origin, victim) {
                continue;
            }
            if let Some(item) = self.dequeue_for_worker(victim_index) {
                return Some(item);
            }
        }
        None
    }

    fn worker_semaphore_ptr(&self, worker_index: usize) -> Result<*const Semaphore, ThreadError> {
        self.work_ready
            .get(worker_index)
            .and_then(Option::as_ref)
            .map(core::ptr::from_ref)
            .ok_or_else(ThreadError::unsupported)
    }

    fn release_shutdown_wakeups(&self) -> Result<(), ThreadError> {
        for worker_index in 0..self.worker_count {
            let semaphore = self.work_ready[worker_index]
                .as_ref()
                .ok_or_else(ThreadError::unsupported)?;
            semaphore.release(1).map_err(thread_error_from_sync)?;
        }
        Ok(())
    }

    fn drain_work_ready(&self) {
        for semaphore in self.work_ready.iter().flatten() {
            while semaphore.try_acquire().unwrap_or(false) {}
        }
    }
}

impl SystemThreadPool {
    /// Reports the underlying fusion-pal thread support driving the carrier pool.
    #[must_use]
    pub fn support(system: &ThreadSystem) -> ThreadSupport {
        system.support()
    }

    /// Creates a carrier pool using the supplied configuration.
    ///
    /// # Errors
    ///
    /// Returns `invalid` for obviously inconsistent bounds and `unsupported` when the
    /// selected backend cannot honestly realize a fixed bounded worker pool yet.
    pub fn new(
        system: ThreadSystem,
        config: &SystemThreadPoolConfig<'_>,
    ) -> Result<Self, SystemThreadPoolError> {
        validate_pool_config(system.support(), config)?;
        let registry = registry()?;
        let worker_count = config.max_threads;
        let slot_index = allocate_pool_slot(registry, config, worker_count)?;

        let spawn_result = spawn_workers(slot_index, system, config, worker_count);
        if let Err(error) = spawn_result {
            let mut pool = Self {
                system,
                min_threads: config.min_threads,
                max_threads: config.max_threads,
                slot_index: Some(slot_index),
            };
            let _ = pool.shutdown_inner();
            return Err(error);
        }

        Ok(Self {
            system,
            min_threads: config.min_threads,
            max_threads: config.max_threads,
            slot_index: Some(slot_index),
        })
    }

    /// Returns the configured statistics snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if the pool can no longer observe its slot state honestly.
    pub fn stats(&self) -> Result<SystemThreadPoolStats, ThreadError> {
        let Some(slot_index) = self.slot_index else {
            return Ok(SystemThreadPoolStats {
                min_threads: self.min_threads,
                max_threads: self.max_threads,
                active_workers: 0,
                queued_items: 0,
            });
        };

        with_slot(slot_index, |slot| {
            Ok(SystemThreadPoolStats {
                min_threads: self.min_threads,
                max_threads: self.max_threads,
                active_workers: slot.worker_count,
                queued_items: slot.queued_items,
            })
        })
    }

    /// Returns the current active worker count.
    ///
    /// # Errors
    ///
    /// Returns an error if the pool can no longer observe its slot state honestly.
    pub fn worker_count(&self) -> Result<usize, ThreadError> {
        Ok(self.stats()?.active_workers)
    }

    /// Submits one raw work item to the bounded carrier queue.
    ///
    /// # Errors
    ///
    /// Returns an error when the pool is shut down, the queue is full, or the pool can no
    /// longer coordinate submission honestly.
    pub fn submit(&self, work: SystemWorkItem) -> Result<(), SystemThreadPoolError> {
        let slot_index = self.slot_index.ok_or_else(ThreadError::state_conflict)?;
        let (primary, companion) = with_slot(slot_index, |slot| {
            let preferred_worker = slot.preferred_worker_for_current();
            let worker_index = slot.enqueue(work, preferred_worker)?;
            Ok((
                slot.worker_semaphore_ptr(worker_index)?,
                slot.companion_worker_for_submission(worker_index)
                    .map(|companion| slot.worker_semaphore_ptr(companion))
                    .transpose()?,
            ))
        })?;
        // SAFETY: the slot-owned semaphore remains valid while the slot stays allocated.
        // We intentionally release after dropping the queue lock so wakeups do not occur
        // while the caller still serializes access to the ring-buffer state.
        let semaphore = unsafe { &*primary };
        semaphore.release(1).map_err(thread_error_from_sync)?;
        if let Some(companion) = companion {
            let companion = unsafe { &*companion };
            companion.release(1).map_err(thread_error_from_sync)?;
        }
        Ok(())
    }

    /// Shuts the pool down according to its configured shutdown policy.
    ///
    /// # Errors
    ///
    /// Returns an error if the pool can no longer coordinate worker shutdown honestly.
    #[allow(clippy::needless_pass_by_value)]
    pub fn shutdown(mut self) -> Result<(), SystemThreadPoolError> {
        self.shutdown_inner()
    }

    /// Returns the underlying system thread support surface.
    #[must_use]
    pub fn thread_support(&self) -> ThreadSupport {
        self.system.support()
    }

    fn shutdown_inner(&mut self) -> Result<(), ThreadError> {
        let Some(slot_index) = self.slot_index.take() else {
            return Ok(());
        };

        let mut handles: [Option<ThreadHandle>; MAX_POOL_WORKERS] = array::from_fn(|_| None);
        let worker_count = {
            let worker_count = with_slot(slot_index, |slot| {
                slot.accepting = false;
                slot.shutting_down = true;
                if !matches!(slot.shutdown_policy, SystemShutdownPolicy::Drain) {
                    slot.clear_queue();
                }

                let worker_count = slot.worker_count;
                for (dst, src) in handles.iter_mut().zip(slot.workers.iter_mut()) {
                    *dst = src.take();
                }
                slot.release_shutdown_wakeups()?;
                Ok(worker_count)
            })?;
            worker_count
        };

        for handle in handles.into_iter().take(worker_count).flatten() {
            let _ = self.system.join(handle);
        }

        let registry = registry()?;
        let _registry_guard = registry.lock.lock().map_err(thread_error_from_sync)?;
        let slots_ptr = registry.slots.get();
        // SAFETY: the registry lock serializes exclusive access to the slot table here.
        unsafe { (&mut *slots_ptr)[slot_index].reset() };
        Ok(())
    }
}

impl Drop for SystemThreadPool {
    fn drop(&mut self) {
        let _ = self.shutdown_inner();
    }
}

impl fmt::Display for SystemThreadPoolStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "workers {}/{} active, {} queued",
            self.active_workers, self.max_threads, self.queued_items
        )
    }
}

fn registry() -> Result<&'static PoolRegistryStorage, ThreadError> {
    static REGISTRY: OnceLock<PoolRegistryStorage> = OnceLock::new();
    REGISTRY
        .get_or_try_init(|| Ok::<_, SyncError>(PoolRegistryStorage::new()))
        .map_err(|error| match error {
            OnceInitError::Sync(sync) | OnceInitError::Init(sync) => thread_error_from_sync(sync),
        })
}

fn validate_pool_config(
    support: ThreadSupport,
    config: &SystemThreadPoolConfig<'_>,
) -> Result<(), ThreadError> {
    if config.min_threads == 0 || config.min_threads > config.max_threads {
        return Err(ThreadError::invalid());
    }
    if config.max_threads > MAX_POOL_WORKERS {
        return Err(ThreadError::resource_exhausted());
    }
    if config.min_threads != config.max_threads {
        return Err(ThreadError::unsupported());
    }
    if config.resize_policy != SystemResizePolicy::Fixed {
        return Err(ThreadError::unsupported());
    }
    if !matches!(
        config.placement,
        SystemPoolPlacement::Inherit
            | SystemPoolPlacement::Static(_)
            | SystemPoolPlacement::PerCore
            | SystemPoolPlacement::CoreClasses(_)
    ) {
        return Err(ThreadError::unsupported());
    }
    if !support.lifecycle.caps.contains(ThreadLifecycleCaps::SPAWN)
        || !support.lifecycle.caps.contains(ThreadLifecycleCaps::JOIN)
    {
        return Err(ThreadError::unsupported());
    }
    match config.placement {
        SystemPoolPlacement::Inherit => {}
        SystemPoolPlacement::Static(_) | SystemPoolPlacement::PerCore => {
            if !support
                .placement
                .caps
                .contains(fusion_pal::sys::thread::ThreadPlacementCaps::LOGICAL_CPU_AFFINITY)
            {
                return Err(ThreadError::unsupported());
            }
        }
        SystemPoolPlacement::CoreClasses(classes) => {
            if classes.is_empty()
                || support.placement.core_class_affinity
                    == fusion_pal::sys::thread::ThreadGuarantee::Unsupported
            {
                return Err(ThreadError::unsupported());
            }
        }
        SystemPoolPlacement::PerPackage | SystemPoolPlacement::Dynamic => {
            return Err(ThreadError::unsupported());
        }
    }
    Ok(())
}

fn allocate_pool_slot(
    registry: &PoolRegistryStorage,
    config: &SystemThreadPoolConfig<'_>,
    worker_count: usize,
) -> Result<usize, ThreadError> {
    let _guard = registry.lock.lock().map_err(thread_error_from_sync)?;
    let slots = unsafe { &mut *registry.slots.get() };
    let Some(slot_index) = slots.iter().position(|slot| !slot.allocated) else {
        return Err(ThreadError::resource_exhausted());
    };
    let slot = &mut slots[slot_index];

    slot.install_worker_semaphores(worker_count)?;
    slot.allocated = true;
    slot.configure_runtime(config, worker_count);
    Ok(slot_index)
}

fn spawn_workers(
    slot_index: usize,
    system: ThreadSystem,
    config: &SystemThreadPoolConfig<'_>,
    worker_count: usize,
) -> Result<(), ThreadError> {
    let worker_placement = resolve_worker_placement(config, worker_count)?;

    for worker_index in 0..worker_count {
        let token = encode_worker_token(slot_index, worker_index);
        let handle = match worker_placement.as_ref() {
            Some(WorkerPlacement::LogicalCpus(cpus)) => {
                let single = &cpus[worker_index..=worker_index];
                let targets = [ThreadPlacementTarget::LogicalCpus(single)];
                let placement = fusion_pal::sys::thread::ThreadPlacementRequest {
                    targets: &targets,
                    mode: ThreadConstraintMode::Require,
                    phase: ThreadPlacementPhase::PreStartPreferred,
                    migration: ThreadMigrationPolicy::Inherit,
                };
                let thread_config = ThreadConfig {
                    join_policy: fusion_pal::sys::thread::ThreadJoinPolicy::Joinable,
                    name: config.name_prefix,
                    start_mode: ThreadStartMode::PlacementCommitted,
                    placement,
                    scheduler: config.scheduler,
                    stack: config.stack,
                };
                unsafe {
                    system.spawn_raw(
                        &thread_config,
                        worker_thread_entry as RawThreadEntry,
                        token.cast(),
                    )
                }?
            }
            Some(WorkerPlacement::CoreClasses(classes)) => {
                let targets = [ThreadPlacementTarget::CoreClasses(classes)];
                let placement = fusion_pal::sys::thread::ThreadPlacementRequest {
                    targets: &targets,
                    mode: ThreadConstraintMode::Prefer,
                    phase: ThreadPlacementPhase::PreStartPreferred,
                    migration: ThreadMigrationPolicy::Inherit,
                };
                let thread_config = ThreadConfig {
                    join_policy: fusion_pal::sys::thread::ThreadJoinPolicy::Joinable,
                    name: config.name_prefix,
                    start_mode: ThreadStartMode::PlacementCommitted,
                    placement,
                    scheduler: config.scheduler,
                    stack: config.stack,
                };
                unsafe {
                    system.spawn_raw(
                        &thread_config,
                        worker_thread_entry as RawThreadEntry,
                        token.cast(),
                    )
                }?
            }
            None => {
                let thread_config = ThreadConfig {
                    join_policy: fusion_pal::sys::thread::ThreadJoinPolicy::Joinable,
                    name: config.name_prefix,
                    start_mode: ThreadStartMode::Immediate,
                    placement: fusion_pal::sys::thread::ThreadPlacementRequest::new(),
                    scheduler: config.scheduler,
                    stack: config.stack,
                };
                unsafe {
                    system.spawn_raw(
                        &thread_config,
                        worker_thread_entry as RawThreadEntry,
                        token.cast(),
                    )
                }?
            }
        };

        with_slot(slot_index, |slot| {
            slot.workers[worker_index] = Some(handle);
            Ok(())
        })?;
    }

    Ok(())
}

fn resolve_worker_placement<'a>(
    config: &'a SystemThreadPoolConfig<'a>,
    worker_count: usize,
) -> Result<Option<WorkerPlacement<'a>>, ThreadError> {
    match config.placement {
        SystemPoolPlacement::Inherit => Ok(None),
        SystemPoolPlacement::Static(cpus) => {
            if cpus.len() < worker_count {
                return Err(ThreadError::invalid());
            }
            let mut resolved = [ZERO_LOGICAL_CPU; MAX_POOL_WORKERS];
            resolved[..worker_count].copy_from_slice(&cpus[..worker_count]);
            Ok(Some(WorkerPlacement::LogicalCpus(resolved)))
        }
        SystemPoolPlacement::PerCore => {
            let mut resolved = [ZERO_LOGICAL_CPU; MAX_POOL_WORKERS];
            let summary = system_cpu()
                .write_logical_cpus(&mut resolved[..worker_count])
                .map_err(|_| ThreadError::unsupported())?;
            if summary.total < worker_count {
                return Err(ThreadError::resource_exhausted());
            }
            Ok(Some(WorkerPlacement::LogicalCpus(resolved)))
        }
        SystemPoolPlacement::CoreClasses(classes) => {
            Ok(Some(WorkerPlacement::CoreClasses(classes)))
        }
        SystemPoolPlacement::PerPackage | SystemPoolPlacement::Dynamic => {
            Err(ThreadError::unsupported())
        }
    }
}

fn steal_boundary_allows(
    boundary: SystemStealBoundary,
    origin: Option<CarrierObservation>,
    candidate: Option<CarrierObservation>,
) -> bool {
    match boundary {
        SystemStealBoundary::LocalOnly => false,
        SystemStealBoundary::Global => true,
        SystemStealBoundary::SameCoreCluster => {
            let (Some(origin), Some(candidate)) = (origin, candidate) else {
                return false;
            };
            origin.location.cluster.is_some()
                && origin.location.cluster == candidate.location.cluster
        }
        SystemStealBoundary::SamePackage => {
            let (Some(origin), Some(candidate)) = (origin, candidate) else {
                return false;
            };
            origin.location.package.is_some()
                && origin.location.package == candidate.location.package
        }
        SystemStealBoundary::SameNumaNode => {
            let (Some(origin), Some(candidate)) = (origin, candidate) else {
                return false;
            };
            origin.location.numa_node.is_some()
                && origin.location.numa_node == candidate.location.numa_node
        }
    }
}

unsafe fn worker_thread_entry(context: *mut ()) -> fusion_pal::sys::thread::ThreadEntryReturn {
    let (slot_index, worker_index) = decode_worker_token(context.cast_const());
    if registry().is_err() {
        return fusion_pal::sys::thread::ThreadEntryReturn::new(1);
    }
    if let Ok(observation) = system_carrier().observe_current() {
        let _ = with_slot(slot_index, |slot| {
            slot.publish_worker_observation(worker_index, observation);
            Ok(())
        });
    }

    loop {
        let Ok(semaphore) = with_slot(slot_index, |slot| slot.worker_semaphore_ptr(worker_index))
        else {
            return fusion_pal::sys::thread::ThreadEntryReturn::new(2);
        };

        let semaphore = unsafe { &*semaphore };
        if semaphore.acquire().is_err() {
            return fusion_pal::sys::thread::ThreadEntryReturn::new(4);
        }

        let next = match with_slot(slot_index, |slot| {
            if let Ok(observation) = system_carrier().observe_current() {
                slot.publish_worker_observation(worker_index, observation);
            }
            Ok(match slot.dequeue_for_worker(worker_index) {
                Some(item) => Some(item),
                None => match slot.steal_for_worker(worker_index) {
                    Some(item) => Some(item),
                    None if slot.shutting_down => None,
                    None => return Err(ThreadError::busy()),
                },
            })
        }) {
            Ok(Some(item)) => Some(item),
            Ok(None) => None,
            Err(error) if error.kind() == ThreadErrorKind::Busy => continue,
            Err(_) => return fusion_pal::sys::thread::ThreadEntryReturn::new(5),
        };

        match next {
            Some(item) => unsafe { (item.entry)(item.context) },
            None => break,
        }
    }

    fusion_pal::sys::thread::ThreadEntryReturn::new(0)
}

fn with_slot<R>(
    slot_index: usize,
    f: impl FnOnce(&mut PoolSlot) -> Result<R, ThreadError>,
) -> Result<R, ThreadError> {
    let registry = registry()?;
    if slot_index >= MAX_POOL_SLOTS {
        return Err(ThreadError::invalid());
    }

    let slot_ptr = unsafe { (*registry.slots.get()).as_mut_ptr().add(slot_index) };
    // SAFETY: `slot_ptr` points at one stable slot in the process-wide registry.
    // The slot queue lock serializes mutable access to the slot state below.
    let guard = unsafe { &(*slot_ptr).queue_lock }
        .lock()
        .map_err(thread_error_from_sync)?;
    // SAFETY: queue-lock ownership gives us exclusive mutable access to the selected slot
    // for the duration of this closure invocation.
    let result = unsafe {
        let slot = &mut *slot_ptr;
        if !slot.allocated {
            return Err(ThreadError::state_conflict());
        }
        f(slot)
    };
    drop(guard);
    result
}

const fn encode_worker_token(slot_index: usize, worker_index: usize) -> *mut u8 {
    let packed = (slot_index << 16) | worker_index;
    packed as *mut u8
}

fn decode_worker_token(token: *const ()) -> (usize, usize) {
    let packed = token.addr();
    (packed >> 16, packed & 0xffff)
}

const fn thread_error_from_sync(error: SyncError) -> ThreadError {
    match error.kind {
        SyncErrorKind::Unsupported => ThreadError::unsupported(),
        SyncErrorKind::Invalid | SyncErrorKind::Overflow => ThreadError::invalid(),
        SyncErrorKind::Busy => ThreadError::busy(),
        SyncErrorKind::PermissionDenied => ThreadError::permission_denied(),
        SyncErrorKind::Platform(code) => ThreadError::platform(code),
    }
}
