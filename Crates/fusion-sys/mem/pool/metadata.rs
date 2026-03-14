/// Fixed metadata layout used by a concrete `MemoryPool` instantiation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryPoolMetadataLayout {
    /// Maximum number of contributors the pool can own.
    pub max_members: usize,
    /// Maximum number of extent records the pool can track across free and leased states.
    pub max_extents: usize,
    /// Minimum extent records needed to represent one initial free extent per member.
    pub initial_extents_required: usize,
}

impl MemoryPoolMetadataLayout {
    /// Returns the layout implied by `MemoryPool<MEMBERS, EXTENTS>`.
    #[must_use]
    pub const fn for_capacities<const MEMBERS: usize, const EXTENTS: usize>() -> Self {
        Self {
            max_members: MEMBERS,
            max_extents: EXTENTS,
            initial_extents_required: MEMBERS,
        }
    }
}
