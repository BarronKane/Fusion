//! Capability vocabulary for generic coprocessor backends.

use bitflags::bitflags;

/// Shared implementation-category vocabulary specialized for PCU support.
pub use crate::contract::caps::ImplementationKind as PcuImplementationKind;

bitflags! {
    /// Generic coprocessor features the backend can honestly surface.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PcuCaps: u32 {
        /// The backend can enumerate surfaced coprocessor devices.
        const ENUMERATE           = 1 << 0;
        /// Devices can be claimed explicitly.
        const CLAIM_DEVICE        = 1 << 1;
        /// The backend can accept dispatched kernels or jobs.
        const DISPATCH            = 1 << 2;
        /// Completion can be polled or queried asynchronously.
        const COMPLETION_STATUS   = 1 << 3;
        /// The backend can bind caller-owned memory resources directly.
        const EXTERNAL_RESOURCES  = 1 << 4;
        /// The backend supports compute-style invocation shapes.
        const COMPUTE_DISPATCH    = 1 << 5;
        /// The backend can expose or negotiate device-local execution memory.
        const DEVICE_LOCAL_MEMORY = 1 << 6;
    }
}

/// Full capability surface for one generic coprocessor backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuSupport {
    /// Backend-supported generic coprocessor features.
    pub caps: PcuCaps,
    /// Native, lowered-with-restrictions, or unsupported implementation category.
    pub implementation: PcuImplementationKind,
    /// Number of surfaced coprocessor devices.
    pub device_count: u8,
}

impl PcuSupport {
    /// Returns a fully unsupported generic coprocessor surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: PcuCaps::empty(),
            implementation: PcuImplementationKind::Unsupported,
            device_count: 0,
        }
    }
}
