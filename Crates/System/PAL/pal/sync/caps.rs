use bitflags::bitflags;

/// Indicates whether a capability is native, emulated, spin-based, or unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyncImplementationKind {
    /// The backend uses a native operating-system primitive directly.
    Native,
    /// The backend emulates the primitive with lower-level facilities.
    Emulated,
    /// The backend falls back to a spin-only implementation.
    SpinOnly,
    /// The backend does not support the primitive at all.
    Unsupported,
}

/// Priority inversion handling offered by the backend for a mutex primitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PriorityInheritanceSupport {
    /// No priority-inheritance semantics are available.
    None,
    /// The primitive always carries priority-inheritance semantics.
    Implicit,
    /// Priority-inheritance semantics can be selected explicitly.
    Configurable,
}

/// Recursive locking support offered by the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecursionSupport {
    /// Recursive locking is not supported.
    None,
    /// Recursive locking is emulated above a non-recursive backend primitive.
    Emulated,
    /// Recursive locking is provided natively by the backend.
    Native,
}

/// Robust owner-death semantics offered by the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RobustnessSupport {
    /// The backend cannot report owner death.
    None,
    /// The backend can report owner death and recoverability semantics.
    OwnerDeath,
}

/// Process-sharing scope offered by the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProcessScopeSupport {
    /// The primitive is local to the current process.
    LocalOnly,
    /// The primitive can be shared across processes.
    ProcessShared,
}

bitflags! {
    /// Timeout semantics supported by a synchronization primitive.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct TimeoutCaps: u32 {
        /// Supports relative timeouts.
        const RELATIVE = 1 << 0;
        /// Relative timeouts are measured against a monotonic clock.
        const RELATIVE_MONOTONIC = 1 << 1;
        /// Supports absolute timeouts on a monotonic clock.
        const ABSOLUTE_MONOTONIC = 1 << 2;
        /// Supports absolute timeouts on a realtime/wall clock.
        const ABSOLUTE_REALTIME = 1 << 3;
    }
}

bitflags! {
    /// Capability flags for a raw wait/wake primitive.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct WaitCaps: u32 {
        /// Supports waiting while a word remains equal to an expected value.
        const WAIT_WHILE_EQUAL = 1 << 0;
        /// Supports waking a single waiter.
        const WAKE_ONE         = 1 << 1;
        /// Supports waking all waiters on a word.
        const WAKE_ALL         = 1 << 2;
        /// Wait operations may return spuriously and require caller-side rechecking.
        const SPURIOUS_WAKE    = 1 << 3;
    }
}

bitflags! {
    /// Capability flags for a raw mutex primitive.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MutexCaps: u32 {
        /// Supports non-blocking acquisition attempts.
        const TRY_LOCK     = 1 << 0;
        /// Supports blocking acquisition.
        const BLOCKING     = 1 << 1;
        /// Supports relative-timeout acquisition attempts.
        const LOCK_FOR     = 1 << 2;
        /// Supports static initialization without heap allocation.
        const STATIC_INIT  = 1 << 3;
    }
}

bitflags! {
    /// Capability flags for a counting semaphore primitive.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct SemaphoreCaps: u32 {
        /// Supports non-blocking acquire attempts.
        const TRY_ACQUIRE  = 1 << 0;
        /// Supports blocking acquire operations.
        const BLOCKING     = 1 << 1;
        /// Supports relative-timeout acquire attempts.
        const ACQUIRE_FOR  = 1 << 2;
        /// Supports releasing more than one permit at a time.
        const RELEASE_MANY = 1 << 3;
    }
}

/// Raw wait/wake support offered by a backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WaitSupport {
    /// Fine-grained wait/wake capability flags.
    pub caps: WaitCaps,
    /// Supported timeout models for wait operations.
    pub timeout: TimeoutCaps,
    /// Process-sharing semantics, if any.
    pub process_scope: ProcessScopeSupport,
    /// Whether the backend implementation is native, emulated, or unavailable.
    pub implementation: SyncImplementationKind,
}

impl WaitSupport {
    /// Returns an explicitly unsupported wait surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: WaitCaps::empty(),
            timeout: TimeoutCaps::empty(),
            process_scope: ProcessScopeSupport::LocalOnly,
            implementation: SyncImplementationKind::Unsupported,
        }
    }
}

/// Raw mutex support offered by a backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MutexSupport {
    /// Fine-grained mutex capability flags.
    pub caps: MutexCaps,
    /// Supported timeout models for acquisition attempts.
    pub timeout: TimeoutCaps,
    /// Priority-inheritance semantics of this mutex primitive.
    pub priority_inheritance: PriorityInheritanceSupport,
    /// Recursive-locking semantics of this mutex primitive.
    pub recursion: RecursionSupport,
    /// Robust owner-death semantics of this mutex primitive.
    pub robustness: RobustnessSupport,
    /// Process-sharing semantics, if any.
    pub process_scope: ProcessScopeSupport,
    /// Whether the backend implementation is native, emulated, or unavailable.
    pub implementation: SyncImplementationKind,
}

impl MutexSupport {
    /// Returns an explicitly unsupported mutex surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: MutexCaps::empty(),
            timeout: TimeoutCaps::empty(),
            priority_inheritance: PriorityInheritanceSupport::None,
            recursion: RecursionSupport::None,
            robustness: RobustnessSupport::None,
            process_scope: ProcessScopeSupport::LocalOnly,
            implementation: SyncImplementationKind::Unsupported,
        }
    }
}

/// Counting semaphore support offered by a backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SemaphoreSupport {
    /// Fine-grained semaphore capability flags.
    pub caps: SemaphoreCaps,
    /// Supported timeout models for acquire attempts.
    pub timeout: TimeoutCaps,
    /// Process-sharing semantics, if any.
    pub process_scope: ProcessScopeSupport,
    /// Whether the backend implementation is native, emulated, or unavailable.
    pub implementation: SyncImplementationKind,
}

impl SemaphoreSupport {
    /// Returns an explicitly unsupported semaphore surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: SemaphoreCaps::empty(),
            timeout: TimeoutCaps::empty(),
            process_scope: ProcessScopeSupport::LocalOnly,
            implementation: SyncImplementationKind::Unsupported,
        }
    }
}

/// Aggregated synchronization support surface for a backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SyncSupport {
    /// Raw wait/wake support.
    pub wait: WaitSupport,
    /// Raw mutex support.
    pub mutex: MutexSupport,
    /// Counting semaphore support.
    pub semaphore: SemaphoreSupport,
}

impl SyncSupport {
    /// Returns a backend with no supported synchronization primitives.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            wait: WaitSupport::unsupported(),
            mutex: MutexSupport::unsupported(),
            semaphore: SemaphoreSupport::unsupported(),
        }
    }
}
