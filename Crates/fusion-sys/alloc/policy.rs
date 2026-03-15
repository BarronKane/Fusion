use crate::mem::provider::CriticalSafetyRequirements;

bitflags::bitflags! {
    /// Allocator families and installation modes permitted by policy.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct AllocModeSet: u32 {
        /// Permits slab allocation.
        const SLAB         = 1 << 0;
        /// Permits bounded arena allocation.
        const ARENA        = 1 << 1;
        /// Permits general-purpose heap allocation.
        const HEAP         = 1 << 2;
        /// Permits global allocator installation.
        const GLOBAL_ALLOC = 1 << 3;
    }
}

bitflags::bitflags! {
    /// Coarse allocator capabilities that higher layers may gate on.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct AllocCapabilities: u32 {
        /// Supports fixed-size slab allocation.
        const SLAB          = 1 << 0;
        /// Supports bounded arena allocation.
        const ARENA         = 1 << 1;
        /// Supports general-purpose heap allocation.
        const HEAP          = 1 << 2;
        /// Supports zero-initialized allocation requests.
        const ZEROED_ALLOC  = 1 << 3;
        /// Supports realloc-style growth or shrink operations.
        const REALLOC       = 1 << 4;
        /// Supports a process-global allocator installation story.
        const GLOBAL_ALLOC  = 1 << 5;
        /// Worst-case allocation behavior is intended to be deterministic.
        const DETERMINISTIC = 1 << 6;
        /// Capacity is statically bounded rather than elastically growing.
        const BOUNDED       = 1 << 7;
    }
}

bitflags::bitflags! {
    /// Coarse allocator hazards that higher layers may reject explicitly.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct AllocHazards: u32 {
        /// Allocation behavior may fragment over time.
        const FRAGMENTATION    = 1 << 0;
        /// Allocation latency may vary with current heap or pool state.
        const VARIABLE_LATENCY = 1 << 1;
        /// Allocation may require external growth or provisioning.
        const EXTERNAL_GROWTH  = 1 << 2;
        /// Allocation may rely on overcommit-like semantics.
        const OVERCOMMIT       = 1 << 3;
        /// Allocation may rely on emulated lower-level semantics.
        const EMULATED         = 1 << 4;
        /// Allocation may block while coordinating shared state.
        const MAY_BLOCK        = 1 << 5;
    }
}

/// Allocation policy controlling which allocator families are legal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AllocPolicy {
    /// Allocator families and installation modes permitted by this policy.
    pub modes: AllocModeSet,
    /// Safety requirements imposed on the backing memory substrate.
    pub safety: CriticalSafetyRequirements,
}

impl AllocPolicy {
    /// Returns a conservative critical-safe allocation policy.
    #[must_use]
    pub const fn new() -> Self {
        Self::critical_safe()
    }

    /// Returns a conservative critical-safe allocation policy.
    #[must_use]
    pub const fn critical_safe() -> Self {
        Self {
            modes: AllocModeSet::SLAB.union(AllocModeSet::ARENA),
            safety: CriticalSafetyRequirements::POOLABLE
                .union(CriticalSafetyRequirements::DETERMINISTIC_CAPACITY)
                .union(CriticalSafetyRequirements::PRIVATE_ONLY)
                .union(CriticalSafetyRequirements::NO_SHARED_ALIASING)
                .union(CriticalSafetyRequirements::NO_EXTERNAL_MUTATION)
                .union(CriticalSafetyRequirements::NO_EMULATION)
                .union(CriticalSafetyRequirements::REQUIRE_COHERENT)
                .union(CriticalSafetyRequirements::NO_HAZARDOUS_IO)
                .union(CriticalSafetyRequirements::EXECUTE_NEVER),
        }
    }

    /// Returns a conservative slab-only allocation policy.
    #[must_use]
    pub const fn slab_only() -> Self {
        Self {
            modes: AllocModeSet::SLAB,
            safety: Self::critical_safe().safety,
        }
    }

    /// Returns a conservative bounded-arena-only allocation policy.
    #[must_use]
    pub const fn arena_only() -> Self {
        Self {
            modes: AllocModeSet::ARENA,
            safety: Self::critical_safe().safety,
        }
    }

    /// Returns a more permissive general-purpose allocation policy.
    #[must_use]
    pub const fn general_purpose() -> Self {
        Self {
            modes: AllocModeSet::SLAB
                .union(AllocModeSet::ARENA)
                .union(AllocModeSet::HEAP),
            safety: CriticalSafetyRequirements::empty(),
        }
    }

    /// Returns `true` when this policy permits `mode`.
    #[must_use]
    pub const fn allows(self, mode: AllocModeSet) -> bool {
        self.modes.contains(mode)
    }
}

impl Default for AllocPolicy {
    fn default() -> Self {
        Self::new()
    }
}
