//! Invocation-model vocabulary for the PCU IR core.

/// Topology shape for one kernel's execution model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuInvocationTopology {
    Single,
    Grid { workgroup_size: [u32; 3] },
    Continuous,
    Triggered,
}

/// Parallelism relationship between simultaneously active invocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuInvocationParallelism {
    Serial,
    Independent,
    Cooperative,
    Lockstep,
}

/// Progress/lifetime model for one invocation family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuInvocationProgress {
    Finite,
    Persistent,
    Continuous,
}

/// Ordering contract for work issued through one invocation model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuInvocationOrdering {
    Unordered,
    InOrder,
    PerPort,
}

/// Full invocation model for one kernel profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuInvocationModel {
    pub topology: PcuInvocationTopology,
    pub parallelism: PcuInvocationParallelism,
    pub progress: PcuInvocationProgress,
    pub ordering: PcuInvocationOrdering,
}

impl PcuInvocationModel {
    /// Creates one single-shot serial invocation model.
    #[must_use]
    pub const fn single() -> Self {
        Self {
            topology: PcuInvocationTopology::Single,
            parallelism: PcuInvocationParallelism::Serial,
            progress: PcuInvocationProgress::Finite,
            ordering: PcuInvocationOrdering::InOrder,
        }
    }

    /// Creates one independent grid invocation model with the supplied workgroup size.
    #[must_use]
    pub const fn grid(workgroup_size: [u32; 3]) -> Self {
        Self {
            topology: PcuInvocationTopology::Grid { workgroup_size },
            parallelism: PcuInvocationParallelism::Independent,
            progress: PcuInvocationProgress::Finite,
            ordering: PcuInvocationOrdering::Unordered,
        }
    }

    /// Creates one continuous stream-oriented invocation model.
    #[must_use]
    pub const fn continuous() -> Self {
        Self {
            topology: PcuInvocationTopology::Continuous,
            parallelism: PcuInvocationParallelism::Lockstep,
            progress: PcuInvocationProgress::Continuous,
            ordering: PcuInvocationOrdering::PerPort,
        }
    }
}
