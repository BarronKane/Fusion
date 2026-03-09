bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MemoryDomainSet: u32 {
        const VIRTUAL_ADDRESS_SPACE = 1 << 0;
        const DEVICE_LOCAL          = 1 << 1;
        const PHYSICAL              = 1 << 2;
        const STATIC_REGION         = 1 << 3;
        const MMIO                  = 1 << 4;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryDomain {
    VirtualAddressSpace,
    DeviceLocal,
    Physical,
    StaticRegion,
    Mmio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceBackingKind {
    AnonymousPrivate,
    AnonymousShared,
    FilePrivate,
    FileShared,
    Borrowed,
    StaticRegion,
    Partition,
    DeviceLocal,
    Physical,
    Mmio,
}
