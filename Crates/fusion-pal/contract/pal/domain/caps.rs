//! Capability vocabulary for native domain/courier/context contracts.

use bitflags::bitflags;

/// Shared implementation-category vocabulary specialized for native domain support.
pub use crate::contract::pal::caps::ImplementationKind as DomainImplementationKind;
/// Shared implementation-category vocabulary specialized for courier support.
pub use crate::contract::pal::caps::ImplementationKind as CourierImplementationKind;
/// Shared implementation-category vocabulary specialized for native context support.
pub use crate::contract::pal::caps::ImplementationKind as ContextImplementationKind;

bitflags! {
    /// Features the selected domain can honestly surface.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct DomainCaps: u32 {
        /// The domain can enumerate couriers directly.
        const COURIER_REGISTRY            = 1 << 0;
        /// The domain can enumerate contexts directly.
        const CONTEXT_REGISTRY            = 1 << 1;
        /// The domain can host couriers with distinct visibility envelopes.
        const COURIER_VISIBILITY          = 1 << 2;
        /// The domain can project or attach remote couriers.
        const REMOTE_COURIERS             = 1 << 3;
        /// The domain can surface optional debug endpoints under debug-profile policy.
        const DEBUG_CHANNELS              = 1 << 4;
    }
}

bitflags! {
    /// Features one courier can honestly surface.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct CourierCaps: u32 {
        /// The courier can enumerate only its visible contexts.
        const ENUMERATE_VISIBLE_CONTEXTS  = 1 << 0;
        /// The courier can project contexts into another courier.
        const PROJECT_CONTEXTS            = 1 << 1;
        /// The courier can own or spawn sub-fibers.
        const SPAWN_SUB_FIBERS            = 1 << 2;
        /// The courier can surface optional debug endpoints.
        const DEBUG_CHANNEL               = 1 << 3;
    }
}

bitflags! {
    /// Features one visible context can honestly surface.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ContextCaps: u32 {
        /// The context can be projected into another courier.
        const PROJECTABLE                 = 1 << 0;
        /// The context is backed by a channel transport.
        const CHANNEL_BACKED              = 1 << 1;
        /// The context can surface an optional debug endpoint.
        const DEBUG_ENDPOINT              = 1 << 2;
        /// The context exposes a control/metadata plane.
        const CONTROL_ENDPOINT            = 1 << 3;
    }
}
