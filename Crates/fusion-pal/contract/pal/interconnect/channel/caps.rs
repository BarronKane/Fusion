//! Capability vocabulary for channel contracts.

use bitflags::bitflags;

/// Shared implementation-category vocabulary specialized for channel support.
pub use crate::contract::pal::caps::ImplementationKind as ChannelImplementationKind;

bitflags! {
    /// Channel features honestly surfaced by a channel implementation.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ChannelCaps: u32 {
        /// The channel surfaces one write-side endpoint for typed messages.
        const WRITE                        = 1 << 0;
        /// The channel surfaces one read-side endpoint for typed messages.
        const READ                         = 1 << 1;
        /// The channel buffers messages in a bounded queue.
        const BUFFERED                     = 1 << 2;
        /// The channel can promote from SPSC to SPMC.
        const MODE_PROMOTION               = 1 << 3;
        /// Cross-courier access may be claim-gated.
        const CLAIM_GATED_CROSS_COURIER   = 1 << 4;
        /// Cross-domain access may be claim-gated.
        const CLAIM_GATED_CROSS_DOMAIN    = 1 << 5;
        /// Back-compat alias while the tree stops saying “send” when it means one write side.
        const SEND                         = Self::WRITE.bits();
        /// Back-compat alias while the tree stops saying “receive” when it means one read side.
        const RECEIVE                      = Self::READ.bits();
    }
}
