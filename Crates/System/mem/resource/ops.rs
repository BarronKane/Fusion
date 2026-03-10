bitflags::bitflags! {
    /// Operations a concrete memory resource instance can legally expose.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ResourceOpSet: u32 {
        /// Protection changes are supported.
        const PROTECT  = 1 << 0;
        /// Advisory hints are supported.
        const ADVISE   = 1 << 1;
        /// Residency lock/unlock is supported.
        const LOCK     = 1 << 2;
        /// Region query is supported.
        const QUERY    = 1 << 3;
        /// Commit of reserved backing is supported.
        const COMMIT   = 1 << 4;
        /// Decommit while preserving reservation is supported.
        const DECOMMIT = 1 << 5;
        /// Semantic discard/reset of contents is supported.
        const DISCARD  = 1 << 6;
        /// Explicit persistence or cache flush is supported.
        const FLUSH    = 1 << 7;
    }
}

bitflags::bitflags! {
    /// Soft preferences that acquisition may try to honor but may legally miss.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ResourcePreferenceSet: u32 {
        /// Prefer a non-default placement strategy.
        const PLACEMENT  = 1 << 0;
        /// Prefer eager population or prefaulting.
        const PREFAULT   = 1 << 1;
        /// Prefer initial residency locking.
        const LOCK       = 1 << 2;
        /// Prefer huge-page treatment when available.
        const HUGE_PAGES = 1 << 3;
    }
}

bitflags::bitflags! {
    /// Inherent hazards that remain true even when the resource is used correctly.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ResourceHazardSet: u32 {
        /// Executable code may exist or be permitted within the resource contract.
        const EXECUTABLE                = 1 << 0;
        /// Shared aliasing or externally visible writes can occur.
        const SHARED_ALIASING           = 1 << 1;
        /// Some semantics are emulated rather than enforced by the operating system.
        const EMULATED                  = 1 << 2;
        /// Overcommit or lazy commitment may cause later allocation failure.
        const OVERCOMMIT                = 1 << 3;
        /// The range is not fully coherent with all relevant agents.
        const NON_COHERENT              = 1 << 4;
        /// State may change outside the control of this resource handle.
        const EXTERNAL_MUTATION         = 1 << 5;
        /// Access may trigger device-visible side effects.
        const MMIO_SIDE_EFFECTS         = 1 << 6;
        /// Data persistence requires explicit flush-like operations.
        const PERSISTENCE_REQUIRES_FLUSH = 1 << 7;
        /// Alias for shared aliasing when a shorter name is convenient.
        const SHARED                    = Self::SHARED_ALIASING.bits();
    }
}
