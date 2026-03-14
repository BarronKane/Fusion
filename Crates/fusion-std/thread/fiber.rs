//! Domain 2: public green-thread and fiber orchestration surface.

use core::num::NonZeroUsize;

use fusion_sys::fiber::{FiberError, FiberSupport, FiberSystem};

use super::ThreadPool;

/// Scheduling policy for green threads on top of carrier workers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GreenScheduling {
    /// Simple FIFO scheduling across carriers.
    Fifo,
    /// Priority-aware scheduling across carriers.
    Priority,
    /// Per-carrier deque scheduling with work stealing.
    WorkStealing,
}

/// Growth policy for the green-thread pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GreenGrowth {
    /// Fixed-capacity pool with explicit admission control.
    Fixed,
    /// Grow green-thread population on demand up to the configured cap.
    OnDemand,
}

/// Public green-thread pool configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GreenPoolConfig {
    /// Per-green-thread stack size.
    pub stack_size: NonZeroUsize,
    /// Guard size for each green-thread stack.
    pub guard_bytes: usize,
    /// Maximum live green threads admitted by the pool.
    pub max_green_threads: usize,
    /// Scheduling policy across carriers.
    pub scheduling: GreenScheduling,
    /// Population growth policy.
    pub growth: GreenGrowth,
}

/// Opaque public green-thread handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GreenHandle {
    id: u64,
}

impl GreenHandle {
    /// Returns the stable green-thread identifier.
    #[must_use]
    pub const fn id(self) -> u64 {
        self.id
    }
}

/// Public green-thread pool wrapper.
#[derive(Debug, Clone, Copy)]
pub struct GreenPool {
    support: FiberSupport,
}

impl GreenPool {
    /// Returns the low-level fiber support available on the current backend.
    #[must_use]
    pub fn support() -> FiberSupport {
        FiberSystem::new().support()
    }

    /// Creates a green-thread pool on top of the supplied carrier pool.
    ///
    /// # Errors
    ///
    /// Returns an honest unsupported error until the low-level fiber primitive and green
    /// scheduler are implemented.
    pub const fn new(_config: &GreenPoolConfig, _carrier: &ThreadPool) -> Result<Self, FiberError> {
        Err(FiberError::unsupported())
    }

    /// Returns the currently configured low-level support surface.
    #[must_use]
    pub const fn fiber_support(&self) -> FiberSupport {
        self.support
    }
}

/// Yields the current green thread cooperatively.
///
/// # Errors
///
/// Returns an honest unsupported error until the green-thread scheduler exists.
pub const fn yield_now() -> Result<(), FiberError> {
    Err(FiberError::unsupported())
}
