//! Domain 5: public runtime orchestration surface.

use super::{Executor, ExecutorConfig, GreenPool, GreenPoolConfig, ThreadPool, ThreadPoolConfig};

/// Runtime profile selecting broad safety and elasticity policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeProfile {
    /// Fixed-capacity, deterministic carrier and queue behavior.
    Deterministic,
    /// Balanced hosted profile with optional elasticity.
    Balanced,
    /// Throughput-oriented profile with relaxed elasticity limits.
    Throughput,
    /// Fully custom manual control.
    Custom,
}

/// Hard constraints enforced by the deterministic runtime profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeterministicConstraints {
    /// Carrier thread count is fixed after startup.
    pub workers: FixedConstraint,
    /// Queue capacities are fixed and bounded.
    pub queues: FixedConstraint,
    /// Green-thread population is fixed or explicitly capped.
    pub green_limit: FixedConstraint,
    /// Cross-domain stealing is forbidden unless explicitly allowed.
    pub global_steal: GlobalStealConstraint,
}

impl DeterministicConstraints {
    /// Returns strict deterministic defaults.
    #[must_use]
    pub const fn strict() -> Self {
        Self {
            workers: FixedConstraint::Required,
            queues: FixedConstraint::Required,
            green_limit: FixedConstraint::Required,
            global_steal: GlobalStealConstraint::Disallow,
        }
    }
}

/// Whether a runtime resource must remain fixed after startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FixedConstraint {
    /// The resource must remain fixed after startup.
    Required,
    /// The resource may remain flexible.
    Flexible,
}

/// Whether global stealing is allowed under deterministic policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GlobalStealConstraint {
    /// Global stealing is forbidden.
    Disallow,
    /// Global stealing is allowed.
    Allow,
}

/// Elastic behavior knobs for hosted-oriented runtime profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ElasticConfig {
    /// Allow carrier-pool resizing.
    pub allow_resize: bool,
    /// Allow on-demand green-thread growth.
    pub allow_on_demand_green: bool,
    /// Allow work stealing across the full machine.
    pub allow_global_steal: bool,
}

impl ElasticConfig {
    /// Returns permissive hosted defaults.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            allow_resize: true,
            allow_on_demand_green: true,
            allow_global_steal: true,
        }
    }
}

impl Default for ElasticConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Public runtime configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RuntimeConfig<'a> {
    /// Selected runtime profile.
    pub profile: RuntimeProfile,
    /// Carrier thread-pool configuration.
    pub thread_pool: ThreadPoolConfig<'a>,
    /// Optional green-thread configuration.
    pub green: Option<GreenPoolConfig>,
    /// Executor configuration.
    pub executor: ExecutorConfig,
    /// Optional deterministic constraints.
    pub deterministic: Option<DeterministicConstraints>,
    /// Optional elastic profile configuration.
    pub elastic: Option<ElasticConfig>,
}

/// Public runtime statistics snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RuntimeStats {
    /// Active carrier workers, when the pool exists.
    pub carrier_workers: usize,
    /// Active green threads, when the pool exists.
    pub green_threads: usize,
    /// Known queued tasks.
    pub queued_tasks: usize,
}

/// Public runtime error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeError {
    /// The requested composition is unsupported or not implemented yet.
    Unsupported,
    /// The requested carrier thread pool could not be created honestly.
    ThreadPool(super::ThreadPoolError),
    /// The requested green-thread pool could not be created honestly.
    Green(fusion_sys::fiber::FiberError),
    /// The configured executor could not be bound honestly.
    Executor(super::ExecutorError),
}

/// Public runtime orchestrator.
#[derive(Debug)]
pub struct Runtime {
    executor: Executor,
    green_pool: Option<GreenPool>,
    thread_pool: Option<ThreadPool>,
}

impl Runtime {
    /// Creates a runtime orchestrator from the supplied configuration.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` for not-yet-implemented green-thread-backed configurations, or
    /// a lower-level carrier-pool error when pool creation fails.
    pub fn new(config: &RuntimeConfig<'_>) -> Result<Self, RuntimeError> {
        validate_runtime_config(config)?;

        let thread_pool = ThreadPool::new(&config.thread_pool).map_err(RuntimeError::ThreadPool)?;
        let green_pool = match config.green {
            Some(green) => Some(GreenPool::new(&green, &thread_pool).map_err(RuntimeError::Green)?),
            None => None,
        };
        let executor = match config.executor.mode {
            super::ExecutorMode::CurrentThread => Executor::new(config.executor),
            super::ExecutorMode::ThreadPool => Executor::new(config.executor)
                .on_pool(&thread_pool)
                .map_err(RuntimeError::Executor)?,
            super::ExecutorMode::GreenPool => Executor::new(config.executor)
                .on_green(green_pool.as_ref().ok_or(RuntimeError::Unsupported)?)
                .map_err(RuntimeError::Executor)?,
            super::ExecutorMode::Hybrid => return Err(RuntimeError::Unsupported),
        };

        Ok(Self {
            executor,
            green_pool,
            thread_pool: Some(thread_pool),
        })
    }

    /// Returns the configured executor surface.
    #[must_use]
    pub const fn executor(&self) -> &Executor {
        &self.executor
    }

    /// Returns the thread pool when one exists.
    #[must_use]
    pub const fn thread_pool(&self) -> Option<&ThreadPool> {
        self.thread_pool.as_ref()
    }

    /// Returns the green-thread pool when one exists.
    #[must_use]
    pub const fn green_pool(&self) -> Option<&GreenPool> {
        self.green_pool.as_ref()
    }

    /// Returns a snapshot of the current runtime state.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying carrier-pool statistics cannot be observed honestly.
    pub fn stats(&self) -> Result<RuntimeStats, RuntimeError> {
        let (carrier_workers, queued_tasks) = match self.thread_pool.as_ref() {
            Some(pool) => {
                let stats = pool.stats().map_err(RuntimeError::ThreadPool)?;
                (stats.active_workers, stats.queued_items)
            }
            None => (0, 0),
        };
        let green_threads = self.green_pool.as_ref().map_or(0, GreenPool::active_count);

        Ok(RuntimeStats {
            carrier_workers,
            green_threads,
            queued_tasks,
        })
    }
}

fn validate_runtime_config(config: &RuntimeConfig<'_>) -> Result<(), RuntimeError> {
    if matches!(config.executor.mode, super::ExecutorMode::GreenPool) && config.green.is_none() {
        return Err(RuntimeError::Unsupported);
    }

    if matches!(config.executor.mode, super::ExecutorMode::Hybrid) {
        return Err(RuntimeError::Unsupported);
    }

    if config.profile == RuntimeProfile::Deterministic {
        let constraints = config
            .deterministic
            .unwrap_or_else(DeterministicConstraints::strict);
        if matches!(constraints.workers, FixedConstraint::Required)
            && config.thread_pool.min_threads != config.thread_pool.max_threads
        {
            return Err(RuntimeError::Unsupported);
        }
        if matches!(constraints.global_steal, GlobalStealConstraint::Disallow)
            && matches!(
                config.thread_pool.steal_boundary,
                super::StealBoundary::Global
            )
        {
            return Err(RuntimeError::Unsupported);
        }
    }

    Ok(())
}
