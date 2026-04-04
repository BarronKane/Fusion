use super::*;

/// Global sizing strategy for runtime-owned slabs, arenas, and derived envelopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeSizingStrategy {
    /// Use the exact counted requirement with no extra headroom.
    Exact,
    /// Add 20% headroom and round up to the next power-of-two envelope.
    GlobalNearestRoundUp,
}

impl RuntimeSizingStrategy {
    /// Returns the effective byte requirement for one counted backing size under this strategy.
    #[must_use]
    pub fn apply_bytes(self, bytes: usize) -> Option<usize> {
        match self {
            Self::Exact => Some(bytes),
            Self::GlobalNearestRoundUp => {
                if bytes == 0 {
                    return Some(0);
                }
                let extra = bytes.checked_add(4)? / 5;
                let padded = bytes.checked_add(extra)?;
                padded.checked_next_power_of_two()
            }
        }
    }
}

/// Returns the crate-wide default runtime sizing strategy selected by features.
#[must_use]
pub const fn default_runtime_sizing_strategy() -> RuntimeSizingStrategy {
    #[cfg(feature = "sizing-global-nearest-round-up")]
    {
        RuntimeSizingStrategy::GlobalNearestRoundUp
    }
    #[cfg(not(feature = "sizing-global-nearest-round-up"))]
    {
        RuntimeSizingStrategy::Exact
    }
}

/// Unified backing request for one combined runtime-owned slab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RuntimeBackingRequest {
    /// Requested byte count.
    pub bytes: usize,
    /// Required slab alignment.
    pub align: usize,
}

/// Explicit one-slab backing plan for one current-thread fiber + async runtime bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurrentFiberAsyncRuntimeBackingPlan {
    /// Total owning slab request for the combined runtime bundle.
    pub slab: RuntimeBackingRequest,
    /// Fiber-runtime sub-slab range inside the owning slab.
    pub fibers: ResourceRange,
    /// Async-runtime sub-slab range inside the owning slab.
    pub executor: ResourceRange,
    /// Nested current-thread fiber plan for the fiber sub-slab.
    pub fiber_plan: CurrentFiberPoolCombinedBackingPlan,
    /// Nested current-thread async plan for the async sub-slab.
    pub executor_plan: CurrentAsyncRuntimeCombinedBackingPlan,
}

/// Error for combined current-thread fiber + async bootstrap work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CurrentFiberAsyncRuntimeError {
    /// Fiber bootstrap failed honestly.
    Fiber(fusion_sys::fiber::FiberError),
    /// Async runtime bootstrap failed honestly.
    Executor(super::ExecutorError),
}

/// Ergonomic current-thread bootstrap for one fiber pool plus one async runtime.
#[derive(Debug, Clone, Copy)]
pub struct CurrentFiberAsyncBootstrap<'a> {
    pub(super) fibers: FiberPoolBootstrap<'a>,
    pub(super) executor: ExecutorConfig,
}

/// One current-thread runtime bundle containing both fibers and async.
#[derive(Debug)]
pub struct CurrentFiberAsyncRuntime {
    pub(super) fibers: CurrentFiberPool,
    pub(super) executor: CurrentAsyncRuntime,
}

pub(super) fn selected_stack_size_with_optional_floor(
    stack_floor_bytes: Option<usize>,
) -> Result<NonZeroUsize, FiberError> {
    let requested = generated_default_fiber_stack_bytes()?.max(stack_floor_bytes.unwrap_or(0));
    let requested = NonZeroUsize::new(requested).ok_or_else(FiberError::invalid)?;
    Ok(FiberStackClass::from_stack_bytes(requested)?.size_bytes())
}

pub(super) fn current_thread_default_guard_pages() -> usize {
    usize::from(FiberSystem::new().support().context.guard_required)
}

pub(super) fn executor_error_from_fiber(error: FiberError) -> ExecutorError {
    match error.kind() {
        FiberErrorKind::Unsupported => ExecutorError::Unsupported,
        FiberErrorKind::Invalid => ExecutorError::Sync(SyncErrorKind::Invalid),
        FiberErrorKind::ResourceExhausted => ExecutorError::Sync(SyncErrorKind::Overflow),
        FiberErrorKind::DeadlineExceeded | FiberErrorKind::StateConflict => {
            ExecutorError::Sync(SyncErrorKind::Busy)
        }
        FiberErrorKind::Context(_) => ExecutorError::Sync(SyncErrorKind::Busy),
    }
}

