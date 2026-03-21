//! Shared programmable-IO identifiers and descriptor vocabulary.

use super::caps::PcuCaps;
use super::error::PcuError;

bitflags::bitflags! {
    /// Coarse pin-mapping features supported by one lane.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PcuPinMappingCaps: u32 {
        /// Input pins can be remapped.
        const INPUT_BASE   = 1 << 0;
        /// Output pins can be remapped.
        const OUTPUT_BASE  = 1 << 1;
        /// SET pins can be remapped.
        const SET_BASE     = 1 << 2;
        /// Side-set pins can be remapped.
        const SIDESET_BASE = 1 << 3;
        /// Jump-pin selection can be remapped.
        const JMP_PIN      = 1 << 4;
    }
}

/// Opaque engine identifier surfaced by a programmable-IO backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuEngineId(pub u8);

/// Opaque program identifier supplied by higher layers when loading one native image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuProgramId(pub u32);

/// One lane or state-machine identifier within one engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuLaneId {
    /// Owning engine block.
    pub engine: PcuEngineId,
    /// Zero-based lane index inside the engine.
    pub index: u8,
}

/// One programmable-IO lane mask within one engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuLaneMask(u8);

impl PcuLaneMask {
    /// Empty lane mask.
    pub const EMPTY: Self = Self(0);

    /// Creates one checked lane mask.
    ///
    /// # Errors
    ///
    /// Returns an error when the mask is empty.
    pub const fn new(bits: u8) -> Result<Self, PcuError> {
        if bits == 0 {
            Err(PcuError::invalid())
        } else {
            Ok(Self(bits))
        }
    }

    /// Returns a mask containing one lane.
    #[must_use]
    pub const fn from_lane(index: u8) -> Self {
        Self(1u8 << index)
    }

    /// Returns the raw bitmask.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Returns whether the mask contains one lane index.
    #[must_use]
    pub const fn contains_lane(self, index: u8) -> bool {
        (self.0 & (1u8 << index)) != 0
    }
}

/// FIFO direction for one lane-local endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuFifoDirection {
    /// Transmit-side FIFO.
    Tx,
    /// Receive-side FIFO.
    Rx,
}

/// FIFO identifier for one lane-local endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuFifoId {
    /// Owning lane.
    pub lane: PcuLaneId,
    /// FIFO direction.
    pub direction: PcuFifoDirection,
}

/// Instruction-memory descriptor for one engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuInstructionMemoryDescriptor {
    /// Number of instruction words surfaced by this engine.
    pub word_count: u16,
    /// Width of one instruction word in bits.
    pub word_bits: u8,
    /// Whether the instruction store is shared by all lanes in the engine.
    pub shared_across_lanes: bool,
}

/// FIFO descriptor for one lane-local endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuFifoDescriptor {
    /// Stable FIFO identifier.
    pub id: PcuFifoId,
    /// FIFO depth in words.
    pub depth_words: u8,
    /// Width of each FIFO word in bits.
    pub word_bits: u8,
}

/// Clocking descriptor for one programmable-IO engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuClockDescriptor {
    /// Whether the engine is clocked from the main system fabric clock.
    pub uses_system_clock: bool,
    /// Whether the engine supports fractional clock dividers.
    pub fractional_divider: bool,
}

/// Static descriptor for one programmable-IO lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuLaneDescriptor {
    /// Stable lane identifier.
    pub id: PcuLaneId,
    /// Human-readable lane name.
    pub name: &'static str,
    /// Lane-local TX FIFO.
    pub tx_fifo: PcuFifoDescriptor,
    /// Lane-local RX FIFO.
    pub rx_fifo: PcuFifoDescriptor,
    /// Coarse pin-mapping capabilities for this lane.
    pub pin_mapping: PcuPinMappingCaps,
}

/// Static descriptor for one programmable-IO engine block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuEngineDescriptor {
    /// Stable engine identifier.
    pub id: PcuEngineId,
    /// Human-readable engine name.
    pub name: &'static str,
    /// Number of surfaced lanes or state machines.
    pub lane_count: u8,
    /// Instruction-memory description for this engine.
    pub instruction_memory: PcuInstructionMemoryDescriptor,
    /// Clocking description for this engine.
    pub clocking: PcuClockDescriptor,
    /// Engine-local capability refinement.
    pub caps: PcuCaps,
    /// Engine-visible IRQ lines, if any.
    pub irq_lines: &'static [u16],
    /// Base TX DMA request selector, if the engine exposes one per lane.
    pub tx_dreq_base: Option<u16>,
    /// Base RX DMA request selector, if the engine exposes one per lane.
    pub rx_dreq_base: Option<u16>,
}

/// Opaque native program image ready to load into one engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuProgramImage<'a> {
    /// Stable program identifier chosen by the caller.
    pub id: PcuProgramId,
    /// Native instruction words in backend-defined encoding.
    pub words: &'a [u16],
}

/// Exclusive engine claim returned by one backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuEngineClaim {
    pub(crate) engine: PcuEngineId,
}

impl PcuEngineClaim {
    /// Returns the claimed engine identifier.
    #[must_use]
    pub const fn engine(self) -> PcuEngineId {
        self.engine
    }
}

/// Lane claim returned by one backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuLaneClaim {
    pub(crate) engine: PcuEngineId,
    pub(crate) lanes: PcuLaneMask,
}

impl PcuLaneClaim {
    /// Returns the claimed engine identifier.
    #[must_use]
    pub const fn engine(self) -> PcuEngineId {
        self.engine
    }

    /// Returns the claimed lane bitmask.
    #[must_use]
    pub const fn lanes(self) -> PcuLaneMask {
        self.lanes
    }

    /// Returns whether this claim contains one specific lane.
    #[must_use]
    pub const fn contains_lane(self, lane: PcuLaneId) -> bool {
        self.engine.0 == lane.engine.0 && self.lanes.contains_lane(lane.index)
    }
}

/// Loaded program lease returned by one backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuProgramLease {
    pub(crate) engine: PcuEngineId,
    pub(crate) program: PcuProgramId,
    pub(crate) word_count: u16,
}

impl PcuProgramLease {
    /// Returns the engine containing this loaded program.
    #[must_use]
    pub const fn engine(self) -> PcuEngineId {
        self.engine
    }

    /// Returns the caller-supplied program identifier.
    #[must_use]
    pub const fn program(self) -> PcuProgramId {
        self.program
    }

    /// Returns the number of loaded instruction words.
    #[must_use]
    pub const fn word_count(self) -> u16 {
        self.word_count
    }
}
