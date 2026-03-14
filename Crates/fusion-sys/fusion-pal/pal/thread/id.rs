/// Stable identifier for a thread as surfaced by a fusion-pal backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadId(pub u64);

/// Stability guarantee for thread identifiers returned by a backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadIdentityStability {
    /// The backend cannot characterize identifier reuse honestly.
    Unknown,
    /// Identifiers are unique only for the lifetime of the live thread and may be reused
    /// after the thread exits.
    ThreadLifetime,
    /// Identifiers are never reused within the lifetime of the hosting process or runtime
    /// instance.
    ProcessLifetime,
    /// Identifiers are never reused for the lifetime of the running system instance.
    SystemLifetime,
}

/// Identifier for a processor group or analogous scheduler-visible CPU bank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadProcessorGroupId(pub u16);

/// Scheduler-visible logical CPU identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadLogicalCpuId {
    /// Processor group containing this logical CPU.
    pub group: ThreadProcessorGroupId,
    /// Index of the logical CPU within the processor group.
    pub index: u16,
}

/// Stable identifier for a physical or topology-defined core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadCoreId(pub u32);

/// Stable identifier for a cluster of cores, such as a compute tile or LLC domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadClusterId(pub u32);

/// Stable identifier for a heterogeneous core class or performance bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadCoreClassId(pub u16);
