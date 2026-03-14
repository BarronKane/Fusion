//! Domain 3: public async executor and reactor surface.

use core::future::Future;
use core::marker::PhantomData;

use fusion_sys::event::{EventSupport, EventSystem};

use super::{GreenPool, ThreadPool};

/// Public executor operating mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExecutorMode {
    /// Drive futures cooperatively on the current thread.
    CurrentThread,
    /// Drive futures on a carrier thread pool.
    ThreadPool,
    /// Drive futures on a green-thread pool.
    GreenPool,
    /// Drive futures across a hybrid thread-pool and green-thread arrangement.
    Hybrid,
}

/// Public reactor configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReactorConfig {
    /// Maximum events pulled from the reactor in one poll, when bounded manually.
    pub max_events: Option<usize>,
}

impl ReactorConfig {
    /// Returns the default reactor configuration.
    #[must_use]
    pub const fn new() -> Self {
        Self { max_events: None }
    }
}

impl Default for ReactorConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Public executor configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExecutorConfig {
    /// Selected execution mode.
    pub mode: ExecutorMode,
    /// Reactor configuration for I/O readiness or completion integration.
    pub reactor: ReactorConfig,
}

impl ExecutorConfig {
    /// Returns a current-thread executor configuration.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            mode: ExecutorMode::CurrentThread,
            reactor: ReactorConfig::new(),
        }
    }
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Public executor error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExecutorError {
    /// The requested executor mode is unsupported or not implemented yet.
    Unsupported,
}

/// Public reactor wrapper.
#[derive(Debug, Clone, Copy)]
pub struct Reactor {
    inner: EventSystem,
}

impl Reactor {
    /// Creates a reactor wrapper for the selected backend event source.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: EventSystem::new(),
        }
    }

    /// Reports the truthful backend event support surface.
    #[must_use]
    pub fn support(&self) -> EventSupport {
        self.inner.support()
    }
}

impl Default for Reactor {
    fn default() -> Self {
        Self::new()
    }
}

/// Public spawned-task handle.
#[derive(Debug)]
pub struct TaskHandle<T> {
    id: u64,
    _marker: PhantomData<T>,
}

impl<T> TaskHandle<T> {
    /// Returns the stable task identifier.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }
}

/// Public set of task handles joined as a group.
#[derive(Debug, Default)]
pub struct JoinSet<T> {
    _marker: PhantomData<T>,
}

/// Public async executor wrapper.
#[derive(Debug, Clone, Copy)]
pub struct Executor {
    mode: ExecutorMode,
    reactor: Reactor,
}

impl Executor {
    /// Creates a new executor surface.
    #[must_use]
    pub const fn new(config: ExecutorConfig) -> Self {
        Self {
            mode: config.mode,
            reactor: Reactor::new(),
        }
    }

    /// Returns the configured executor mode.
    #[must_use]
    pub const fn mode(&self) -> ExecutorMode {
        self.mode
    }

    /// Returns the public reactor wrapper.
    #[must_use]
    pub const fn reactor(&self) -> &Reactor {
        &self.reactor
    }

    /// Spawns a `Send` future onto the executor.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` until the executor runtime is implemented on top of the
    /// selected carrier domain.
    pub fn spawn<F>(&self, _future: F) -> Result<TaskHandle<F::Output>, ExecutorError>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        Err(ExecutorError::Unsupported)
    }

    /// Spawns a non-`Send` future local to the current execution domain.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` until the local-task executor path exists.
    pub fn spawn_local<F>(&self, _future: F) -> Result<TaskHandle<F::Output>, ExecutorError>
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        Err(ExecutorError::Unsupported)
    }

    /// Attaches the executor to a carrier thread pool later on.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` until thread-pool-backed execution is implemented.
    pub const fn on_pool(self, _pool: &ThreadPool) -> Result<Self, ExecutorError> {
        Err(ExecutorError::Unsupported)
    }

    /// Attaches the executor to a green-thread pool later on.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` until green-thread-backed execution is implemented.
    pub const fn on_green(self, _green: &GreenPool) -> Result<Self, ExecutorError> {
        Err(ExecutorError::Unsupported)
    }
}
