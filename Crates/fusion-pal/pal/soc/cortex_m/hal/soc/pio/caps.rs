//! Capability vocabulary for Cortex-M programmable-IO backends.

use bitflags::bitflags;

/// Shared implementation-category vocabulary specialized for PCU support.
pub use crate::contract::caps::ImplementationKind as PcuImplementationKind;

bitflags! {
    /// Programmable-IO features the backend can honestly surface.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PcuCaps: u32 {
        /// The backend can enumerate programmable-IO engines and lanes.
        const ENUMERATE                    = 1 << 0;
        /// Engines can be claimed explicitly.
        const CLAIM_ENGINE                 = 1 << 1;
        /// Lanes or state machines can be claimed explicitly.
        const CLAIM_LANES                  = 1 << 2;
        /// Native program images can be loaded into engine instruction memory.
        const LOAD_PROGRAM                 = 1 << 3;
        /// Claimed lanes can be started, stopped, or restarted.
        const CONTROL                      = 1 << 4;
        /// TX/RX FIFOs can be accessed directly.
        const FIFO_IO                      = 1 << 5;
        /// One instruction image is shared across multiple lanes.
        const SHARED_INSTRUCTION_MEMORY    = 1 << 6;
        /// Each lane has its own program counter and execution state.
        const PER_LANE_PROGRAM_COUNTER     = 1 << 7;
        /// Side-set or equivalent auxiliary pin driving is supported.
        const LANE_SIDESET                 = 1 << 8;
        /// The engine can wait directly on pin state.
        const WAIT_ON_PIN                  = 1 << 9;
        /// The engine can signal or wait on internal events or IRQ flags.
        const IRQ_SIGNAL                   = 1 << 10;
        /// Shift engines can move data in both directions.
        const BIDIRECTIONAL_SHIFT          = 1 << 11;
        /// Automatic pull from TX-side shift state is supported.
        const AUTOPULL                     = 1 << 12;
        /// Automatic push into RX-side shift state is supported.
        const AUTOPUSH                     = 1 << 13;
        /// DMA pacing or FIFO attachment is supported.
        const DMA_FEED                     = 1 << 14;
        /// Program replacement requires stopping participating lanes first.
        const PROGRAM_SWAP_REQUIRES_STOP   = 1 << 15;
        /// Program replacement can occur atomically without stopping active lanes.
        const ATOMIC_PROGRAM_SWAP          = 1 << 16;
        /// Multiple lanes can be started cooperatively as one group.
        const MULTI_LANE_COOPERATIVE_START = 1 << 17;
        /// Pin mapping is flexible rather than hardwired.
        const PIN_MAPPING_FLEXIBLE         = 1 << 18;
    }
}

/// Full capability surface for one programmable-IO backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuSupport {
    /// Backend-supported programmable-IO features.
    pub caps: PcuCaps,
    /// Native, lowered-with-restrictions, or unsupported implementation category.
    pub implementation: PcuImplementationKind,
    /// Number of surfaced engine blocks.
    pub engine_count: u8,
}

impl PcuSupport {
    /// Returns a fully unsupported programmable-IO surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: PcuCaps::empty(),
            implementation: PcuImplementationKind::Unsupported,
            engine_count: 0,
        }
    }
}
