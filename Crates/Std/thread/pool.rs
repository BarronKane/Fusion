//! Domain 1: public carrier thread-pool surface.

use fusion_sys::thread::{
    SystemPoolPlacement, SystemResizePolicy, SystemShutdownPolicy, SystemStealBoundary,
    SystemThreadPool, SystemThreadPoolConfig, SystemThreadPoolError, SystemThreadPoolStats,
    ThreadLogicalCpuId, ThreadSchedulerRequest, ThreadStackRequest, ThreadSupport, ThreadSystem,
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

/// Public carrier thread-pool wrapper.
#[derive(Debug, Clone, Copy)]
pub struct ThreadPool {
    inner: SystemThreadPool,
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
        Ok(Self { inner })
    }

    /// Returns the current pool statistics snapshot.
    #[must_use]
    pub const fn stats(&self) -> PoolStats {
        self.inner.stats()
    }
}

pub use fusion_sys::thread::WorkerId;
