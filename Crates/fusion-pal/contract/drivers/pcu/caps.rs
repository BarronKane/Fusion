//! Capability vocabulary for generic PCU backends.

use bitflags::bitflags;

/// Shared implementation-category vocabulary specialized for PCU support.
pub use crate::contract::pal::caps::ImplementationKind as PcuImplementationKind;

bitflags! {
    /// Generic PCU features the backend can honestly surface.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PcuCaps: u32 {
        /// The backend can enumerate surfaced PCU executors.
        const ENUMERATE_EXECUTORS = 1 << 0;
        /// Executors can be claimed explicitly.
        const CLAIM_EXECUTOR      = 1 << 1;
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
        /// Back-compat alias while the tree stops saying “device” when it means “executor.”
        const ENUMERATE           = Self::ENUMERATE_EXECUTORS.bits();
        /// Back-compat alias while the tree stops saying “device” when it means “executor.”
        const CLAIM_DEVICE        = Self::CLAIM_EXECUTOR.bits();
    }
}

/// Full capability surface for one generic PCU backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuSupport {
    /// Backend-supported generic PCU features.
    pub caps: PcuCaps,
    /// Native, lowered-with-restrictions, or unsupported implementation category.
    pub implementation: PcuImplementationKind,
    /// Number of surfaced PCU executors.
    pub executor_count: u8,
}

impl PcuSupport {
    /// Returns a fully unsupported generic PCU surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: PcuCaps::empty(),
            implementation: PcuImplementationKind::Unsupported,
            executor_count: 0,
        }
    }
}
