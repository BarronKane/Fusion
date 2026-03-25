use core::num::NonZeroUsize;

/// Coarse realization regime the allocator layout policy is shaped for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AllocatorLayoutRealization {
    /// Backing is virtual or reservation-backed and only materializes lazily as needed.
    LazyVirtual,
    /// Backing is immediate physical or statically bound memory and should stay thin.
    EagerPhysical,
}

/// Allocator-facing layout policy carried alongside one governed resource.
///
/// This is intentionally distinct from [`super::MemoryGeometry`]. Geometry describes what the
/// machine can do to a range; layout policy describes how allocator metadata and extents should be
/// packed on top of that range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AllocatorLayoutPolicy {
    /// Granule used when front-loaded allocator metadata needs rounding.
    pub metadata_granule: NonZeroUsize,
    /// Minimum alignment allocator-managed extents should request from this resource.
    pub min_extent_align: NonZeroUsize,
    /// Default maximum alignment to assume for general bounded arenas on this resource.
    pub default_arena_align: NonZeroUsize,
    /// Default maximum alignment to assume for slab payloads when the caller has no stronger
    /// requirement.
    pub default_slab_align: NonZeroUsize,
    /// Broad realization regime this policy is shaped for.
    pub realization: AllocatorLayoutRealization,
}

impl AllocatorLayoutPolicy {
    /// Returns one exact, thin allocator layout policy for static or physically bound memory.
    #[must_use]
    pub const fn exact_static() -> Self {
        Self {
            metadata_granule: NonZeroUsize::new(1).expect("non-zero"),
            min_extent_align: NonZeroUsize::new(1).expect("non-zero"),
            default_arena_align: NonZeroUsize::new(16).expect("non-zero"),
            default_slab_align: NonZeroUsize::new(16).expect("non-zero"),
            realization: AllocatorLayoutRealization::EagerPhysical,
        }
    }

    /// Returns one hosted virtual-memory allocator layout policy shaped around the supplied page
    /// granule.
    #[must_use]
    pub const fn hosted_vm(page_granule: NonZeroUsize) -> Self {
        Self {
            metadata_granule: page_granule,
            min_extent_align: page_granule,
            default_arena_align: NonZeroUsize::new(64).expect("non-zero"),
            default_slab_align: NonZeroUsize::new(64).expect("non-zero"),
            realization: AllocatorLayoutRealization::LazyVirtual,
        }
    }
}
