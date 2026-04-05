//! fusion-sys atomic substrate wrappers built on the selected fusion-pal backend.
//!
//! This layer stays intentionally thin for now. The contract truth already lives in
//! `fusion-pal`; `fusion-sys` just re-exports the selected atomic provider and word surface
//! without pretending it invented them.

pub use fusion_pal::sys::atomic::{
    AtomicBaseContract,
    AtomicCompareExchangeOutcome32,
    AtomicError,
    AtomicErrorKind,
    AtomicFallbackKind,
    AtomicImplementationKind,
    AtomicScopeSupport,
    AtomicSupport,
    AtomicTimeoutCaps,
    AtomicWaitOutcome,
    AtomicWaitWord32Caps,
    AtomicWaitWord32Support,
    AtomicWord32Contract,
    AtomicWord32Caps,
    AtomicWord32Support,
    PLATFORM_ATOMIC_WAIT_WORD32_IMPLEMENTATION,
    PLATFORM_ATOMIC_WORD32_IMPLEMENTATION,
    PlatformAtomic,
    PlatformAtomicWord32,
    system_atomic,
};
