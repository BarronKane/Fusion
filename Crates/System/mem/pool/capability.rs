use super::request::PoolBounds;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PoolCapabilitySet: u64 {
        const PRIVATE_BACKING   = 1 << 0;
        const SHARED_BACKING    = 1 << 1;
        const EXECUTABLE        = 1 << 2;
        const LOCKABLE          = 1 << 3;
        const POPULATE          = 1 << 4;
        const FIXED_NOREPLACE   = 1 << 5;
        const ADVISE            = 1 << 6;
        const QUERY             = 1 << 7;
        const ZERO_ON_FREE      = 1 << 8;
        const PHYSICAL          = 1 << 9;
        const DEVICE_LOCAL      = 1 << 10;
        const INTEGRITY         = 1 << 11;
        const CACHE_POLICY      = 1 << 12;
        const GROWABLE          = 1 << 13;
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct UnmetPoolPreferenceSet: u32 {
        const PLACEMENT    = 1 << 0;
        const POPULATE     = 1 << 1;
        const LOCK         = 1 << 2;
        const HUGE_PAGES   = 1 << 3;
        const ZERO_ON_FREE = 1 << 4;
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PoolHazardSet: u32 {
        const EXECUTABLE = 1 << 0;
        const SHARED     = 1 << 1;
        const EMULATED   = 1 << 2;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoolBackingKind {
    AnonymousPrivate,
    AnonymousShared,
    StaticRegion,
    Partition,
    DeviceLocal,
    Physical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolvedPoolConfig {
    pub backing: PoolBackingKind,
    pub bounds: PoolBounds,
    pub granted_capabilities: PoolCapabilitySet,
    pub unmet_preferences: UnmetPoolPreferenceSet,
    pub emulated_capabilities: PoolCapabilitySet,
    pub residual_hazards: PoolHazardSet,
}
