bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ResourceAttrs: u32 {
        const ALLOCATABLE        = 1 << 0;
        const READ_ONLY_BACKING  = 1 << 1;
        const DMA_VISIBLE        = 1 << 2;
        const DEVICE_LOCAL       = 1 << 3;
        const CACHEABLE          = 1 << 4;
        const COHERENT           = 1 << 5;
        const PHYS_CONTIGUOUS    = 1 << 6;
        const TAGGED             = 1 << 7;
        const INTEGRITY_MANAGED  = 1 << 8;
        const STATIC_REGION      = 1 << 9;
        const HAZARDOUS_IO       = 1 << 10;
        const PERSISTENT         = 1 << 11;
    }
}
