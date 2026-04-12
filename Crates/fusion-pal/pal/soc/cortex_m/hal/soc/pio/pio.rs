//! Cortex-M SoC-layer programmable-IO vocabulary and backend contract.

mod error {
    pub use crate::contract::drivers::pcu::PcuError as PioError;
}

mod caps;
mod ir;
mod kernels;
mod lowering;
mod types;
mod unsupported;

pub use crate::contract::drivers::pcu::{
    PcuError as PioError,
    PcuErrorKind as PioErrorKind,
};
#[doc(hidden)]
pub use crate::contract::drivers::pcu::PcuError;
#[doc(hidden)]
pub use caps::{
    PioCaps,
    PioImplementationKind,
    PioSupport,
};
pub use ir::*;
pub use kernels::*;
pub use lowering::*;
#[doc(hidden)]
pub use types::{
    PioClockDescriptor as PcuClockDescriptor,
    PioEngineClaim as PcuEngineClaim,
    PioEngineDescriptor as PcuEngineDescriptor,
    PioEngineId as PcuEngineId,
    PioFifoDescriptor as PcuFifoDescriptor,
    PioFifoDirection as PcuFifoDirection,
    PioFifoId as PcuFifoId,
    PioInstructionMemoryDescriptor as PcuInstructionMemoryDescriptor,
    PioLaneClaim as PcuLaneClaim,
    PioLaneDescriptor as PcuLaneDescriptor,
    PioLaneId as PcuLaneId,
    PioLaneMask as PcuLaneMask,
    PioPinMappingCaps as PcuPinMappingCaps,
    PioProgramId as PcuProgramId,
    PioProgramImage as PcuProgramImage,
    PioProgramLease as PcuProgramLease,
};
#[doc(hidden)]
pub use ir::{
    PioIrClockConfig as PcuIrClockConfig,
    PioIrExecutionConfig as PcuIrExecutionConfig,
    PioIrInSource as PcuIrInSource,
    PioIrInstruction as PcuIrInstruction,
    PioIrInstructionTiming as PcuIrInstructionTiming,
    PioIrIrqAction as PcuIrIrqAction,
    PioIrJumpCondition as PcuIrJumpCondition,
    PioIrMovDestination as PcuIrMovDestination,
    PioIrMovOperation as PcuIrMovOperation,
    PioIrMovSource as PcuIrMovSource,
    PioIrOutDestination as PcuIrOutDestination,
    PioIrPinConfig as PcuIrPinConfig,
    PioIrProgram as PcuIrProgram,
    PioIrSetDestination as PcuIrSetDestination,
    PioIrShiftConfig as PcuIrShiftConfig,
    PioIrShiftDirection as PcuIrShiftDirection,
    PioIrWaitCondition as PcuIrWaitCondition,
};
pub use types::{
    PioClockDescriptor,
    PioEngineClaim,
    PioEngineDescriptor,
    PioEngineId,
    PioFifoDescriptor,
    PioFifoDirection,
    PioFifoId,
    PioInstructionMemoryDescriptor,
    PioLaneClaim,
    PioLaneDescriptor,
    PioLaneId,
    PioLaneMask,
    PioPinMappingCaps,
    PioProgramId,
    PioProgramImage,
    PioProgramLease,
};
#[doc(hidden)]
pub use unsupported::UnsupportedPio;

/// Capability trait for Cortex-M programmable-IO backends.
#[doc(hidden)]
pub trait PioBaseContract {
    /// Reports the truthful programmable-IO surface for this backend.
    fn support(&self) -> PioSupport;

    /// Returns the engine descriptors surfaced by this backend.
    #[must_use]
    fn engines(&self) -> &'static [PioEngineDescriptor];

    /// Returns the lane descriptors for one engine, or an empty slice when the engine is
    /// unknown.
    #[must_use]
    fn lanes(&self, engine: PioEngineId) -> &'static [PioLaneDescriptor];
}

/// Control contract for Cortex-M programmable-IO backends.
#[doc(hidden)]
pub trait PioControlContract: PioBaseContract {
    /// Claims one engine exclusively.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the engine is unknown, unsupported, or already claimed.
    fn claim_engine(&self, engine: PioEngineId) -> Result<PioEngineClaim, PioError>;

    /// Releases one previously claimed engine.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the claim no longer matches backend state.
    fn release_engine(&self, claim: PioEngineClaim) -> Result<(), PioError>;

    /// Claims one or more lanes within one engine.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the engine is unknown, the mask is invalid, or any lane is
    /// already claimed.
    fn claim_lanes(
        &self,
        engine: PioEngineId,
        lanes: PioLaneMask,
    ) -> Result<PioLaneClaim, PioError>;

