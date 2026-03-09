bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ResourceOpSet: u32 {
        const PROTECT  = 1 << 0;
        const ADVISE   = 1 << 1;
        const LOCK     = 1 << 2;
        const QUERY    = 1 << 3;
        const COMMIT   = 1 << 4;
        const DECOMMIT = 1 << 5;
        const DISCARD  = 1 << 6;
        const FLUSH    = 1 << 7;
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ResourcePreferenceSet: u32 {
        const PLACEMENT  = 1 << 0;
        const PREFAULT   = 1 << 1;
        const LOCK       = 1 << 2;
        const HUGE_PAGES = 1 << 3;
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ResourceHazardSet: u32 {
        const EXECUTABLE                = 1 << 0;
        const SHARED_ALIASING           = 1 << 1;
        const EMULATED                  = 1 << 2;
        const OVERCOMMIT                = 1 << 3;
        const NON_COHERENT              = 1 << 4;
        const EXTERNAL_MUTATION         = 1 << 5;
        const MMIO_SIDE_EFFECTS         = 1 << 6;
        const PERSISTENCE_REQUIRES_FLUSH = 1 << 7;
        const SHARED                    = Self::SHARED_ALIASING.bits();
    }
}
