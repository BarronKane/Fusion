use fusion_pal::sys::mem::{CachePolicy, IntegrityMode, Placement, TagMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PoolBounds {
    pub initial_capacity: usize,
    pub max_capacity: Option<usize>,
    pub growable: bool,
}

impl PoolBounds {
    #[must_use]
    pub const fn fixed(capacity: usize) -> Self {
        Self {
            initial_capacity: capacity,
            max_capacity: Some(capacity),
            growable: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoolAccess {
    ReadWrite,
    ReadWriteExecute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoolSharing {
    Private,
    Shared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoolLatency {
    BestEffort,
    Prefault,
    Locked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IntegrityConstraints {
    pub mode: IntegrityMode,
    pub tag: Option<TagMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoolRequirement {
    Placement(Placement),
    Query,
    Locked,
    NoOvercommit,
    CachePolicy(CachePolicy),
    Integrity(IntegrityConstraints),
    DmaVisible,
    PhysicalContiguous,
    DeviceLocal,
    Shared,
    ZeroOnFree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoolPreference {
    Placement(Placement),
    Populate,
    Lock,
    HugePages,
    ZeroOnFree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoolProhibition {
    Executable,
    ReplaceMapping,
    Overcommit,
    Shared,
    DeviceLocal,
    Physical,
    Emulation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PoolRequest<'a> {
    pub name: Option<&'a str>,
    pub bounds: PoolBounds,
    pub access: PoolAccess,
    pub sharing: PoolSharing,
    pub latency: PoolLatency,
    pub requirements: &'a [PoolRequirement],
    pub preferences: &'a [PoolPreference],
    pub prohibitions: &'a [PoolProhibition],
}

impl<'a> PoolRequest<'a> {
    #[must_use]
    pub const fn new(
        bounds: PoolBounds,
        access: PoolAccess,
        sharing: PoolSharing,
        latency: PoolLatency,
        requirements: &'a [PoolRequirement],
        preferences: &'a [PoolPreference],
        prohibitions: &'a [PoolProhibition],
    ) -> Self {
        Self {
            name: None,
            bounds,
            access,
            sharing,
            latency,
            requirements,
            preferences,
            prohibitions,
        }
    }

    #[must_use]
    pub const fn anonymous_private(capacity: usize) -> Self {
        Self {
            name: None,
            bounds: PoolBounds::fixed(capacity),
            access: PoolAccess::ReadWrite,
            sharing: PoolSharing::Private,
            latency: PoolLatency::BestEffort,
            requirements: &[],
            preferences: &[],
            prohibitions: &[],
        }
    }
}
