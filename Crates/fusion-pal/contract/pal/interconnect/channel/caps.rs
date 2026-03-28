//! Capability vocabulary for channel contracts.

use bitflags::bitflags;

/// Shared implementation-category vocabulary specialized for channel support.
pub use crate::contract::pal::caps::ImplementationKind as ChannelImplementationKind;

bitflags! {
    /// Channel features honestly surfaced by a channel implementation.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ChannelCaps: u32 {
        /// The channel can send typed messages.
        const SEND                         = 1 << 0;
        /// The channel can receive typed messages.
        const RECEIVE                      = 1 << 1;
        /// The channel buffers messages in a bounded queue.
        const BUFFERED                     = 1 << 2;
        /// The channel can promote from SPSC to SPMC.
        const MODE_PROMOTION               = 1 << 3;
        /// Cross-courier access may be claim-gated.
        const CLAIM_GATED_CROSS_COURIER   = 1 << 4;
        /// Cross-domain access may be claim-gated.
        const CLAIM_GATED_CROSS_DOMAIN    = 1 << 5;
    }
}
