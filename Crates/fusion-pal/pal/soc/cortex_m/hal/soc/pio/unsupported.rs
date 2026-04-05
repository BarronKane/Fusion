//! Cortex-M unsupported programmable-IO implementation.

use super::{
    PcuBaseContract,
    PcuControlContract,
    PcuEngineClaim,
    PcuEngineDescriptor,
    PcuEngineId,
    PcuError,
    PcuLaneClaim,
    PcuLaneDescriptor,
    PcuLaneMask,
    PcuProgramImage,
    PcuProgramLease,
    PcuSupport,
};

/// Unsupported programmable-IO provider placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedPcu;

impl UnsupportedPcu {
    /// Creates a new unsupported programmable-IO provider placeholder.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl PcuBaseContract for UnsupportedPcu {
    fn support(&self) -> PcuSupport {
        PcuSupport::unsupported()
    }

    fn engines(&self) -> &'static [PcuEngineDescriptor] {
        &[]
    }

    fn lanes(&self, _engine: PcuEngineId) -> &'static [PcuLaneDescriptor] {
        &[]
    }
}

impl PcuControlContract for UnsupportedPcu {
    fn claim_engine(&self, _engine: PcuEngineId) -> Result<PcuEngineClaim, PcuError> {
        Err(PcuError::unsupported())
    }

    fn release_engine(&self, _claim: PcuEngineClaim) -> Result<(), PcuError> {
        Err(PcuError::unsupported())
    }

    fn claim_lanes(
        &self,
        _engine: PcuEngineId,
        _lanes: PcuLaneMask,
    ) -> Result<PcuLaneClaim, PcuError> {
        Err(PcuError::unsupported())
    }

    fn release_lanes(&self, _claim: PcuLaneClaim) -> Result<(), PcuError> {
        Err(PcuError::unsupported())
    }

    fn load_program(
        &self,
        _claim: &PcuEngineClaim,
        _image: &PcuProgramImage<'_>,
    ) -> Result<PcuProgramLease, PcuError> {
        Err(PcuError::unsupported())
    }

    fn unload_program(
        &self,
        _claim: &PcuEngineClaim,
        _lease: PcuProgramLease,
    ) -> Result<(), PcuError> {
        Err(PcuError::unsupported())
    }

    fn start_lanes(&self, _claim: &PcuLaneClaim) -> Result<(), PcuError> {
        Err(PcuError::unsupported())
    }

    fn stop_lanes(&self, _claim: &PcuLaneClaim) -> Result<(), PcuError> {
        Err(PcuError::unsupported())
    }

    fn restart_lanes(&self, _claim: &PcuLaneClaim) -> Result<(), PcuError> {
        Err(PcuError::unsupported())
    }

    fn write_tx_fifo(
        &self,
        _claim: &PcuLaneClaim,
        _lane: super::PcuLaneId,
        _word: u32,
    ) -> Result<(), PcuError> {
        Err(PcuError::unsupported())
    }

    fn read_rx_fifo(
        &self,
        _claim: &PcuLaneClaim,
        _lane: super::PcuLaneId,
    ) -> Result<u32, PcuError> {
        Err(PcuError::unsupported())
    }
}
