bitflags::bitflags! {
    /// Set of memory domains a provider or support surface can describe.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MemoryDomainSet: u32 {
        /// Ordinary virtual-address-backed memory.
        const VIRTUAL_ADDRESS_SPACE = 1 << 0;
        /// Device-local memory such as VRAM or accelerator-managed heaps.
        const DEVICE_LOCAL          = 1 << 1;
        /// Physically addressed memory regions.
        const PHYSICAL              = 1 << 2;
        /// Fixed static or externally provided regions.
        const STATIC_REGION         = 1 << 3;
        /// MMIO-like regions with device side effects.
        const MMIO                  = 1 << 4;
    }
}

/// Coarse domain classification for a memory resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryDomain {
    /// Ordinary virtual-address-space backed memory.
    VirtualAddressSpace,
    /// Device-local memory exposed as a governed resource.
    DeviceLocal,
    /// Physical or physically-addressed memory.
    Physical,
    /// Static or externally provided region-backed memory.
    StaticRegion,
    /// Memory-mapped I/O or similarly hazardous device regions.
    Mmio,
}

/// Concrete backing shape represented by a memory resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceBackingKind {
    /// Private anonymous virtual memory.
    AnonymousPrivate,
    /// Shared anonymous virtual memory.
    AnonymousShared,
    /// Privately mapped file-backed memory.
    FilePrivate,
    /// Shared file-backed memory.
    FileShared,
    /// Borrowed region supplied by the surrounding environment.
    Borrowed,
    /// Fixed static region with platform-defined lifetime.
    StaticRegion,
    /// RTOS- or firmware-managed memory partition.
    Partition,
    /// Device-local backing such as GPU or accelerator heaps.
    DeviceLocal,
    /// Physical backing selected by address or physical descriptor.
    Physical,
    /// MMIO-like mapping with device side effects.
    Mmio,
}
