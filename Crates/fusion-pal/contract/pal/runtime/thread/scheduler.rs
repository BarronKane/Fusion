use core::time::Duration;

/// Scheduler model used by the backend for runnable threads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadSchedulerModel {
    /// The backend cannot classify the scheduler model honestly.
    Unknown,
    /// Runnable threads may be involuntarily preempted according to scheduler policy.
    Preemptive,
    /// Runnable threads execute until they block or yield cooperatively.
    Cooperative,
    /// The backend mixes preemptive and cooperative behavior depending on class or mode.
    Hybrid,
}

/// High-level scheduler class requested for a thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadSchedulerClass {
    /// Inherit the platform or process default scheduler class.
    Inherit,
    /// Use the normal time-sharing scheduler class.
    Default,
    /// Use a background or low-importance scheduler class.
    Background,
    /// Use a fixed-priority realtime class when supported.
    FixedPriorityRealtime,
    /// Use a round-robin realtime class when supported.
    RoundRobinRealtime,
    /// Use a deadline-based scheduler class when supported.
    Deadline,
    /// Use a backend-specific scheduler class.
    VendorSpecific(u16),
}

/// Numeric scheduler priority value.
///
/// Valid ranges and ordering semantics are scheduler-class specific. Callers should query
/// the backend's class-specific priority range before constructing strict scheduler
/// requests rather than assuming the platform uses a familiar scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadPriority(pub i32);

/// Relative strength ordering for numeric scheduler priorities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadPriorityOrder {
    /// Larger numeric values represent stronger scheduling priority.
    HigherIsStronger,
    /// Smaller numeric values represent stronger scheduling priority.
    LowerIsStronger,
}

/// Valid numeric priority range for a scheduler class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadPriorityRange {
    /// Smallest numeric priority accepted for the class.
    pub min: ThreadPriority,
    /// Largest numeric priority accepted for the class.
    pub max: ThreadPriority,
    /// Ordering relationship between numeric value and effective strength.
    pub ordering: ThreadPriorityOrder,
}

impl ThreadPriorityRange {
    /// Returns `true` when the supplied priority is within the accepted range.
    #[must_use]
    pub const fn contains(self, priority: ThreadPriority) -> bool {
        priority.0 >= self.min.0 && priority.0 <= self.max.0
    }
}

/// Deadline scheduler parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadDeadlineRequest {
    /// Execution budget per period.
    pub runtime: Duration,
    /// Relative deadline within each period.
    pub deadline: Duration,
    /// Scheduling period.
    pub period: Duration,
}

/// Requested scheduler policy for a thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadSchedulerRequest {
    /// Requested scheduler class.
    pub class: ThreadSchedulerClass,
    /// Requested numeric priority, if any.
    ///
    /// Callers should treat this as class-specific input validated against the backend's
    /// reported priority range rather than as a portable global scale.
    pub priority: Option<ThreadPriority>,
    /// Requested deadline parameters, if any.
    pub deadline: Option<ThreadDeadlineRequest>,
}

impl ThreadSchedulerRequest {
    /// Returns a scheduler request that inherits backend defaults.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            class: ThreadSchedulerClass::Inherit,
            priority: None,
            deadline: None,
        }
    }
}

impl Default for ThreadSchedulerRequest {
    fn default() -> Self {
        Self::new()
    }
}

/// Effective scheduler policy observed for a thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadSchedulerObservation {
    /// Effective scheduler class, when observable.
    pub class: Option<ThreadSchedulerClass>,
    /// Assigned or configured numeric priority, when observable.
    pub base_priority: Option<ThreadPriority>,
    /// Effective runtime priority after inheritance or backend boosting, when observable.
    pub effective_priority: Option<ThreadPriority>,
}

impl ThreadSchedulerObservation {
    /// Returns an empty scheduler observation.
    #[must_use]
    pub const fn unknown() -> Self {
        Self {
            class: None,
            base_priority: None,
            effective_priority: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ThreadPriority,
        ThreadPriorityOrder,
        ThreadPriorityRange,
    };

    #[test]
    fn priority_range_contains_inclusive_bounds() {
        let range = ThreadPriorityRange {
            min: ThreadPriority(1),
            max: ThreadPriority(99),
            ordering: ThreadPriorityOrder::HigherIsStronger,
        };

        assert!(range.contains(ThreadPriority(1)));
        assert!(range.contains(ThreadPriority(50)));
        assert!(range.contains(ThreadPriority(99)));
        assert!(!range.contains(ThreadPriority(0)));
        assert!(!range.contains(ThreadPriority(100)));
    }
}