pub(super) fn fiber_error_from_executor(error: ExecutorError) -> FiberError {
    match error {
        ExecutorError::Unsupported => FiberError::unsupported(),
        ExecutorError::Stopped | ExecutorError::Cancelled => FiberError::state_conflict(),
        ExecutorError::Sync(kind) => match kind {
            SyncErrorKind::Invalid => FiberError::invalid(),
            SyncErrorKind::Overflow => FiberError::resource_exhausted(),
            _ => FiberError::state_conflict(),
        },
        ExecutorError::TaskPanicked => FiberError::state_conflict(),
    }
}

pub(super) fn executor_error_from_current_runtime(
    error: CurrentFiberAsyncRuntimeError,
) -> ExecutorError {
    match error {
        CurrentFiberAsyncRuntimeError::Fiber(error) => executor_error_from_fiber(error),
        CurrentFiberAsyncRuntimeError::Executor(error) => error,
    }
}

pub(super) fn executor_error_from_runtime_sync(error: crate::sync::SyncError) -> ExecutorError {
    match error.kind {
        SyncErrorKind::Unsupported => ExecutorError::Unsupported,
        SyncErrorKind::Invalid => ExecutorError::Sync(SyncErrorKind::Invalid),
        SyncErrorKind::Overflow => ExecutorError::Sync(SyncErrorKind::Overflow),
        SyncErrorKind::Busy | SyncErrorKind::PermissionDenied | SyncErrorKind::Platform(_) => {
            ExecutorError::Sync(SyncErrorKind::Busy)
        }
    }
}

pub(super) fn executor_error_from_alloc(error: AllocError) -> ExecutorError {
    match error.kind {
        AllocErrorKind::Unsupported | AllocErrorKind::PolicyDenied => ExecutorError::Unsupported,
        AllocErrorKind::InvalidRequest | AllocErrorKind::InvalidDomain => executor_invalid(),
        AllocErrorKind::Busy | AllocErrorKind::SynchronizationFailure(_) => executor_busy(),
        AllocErrorKind::MetadataExhausted
        | AllocErrorKind::CapacityExhausted
        | AllocErrorKind::OutOfMemory
        | AllocErrorKind::ResourceFailure(_)
        | AllocErrorKind::PoolFailure(_) => executor_overflow(),
    }
}

pub(super) const fn executor_invalid() -> ExecutorError {
    ExecutorError::Sync(SyncErrorKind::Invalid)
}

pub(super) const fn executor_busy() -> ExecutorError {
    ExecutorError::Sync(SyncErrorKind::Busy)
}

pub(super) const fn executor_resource_exhausted() -> ExecutorError {
    ExecutorError::Sync(SyncErrorKind::Overflow)
}

pub(super) const fn executor_overflow() -> ExecutorError {
    ExecutorError::Sync(SyncErrorKind::Overflow)
}

pub(super) fn next_runtime_capacity(
    current: usize,
    required_minimum: usize,
) -> Result<usize, ExecutorError> {
    let mut next = current.max(1);
    if required_minimum <= next {
        return next.checked_mul(2).ok_or_else(executor_overflow);
    }
    while next < required_minimum {
        next = next.checked_mul(2).ok_or_else(executor_overflow)?;
    }
    Ok(next)
}

pub(super) fn initial_runtime_capacity(
    limit: Option<usize>,
    requested_capacity: Option<usize>,
) -> Option<usize> {
    let requested = requested_capacity.unwrap_or(1).max(1);
    match limit {
        Some(limit) if requested > limit => None,
        Some(_) | None => Some(requested),
    }
}

pub(super) fn next_bounded_runtime_capacity(
    current: usize,
    required_minimum: usize,
    limit: Option<usize>,
) -> Result<Option<usize>, ExecutorError> {
    if let Some(limit) = limit
        && required_minimum > limit
    {
        return Ok(None);
    }
    let next = if current == 0 {
        required_minimum.max(1)
    } else {
        next_runtime_capacity(current, required_minimum)?
    };
    Ok(Some(match limit {
        Some(limit) => next.min(limit),
        None => next,
    }))
}
