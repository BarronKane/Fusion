//! Low-level bounded carrier-pool planning surface.
//!
//! The concrete worker implementation is still deferred, but this module locks down the
//! system-thread-pool contract and its placement, stealing, resize, and shutdown policy
//! vocabulary so that higher layers can stop inventing them ad hoc.

use core::fmt;

use super::{
    ThreadError, ThreadLogicalCpuId, ThreadSchedulerRequest, ThreadStackRequest, ThreadSupport,
    ThreadSystem,
};

/// Pool worker identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkerId(pub u16);

/// Placement strategy for carrier threads in the system thread pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SystemPoolPlacement<'a> {
    /// Inherit platform defaults with no pool-managed affinity.
    Inherit,
    /// Attempt to place one carrier per logical CPU.
    PerCore,
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
    /// Queued work items, when the implementation exists.
    pub queued_items: usize,
}

/// Low-level system thread pool error.
pub type SystemThreadPoolError = ThreadError;

/// Planned bounded carrier-pool primitive.
#[derive(Debug, Clone, Copy)]
pub struct SystemThreadPool {
    system: ThreadSystem,
    min_threads: usize,
    max_threads: usize,
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
    /// Returns `invalid` for obviously inconsistent bounds and `unsupported` until the
    /// concrete bounded worker implementation lands.
    pub const fn new(
        system: ThreadSystem,
        config: &SystemThreadPoolConfig<'_>,
    ) -> Result<Self, SystemThreadPoolError> {
        if config.min_threads == 0 || config.min_threads > config.max_threads {
            return Err(ThreadError::invalid());
        }
        let _ = system;
        Err(ThreadError::unsupported())
    }

    /// Returns the configured statistics snapshot.
    #[must_use]
    pub const fn stats(&self) -> SystemThreadPoolStats {
        SystemThreadPoolStats {
            min_threads: self.min_threads,
            max_threads: self.max_threads,
            active_workers: 0,
            queued_items: 0,
        }
    }

    /// Returns the underlying system thread support surface.
    #[must_use]
    pub fn thread_support(&self) -> ThreadSupport {
        self.system.support()
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
