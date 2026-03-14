//! Domain 4: public task-graph surface.

use super::{Executor, GreenPool, ThreadPool};

/// Public task target kind for graph dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskTargetKind {
    /// Dispatch to a system carrier pool.
    ThreadPool,
    /// Dispatch to a green-thread pool.
    GreenPool,
    /// Dispatch to an async executor.
    Executor,
}

/// Trait implemented by task-dispatch targets.
pub trait TaskTarget {
    /// Returns the public kind of dispatch target.
    fn target_kind(&self) -> TaskTargetKind;
}

impl TaskTarget for ThreadPool {
    fn target_kind(&self) -> TaskTargetKind {
        TaskTargetKind::ThreadPool
    }
}

impl TaskTarget for GreenPool {
    fn target_kind(&self) -> TaskTargetKind {
        TaskTargetKind::GreenPool
    }
}

impl TaskTarget for Executor {
    fn target_kind(&self) -> TaskTargetKind {
        TaskTargetKind::Executor
    }
}

/// Handle to a task node in a graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskNode(pub u32);

/// Batch of dispatched tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskBatch;

/// Planned task-slab configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskSlab {
    /// Maximum number of task nodes carried in the slab.
    pub capacity: usize,
}

/// Opaque job description placeholder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Job {
    /// Stable opaque job identifier.
    pub id: u64,
}

/// Public task-graph surface.
#[derive(Debug, Default, Clone, Copy)]
pub struct TaskGraph;

impl TaskGraph {
    /// Creates a new empty graph surface.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Returns whether the graph currently contains no planned tasks.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        true
    }
}
