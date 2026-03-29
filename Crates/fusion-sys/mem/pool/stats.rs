/// Snapshot of pool-wide capacity and metadata usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryPoolStats {
    /// Total bytes governed by all pool members.
    pub total_bytes: usize,
    /// Currently free bytes available for new extent leases.
    pub free_bytes: usize,
    /// Bytes currently leased out of the pool.
    pub leased_bytes: usize,
    /// Largest currently free contiguous extent in bytes.
    pub largest_free_extent: usize,
    /// Number of member resources owned by the pool.
    pub member_count: usize,
    /// Number of currently free extent records.
    pub free_extent_count: usize,
    /// Number of currently leased extent records.
    pub leased_extent_count: usize,
    /// Total fixed extent-tracking slots compiled into the pool.
    pub extent_slot_capacity: usize,
    /// Number of extent-tracking slots currently populated.
    pub extent_slots_used: usize,
    /// Number of currently vacant extent-tracking slots.
    pub extent_slots_free: usize,
}
