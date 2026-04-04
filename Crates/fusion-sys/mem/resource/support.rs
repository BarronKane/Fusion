use fusion_pal::sys::mem::{
    MemAdviceCaps,
    MemBackingCaps,
    MemPlacementCaps,
    Protect,
};

use super::domain::MemoryDomainSet;
use super::ops::{
    ResourceOpSet,
    ResourcePreferenceSet,
};

bitflags::bitflags! {
    /// Residency behaviors a live resource instance can support.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ResourceResidencySupport: u32 {
        /// Ordinary best-effort residency is supported.
        const BEST_EFFORT = 1 << 0;
        /// Prefault or eager population can be requested.
        ///
        /// Some backends may still treat this as best-effort rather than a postcondition that
        /// can be proven after acquisition.
        const PREFAULT    = 1 << 1;
        /// A verified lock or pin operation is supported.
        const LOCKED      = 1 << 2;
    }
}

bitflags::bitflags! {
    /// Optional acquisition-time features a backend may expose when creating resources.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ResourceFeatureSupport: u32 {
        /// Stronger no-overcommit acquisition semantics are available.
        const OVERCOMMIT_DISALLOW = 1 << 0;
        /// Non-default cache-policy requests are available.
        const CACHE_POLICY        = 1 << 1;
        /// Integrity or tag-mode acquisition requests are available.
        const INTEGRITY           = 1 << 2;
    }
}

/// Runtime support surface of a live resource instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceSupport {
    /// Protection bits that this instance can legally hold.
    pub protect: Protect,
    /// Operations the instance may expose through extension traits.
    pub ops: ResourceOpSet,
    /// Advisory hints accepted for this instance.
    pub advice: MemAdviceCaps,
    /// Residency behaviors accepted for this instance.
    pub residency: ResourceResidencySupport,
}

/// Acquisition support surface for creating or binding resources on a platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceAcquireSupport {
    /// Memory domains the acquisition path can produce.
    pub domains: MemoryDomainSet,
    /// Backing kinds the acquisition path can create.
    pub backings: MemBackingCaps,
    /// Placement modes that can be requested at acquisition time.
    pub placements: MemPlacementCaps,
    /// Runtime support surface expected on created instances.
    pub instance: ResourceSupport,
    /// Optional acquisition-only feature support.
    pub features: ResourceFeatureSupport,
    /// Soft preferences the backend may try to honor at acquisition time.
    pub preferences: ResourcePreferenceSet,
}
