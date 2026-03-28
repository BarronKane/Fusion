//! Capability vocabulary for universal transport-layer contracts.

use bitflags::bitflags;

/// Shared implementation-category vocabulary specialized for transport support.
pub use crate::contract::pal::caps::ImplementationKind as TransportImplementationKind;

bitflags! {
    /// Features the transport can honestly surface.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct TransportCaps: u32 {
        /// Producers can attach dynamically.
        const ATTACH_PRODUCER             = 1 << 0;
        /// Consumers can attach dynamically.
        const ATTACH_CONSUMER             = 1 << 1;
        /// Producers can detach dynamically.
        const DETACH_PRODUCER             = 1 << 2;
        /// Consumers can detach dynamically.
        const DETACH_CONSUMER             = 1 << 3;
        /// The transport can promote from one topology to another at runtime.
        const TOPOLOGY_PROMOTION          = 1 << 4;
        /// The transport can attach across courier boundaries.
        const CROSS_COURIER_ATTACH        = 1 << 5;
        /// The transport can attach across domain boundaries.
        const CROSS_DOMAIN_ATTACH         = 1 << 6;
        /// The transport has bounded buffering.
        const BUFFERED                    = 1 << 7;
        /// The transport can expose wake/readiness progress.
        const WAKE_SIGNAL                 = 1 << 8;
    }
}
