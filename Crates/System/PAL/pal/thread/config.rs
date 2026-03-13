use super::{ThreadPlacementRequest, ThreadSchedulerRequest, ThreadStackRequest};

/// Joinability policy requested for a new thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadJoinPolicy {
    /// The thread is joinable and must be explicitly joined or detached later.
    Joinable,
    /// The thread is detached and releases its resources without a future join.
    Detached,
}

/// Startup barrier semantics requested for a new thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadStartMode {
    /// Enter user code as soon as the backend starts the thread.
    Immediate,
    /// Prefer to apply placement before user entry begins.
    PlacementCommitted,
    /// Require placement and stack startup policy to be committed before user entry.
    PlacementAndStackCommitted,
}

/// Full requested configuration for a new thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadConfig<'a> {
    /// Joinability policy for the new thread.
    pub join_policy: ThreadJoinPolicy,
    /// Optional human-readable thread name.
    pub name: Option<&'a str>,
    /// Startup barrier semantics for entry.
    pub start_mode: ThreadStartMode,
    /// Requested placement policy.
    pub placement: ThreadPlacementRequest<'a>,
    /// Requested scheduler policy.
    pub scheduler: ThreadSchedulerRequest,
    /// Requested stack and startup-memory policy.
    pub stack: ThreadStackRequest,
}

impl ThreadConfig<'_> {
    /// Returns a thread configuration that inherits backend defaults.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            join_policy: ThreadJoinPolicy::Joinable,
            name: None,
            start_mode: ThreadStartMode::Immediate,
            placement: ThreadPlacementRequest::new(),
            scheduler: ThreadSchedulerRequest::new(),
            stack: ThreadStackRequest::new(),
        }
    }
}

impl Default for ThreadConfig<'_> {
    fn default() -> Self {
        Self::new()
    }
}
