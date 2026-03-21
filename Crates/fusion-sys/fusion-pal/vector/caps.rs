//! Capability vocabulary for interrupt-vector ownership backends.

use bitflags::bitflags;

/// Shared implementation-category vocabulary specialized for vector ownership support.
pub use crate::pal::caps::ImplementationKind as VectorImplementationKind;

bitflags! {
    /// Interrupt-vector features the backend can honestly surface.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct VectorCaps: u32 {
        /// The backend can adopt the current hardware vector table into owned RAM.
        const ADOPT_AND_CLONE     = 1 << 0;
        /// The backend can install or manage an overlay-compatible registration surface.
        const OVERLAY             = 1 << 1;
        /// The backend can maintain independent vector tables per core.
        const PER_CORE_TABLES     = 1 << 2;
        /// The backend can manage one secure-world vector table.
        const SECURE_WORLD        = 1 << 3;
        /// The backend can manage one non-secure-world vector table.
        const NON_SECURE_WORLD    = 1 << 4;
        /// The backend can apply raw per-slot hardware interrupt priorities.
        const PRIORITY_CONTROL    = 1 << 5;
        /// The backend can query and mutate per-slot pending state.
        const PENDING_CONTROL     = 1 << 6;
        /// The backend can freeze configuration through a sealing step.
        const SEAL                = 1 << 7;
        /// One slot may dispatch inline in ISR context.
        const INLINE_DISPATCH     = 1 << 8;
        /// One deferred dispatch lane is available.
        const DEFERRED_PRIMARY    = 1 << 9;
        /// A second deferred dispatch lane is available.
        const DEFERRED_SECONDARY  = 1 << 10;
    }
}

/// Full capability surface for one vector-ownership backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VectorSupport {
    /// Backend-supported vector features.
    pub caps: VectorCaps,
    /// Native, lowered-with-restrictions, or unsupported implementation category.
    pub implementation: VectorImplementationKind,
    /// Number of peripheral IRQ slots surfaced by this backend.
    pub slot_count: u16,
    /// Number of implemented raw interrupt-priority bits.
    pub implemented_priority_bits: u8,
}

impl VectorSupport {
    /// Returns a fully unsupported vector-ownership surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: VectorCaps::empty(),
            implementation: VectorImplementationKind::Unsupported,
            slot_count: 0,
            implemented_priority_bits: 0,
        }
    }
}
