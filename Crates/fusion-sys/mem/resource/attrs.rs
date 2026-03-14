bitflags::bitflags! {
    /// Intrinsic attributes of a memory resource independent of current page state.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ResourceAttrs: u32 {
        /// The range is suitable for general-purpose allocator or pool use.
        const ALLOCATABLE        = 1 << 0;
        /// The backing is fundamentally read-only even if other metadata is mutable.
        const READ_ONLY_BACKING  = 1 << 1;
        /// The range is visible to DMA-capable devices.
        const DMA_VISIBLE        = 1 << 2;
        /// The range resides in device-local memory rather than ordinary host memory.
        const DEVICE_LOCAL       = 1 << 3;
        /// The range participates in normal CPU caching.
        const CACHEABLE          = 1 << 4;
        /// The range participates in the expected coherency domain.
        const COHERENT           = 1 << 5;
        /// The backing is physically contiguous.
        const PHYS_CONTIGUOUS    = 1 << 6;
        /// The range carries hardware tag semantics.
        const TAGGED             = 1 << 7;
        /// The range participates in a platform integrity-management regime.
        const INTEGRITY_MANAGED  = 1 << 8;
        /// The range refers to a fixed static region rather than a fresh allocation.
        const STATIC_REGION      = 1 << 9;
        /// The range has MMIO-like or otherwise hazardous side-effecting behavior.
        const HAZARDOUS_IO       = 1 << 10;
        /// The range preserves state across power loss or restart until explicitly cleared.
        const PERSISTENT         = 1 << 11;
    }
}
