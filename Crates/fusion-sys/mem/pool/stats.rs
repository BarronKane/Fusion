/// Snapshot of pool-wide capacity and metadata usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryPoolStats {
    /// Total bytes governed by all pool members.
    pub total_bytes: usize,
    /// Currently free bytes available for new extent leases.
    pub free_bytes: usize,
    /// Bytes currently leased out of the pool.
    pub leased_bytes: usize,
    /// Number of member resources owned by the pool.
    pub member_count: usize,
    /// Number of currently free extent records.
    pub free_extent_count: usize,
    /// Number of currently leased extent records.
    pub leased_extent_count: usize,
}
