//! Capability vocabulary for protocol contracts.

use bitflags::bitflags;

/// Shared implementation-category vocabulary specialized for protocol support.
pub use crate::contract::pal::caps::ImplementationKind as ProtocolImplementationKind;

bitflags! {
    /// Protocol features honestly surfaced by a protocol contract.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ProtocolCaps: u32 {
        /// The protocol defines a stable version surface.
        const VERSIONED          = 1 << 0;
        /// The protocol requires or supports an explicit negotiation/bootstrap step.
        const NEGOTIATED_BOOTSTRAP = 1 << 1;
        /// The protocol exposes a structured debug/introspection view.
        const DEBUG_VIEW         = 1 << 2;
        /// The protocol exposes a plaintext adapter/debug view.
        const PLAINTEXT_VIEW     = 1 << 3;
    }
}
