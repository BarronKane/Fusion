//! Shared Cortex-M programmable-IO identifiers and descriptor vocabulary.

use super::caps::PioCaps;
use super::error::PioError;

bitflags::bitflags! {
    /// Coarse pin-mapping features supported by one lane.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PioPinMappingCaps: u32 {
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
pub struct PioEngineId(pub u8);

/// Opaque program identifier supplied by higher layers when loading one native image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PioProgramId(pub u32);

/// One lane or state-machine identifier within one engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PioLaneId {
    /// Owning engine block.
    pub engine: PioEngineId,
    /// Zero-based lane index inside the engine.
    pub index: u8,
}

/// One programmable-IO lane mask within one engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PioLaneMask(u8);

impl PioLaneMask {
    /// Empty lane mask.
    pub const EMPTY: Self = Self(0);

    /// Creates one checked lane mask.
    ///
    /// # Errors
    ///
    /// Returns an error when the mask is empty.
    pub const fn new(bits: u8) -> Result<Self, PioError> {
        if bits == 0 {
            Err(PioError::invalid())
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
pub enum PioFifoDirection {
    /// Transmit-side FIFO.
    Tx,
    /// Receive-side FIFO.
    Rx,
}

/// FIFO identifier for one lane-local endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PioFifoId {
    /// Owning lane.
    pub lane: PioLaneId,
    /// FIFO direction.
    pub direction: PioFifoDirection,
}

/// Instruction-memory descriptor for one engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PioInstructionMemoryDescriptor {
    /// Number of instruction words surfaced by this engine.
    pub word_count: u16,
    /// Width of one instruction word in bits.
    pub word_bits: u8,
    /// Whether the instruction store is shared by all lanes in the engine.
    pub shared_across_lanes: bool,
}

/// FIFO descriptor for one lane-local endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PioFifoDescriptor {
    /// Stable FIFO identifier.
    pub id: PioFifoId,
    /// FIFO depth in words.
    pub depth_words: u8,
    /// Width of each FIFO word in bits.
    pub word_bits: u8,
}

/// Clocking descriptor for one programmable-IO engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PioClockDescriptor {
    /// Whether the engine is clocked from the main system fabric clock.
    pub uses_system_clock: bool,
    /// Whether the engine supports fractional clock dividers.
    pub fractional_divider: bool,
}

/// Static descriptor for one programmable-IO lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PioLaneDescriptor {
    /// Stable lane identifier.
    pub id: PioLaneId,
    /// Human-readable lane name.
    pub name: &'static str,
    /// Lane-local TX FIFO.
    pub tx_fifo: PioFifoDescriptor,
    /// Lane-local RX FIFO.
    pub rx_fifo: PioFifoDescriptor,
    /// Coarse pin-mapping capabilities for this lane.
    pub pin_mapping: PioPinMappingCaps,
}

/// Static descriptor for one programmable-IO engine block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PioEngineDescriptor {
    /// Stable engine identifier.
    pub id: PioEngineId,
    /// Human-readable engine name.
    pub name: &'static str,
    /// Number of surfaced lanes or state machines.
    pub lane_count: u8,
    /// Instruction-memory description for this engine.
    pub instruction_memory: PioInstructionMemoryDescriptor,
    /// Clocking description for this engine.
    pub clocking: PioClockDescriptor,
    /// Engine-local capability refinement.
    pub caps: PioCaps,
    /// Engine-visible IRQ lines, if any.
    pub irq_lines: &'static [u16],
    /// Base TX DMA request selector, if the engine exposes one per lane.
    pub tx_dreq_base: Option<u16>,
    /// Base RX DMA request selector, if the engine exposes one per lane.
    pub rx_dreq_base: Option<u16>,
}

/// Opaque native program image ready to load into one engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PioProgramImage<'a> {
    /// Stable program identifier chosen by the caller.
    pub id: PioProgramId,
    /// Native instruction words in backend-defined encoding.
    pub words: &'a [u16],
}

/// Exclusive engine claim returned by one backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PioEngineClaim {
    pub(crate) engine: PioEngineId,
}

impl PioEngineClaim {
    /// Returns the claimed engine identifier.
    #[must_use]
    pub const fn engine(self) -> PioEngineId {
        self.engine
    }
}

/// Lane claim returned by one backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PioLaneClaim {
    pub(crate) engine: PioEngineId,
    pub(crate) lanes: PioLaneMask,
}

impl PioLaneClaim {
    /// Returns the claimed engine identifier.
    #[must_use]
    pub const fn engine(self) -> PioEngineId {
        self.engine
    }

    /// Returns the claimed lane bitmask.
    #[must_use]
    pub const fn lanes(self) -> PioLaneMask {
        self.lanes
    }

    /// Returns whether this claim contains one specific lane.
    #[must_use]
    pub const fn contains_lane(self, lane: PioLaneId) -> bool {
        self.engine.0 == lane.engine.0 && self.lanes.contains_lane(lane.index)
    }
}

/// Loaded program lease returned by one backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PioProgramLease {
    pub(crate) engine: PioEngineId,
    pub(crate) program: PioProgramId,
    pub(crate) word_count: u16,
}

impl PioProgramLease {
    /// Returns the engine containing this loaded program.
    #[must_use]
    pub const fn engine(self) -> PioEngineId {
        self.engine
    }

    /// Returns the caller-supplied program identifier.
    #[must_use]
    pub const fn program(self) -> PioProgramId {
        self.program
    }

    /// Returns the number of loaded instruction words.
    #[must_use]
    pub const fn word_count(self) -> u16 {
        self.word_count
    }
}
