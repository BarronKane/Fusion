//! Cortex-M SoC-layer programmable-IO vocabulary and backend contract.

use crate::pcu::PcuError;

mod error {
    pub use crate::pcu::PcuError;
}

mod caps;
mod ir;
mod kernels;
mod lowering;
mod types;
mod unsupported;

pub use crate::pcu::{PcuError as PioError, PcuErrorKind as PioErrorKind};
#[doc(hidden)]
pub use caps::{PcuCaps, PcuImplementationKind, PcuSupport};
pub use caps::{
    PcuCaps as PioCaps,
    PcuImplementationKind as PioImplementationKind,
    PcuSupport as PioSupport,
};
pub use ir::*;
pub use kernels::*;
pub use lowering::*;
#[doc(hidden)]
pub use types::{
    PcuClockDescriptor,
    PcuEngineClaim,
    PcuEngineDescriptor,
    PcuEngineId,
    PcuFifoDescriptor,
    PcuFifoDirection,
    PcuFifoId,
    PcuInstructionMemoryDescriptor,
    PcuLaneClaim,
    PcuLaneDescriptor,
    PcuLaneId,
    PcuLaneMask,
    PcuPinMappingCaps,
    PcuProgramId,
    PcuProgramImage,
    PcuProgramLease,
};
pub use types::{
    PcuClockDescriptor as PioClockDescriptor,
    PcuEngineClaim as PioEngineClaim,
    PcuEngineDescriptor as PioEngineDescriptor,
    PcuEngineId as PioEngineId,
    PcuFifoDescriptor as PioFifoDescriptor,
    PcuFifoDirection as PioFifoDirection,
    PcuFifoId as PioFifoId,
    PcuInstructionMemoryDescriptor as PioInstructionMemoryDescriptor,
    PcuLaneClaim as PioLaneClaim,
    PcuLaneDescriptor as PioLaneDescriptor,
    PcuLaneId as PioLaneId,
    PcuLaneMask as PioLaneMask,
    PcuPinMappingCaps as PioPinMappingCaps,
    PcuProgramId as PioProgramId,
    PcuProgramImage as PioProgramImage,
    PcuProgramLease as PioProgramLease,
};
#[doc(hidden)]
pub use unsupported::UnsupportedPcu;
pub use unsupported::UnsupportedPcu as UnsupportedPio;

/// Capability trait for Cortex-M programmable-IO backends.
#[doc(hidden)]
pub trait PcuBase {
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
pub trait PcuControl: PcuBase {
    /// Claims one engine exclusively.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the engine is unknown, unsupported, or already claimed.
    fn claim_engine(&self, engine: PioEngineId) -> Result<PioEngineClaim, PcuError>;

    /// Releases one previously claimed engine.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the claim no longer matches backend state.
    fn release_engine(&self, claim: PioEngineClaim) -> Result<(), PcuError>;

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
    ) -> Result<PioLaneClaim, PcuError>;

    /// Releases one previously claimed lane mask.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the claim no longer matches backend state.
    fn release_lanes(&self, claim: PioLaneClaim) -> Result<(), PcuError>;

    /// Loads one native program image into one claimed engine.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the image does not fit or the engine cannot accept it.
    fn load_program(
        &self,
        claim: &PioEngineClaim,
        image: &PioProgramImage<'_>,
    ) -> Result<PioProgramLease, PcuError>;

    /// Unloads one previously loaded native program image from one claimed engine.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the lease no longer matches backend state.
    fn unload_program(
        &self,
        claim: &PioEngineClaim,
        lease: PioProgramLease,
    ) -> Result<(), PcuError>;

    /// Starts one claimed lane set.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the backend cannot start the requested lanes.
    fn start_lanes(&self, claim: &PioLaneClaim) -> Result<(), PcuError>;

    /// Stops one claimed lane set.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the backend cannot stop the requested lanes.
    fn stop_lanes(&self, claim: &PioLaneClaim) -> Result<(), PcuError>;

    /// Restarts one claimed lane set.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the backend cannot restart the requested lanes.
    fn restart_lanes(&self, claim: &PioLaneClaim) -> Result<(), PcuError>;

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
    ) -> Result<(), PcuError>;

    /// Reads one word from one claimed RX FIFO.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the lane is not part of the claim, the FIFO is empty, or the
    /// backend cannot perform the read.
    fn read_rx_fifo(&self, claim: &PioLaneClaim, lane: PioLaneId) -> Result<u32, PcuError>;
}

pub use PcuBase as PioBase;
pub use PcuControl as PioControl;

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
        super::board::pcu_support()
    }

    fn engines(&self) -> &'static [PioEngineDescriptor] {
        super::board::pcu_engines()
    }

    fn lanes(&self, engine: PioEngineId) -> &'static [PioLaneDescriptor] {
        super::board::pcu_lanes(engine)
    }
}

impl PioControl for CortexMSocPio {
    fn claim_engine(&self, engine: PioEngineId) -> Result<PioEngineClaim, PioError> {
        super::board::claim_pcu_engine(engine)
    }

    fn release_engine(&self, claim: PioEngineClaim) -> Result<(), PioError> {
        super::board::release_pcu_engine(claim)
    }

    fn claim_lanes(
        &self,
        engine: PioEngineId,
        lanes: PioLaneMask,
    ) -> Result<PioLaneClaim, PioError> {
        super::board::claim_pcu_lanes(engine, lanes)
    }

    fn release_lanes(&self, claim: PioLaneClaim) -> Result<(), PioError> {
        super::board::release_pcu_lanes(claim)
    }

    fn load_program(
        &self,
        claim: &PioEngineClaim,
        image: &PioProgramImage<'_>,
    ) -> Result<PioProgramLease, PioError> {
        super::board::load_pcu_program(claim, image)
    }

    fn unload_program(
        &self,
        claim: &PioEngineClaim,
        lease: PioProgramLease,
    ) -> Result<(), PioError> {
        super::board::unload_pcu_program(claim, lease)
    }

    fn start_lanes(&self, claim: &PioLaneClaim) -> Result<(), PioError> {
        super::board::start_pcu_lanes(claim)
    }

    fn stop_lanes(&self, claim: &PioLaneClaim) -> Result<(), PioError> {
        super::board::stop_pcu_lanes(claim)
    }

    fn restart_lanes(&self, claim: &PioLaneClaim) -> Result<(), PioError> {
        super::board::restart_pcu_lanes(claim)
    }

    fn write_tx_fifo(
        &self,
        claim: &PioLaneClaim,
        lane: PioLaneId,
        word: u32,
    ) -> Result<(), PioError> {
        super::board::write_pcu_tx_fifo(claim, lane, word)
    }

    fn read_rx_fifo(&self, claim: &PioLaneClaim, lane: PioLaneId) -> Result<u32, PioError> {
        super::board::read_pcu_rx_fifo(claim, lane)
    }
}
