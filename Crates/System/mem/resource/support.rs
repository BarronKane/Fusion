use fusion_pal::sys::mem::{MemAdviceCaps, MemBackingCaps, MemPlacementCaps, Protect};

use super::domain::MemoryDomainSet;
use super::ops::{ResourceOpSet, ResourcePreferenceSet};

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ResourceResidencySupport: u32 {
        const BEST_EFFORT = 1 << 0;
        const PREFAULT    = 1 << 1;
        const LOCKED      = 1 << 2;
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ResourceFeatureSupport: u32 {
        const OVERCOMMIT_DISALLOW = 1 << 0;
        const CACHE_POLICY        = 1 << 1;
        const INTEGRITY           = 1 << 2;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceSupport {
    pub protect: Protect,
    pub ops: ResourceOpSet,
    pub advice: MemAdviceCaps,
    pub residency: ResourceResidencySupport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceAcquireSupport {
    pub domains: MemoryDomainSet,
    pub backings: MemBackingCaps,
    pub placements: MemPlacementCaps,
    pub instance: ResourceSupport,
    pub features: ResourceFeatureSupport,
    pub preferences: ResourcePreferenceSet,
}