    /// Releases one previously claimed lane mask.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the claim no longer matches backend state.
    fn release_lanes(&self, claim: PioLaneClaim) -> Result<(), PioError>;

    /// Loads one native program image into one claimed engine.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the image does not fit or the engine cannot accept it.
    fn load_program(
        &self,
        claim: &PioEngineClaim,
        image: &PioProgramImage<'_>,
    ) -> Result<PioProgramLease, PioError>;

    /// Unloads one previously loaded native program image from one claimed engine.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the lease no longer matches backend state.
    fn unload_program(
        &self,
        claim: &PioEngineClaim,
        lease: PioProgramLease,
    ) -> Result<(), PioError>;

    /// Starts one claimed lane set.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the backend cannot start the requested lanes.
    fn start_lanes(&self, claim: &PioLaneClaim) -> Result<(), PioError>;

    /// Stops one claimed lane set.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the backend cannot stop the requested lanes.
    fn stop_lanes(&self, claim: &PioLaneClaim) -> Result<(), PioError>;

    /// Restarts one claimed lane set.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the backend cannot restart the requested lanes.
    fn restart_lanes(&self, claim: &PioLaneClaim) -> Result<(), PioError>;

    /// Writes one word into one claimed TX FIFO.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the lane is not part of the claim, the FIFO is full, or the
    /// backend cannot perform the write.
    fn write_tx_fifo(
        &self,
        claim: &PioLaneClaim,
        lane: PioLaneId,
        word: u32,
    ) -> Result<(), PioError>;

    /// Reads one word from one claimed RX FIFO.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the lane is not part of the claim, the FIFO is empty, or the
    /// backend cannot perform the read.
    fn read_rx_fifo(&self, claim: &PioLaneClaim, lane: PioLaneId) -> Result<u32, PioError>;
}

pub use PioBaseContract as PioBase;
pub use PioControlContract as PioControl;

/// Cortex-M SoC-local programmable-IO provider type.
#[derive(Debug, Clone, Copy, Default)]
pub struct CortexMSocPio;

/// Selected Cortex-M SoC-local programmable-IO provider alias.
pub type PlatformPio = CortexMSocPio;

/// Returns the selected Cortex-M SoC-local programmable-IO provider.
#[must_use]
pub const fn system_pio() -> PlatformPio {
    PlatformPio::new()
}

impl CortexMSocPio {
    /// Creates a new Cortex-M SoC-local programmable-IO provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl PioBase for CortexMSocPio {
    fn support(&self) -> PioSupport {
        super::board::pio_support()
    }

    fn engines(&self) -> &'static [PioEngineDescriptor] {
        super::board::pio_engines()
    }

    fn lanes(&self, engine: PioEngineId) -> &'static [PioLaneDescriptor] {
        super::board::pio_lanes(engine)
    }
}

impl PioControl for CortexMSocPio {
    fn claim_engine(&self, engine: PioEngineId) -> Result<PioEngineClaim, PioError> {
        super::board::claim_pio_engine(engine)
    }

    fn release_engine(&self, claim: PioEngineClaim) -> Result<(), PioError> {
        super::board::release_pio_engine(claim)
    }

    fn claim_lanes(
        &self,
        engine: PioEngineId,
        lanes: PioLaneMask,
    ) -> Result<PioLaneClaim, PioError> {
        super::board::claim_pio_lanes(engine, lanes)
    }

    fn release_lanes(&self, claim: PioLaneClaim) -> Result<(), PioError> {
        super::board::release_pio_lanes(claim)
    }

    fn load_program(
        &self,
        claim: &PioEngineClaim,
        image: &PioProgramImage<'_>,
    ) -> Result<PioProgramLease, PioError> {
        super::board::load_pio_program(claim, image)
    }

    fn unload_program(
        &self,
        claim: &PioEngineClaim,
        lease: PioProgramLease,
    ) -> Result<(), PioError> {
        super::board::unload_pio_program(claim, lease)
    }

    fn start_lanes(&self, claim: &PioLaneClaim) -> Result<(), PioError> {
        super::board::start_pio_lanes(claim)
    }

    fn stop_lanes(&self, claim: &PioLaneClaim) -> Result<(), PioError> {
        super::board::stop_pio_lanes(claim)
    }

    fn restart_lanes(&self, claim: &PioLaneClaim) -> Result<(), PioError> {
        super::board::restart_pio_lanes(claim)
    }

    fn write_tx_fifo(
        &self,
        claim: &PioLaneClaim,
        lane: PioLaneId,
        word: u32,
    ) -> Result<(), PioError> {
        super::board::write_pio_tx_fifo(claim, lane, word)
    }

    fn read_rx_fifo(&self, claim: &PioLaneClaim, lane: PioLaneId) -> Result<u32, PioError> {
        super::board::read_pio_rx_fifo(claim, lane)
    }
}
