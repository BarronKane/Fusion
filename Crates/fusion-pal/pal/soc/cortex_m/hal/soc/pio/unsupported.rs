//! Cortex-M unsupported programmable-IO implementation.

use super::{
    PioBaseContract,
    PioControlContract,
    PioEngineClaim,
    PioEngineDescriptor,
    PioEngineId,
    PioError,
    PioLaneClaim,
    PioLaneDescriptor,
    PioLaneMask,
    PioProgramImage,
    PioProgramLease,
    PioSupport,
};

/// Unsupported programmable-IO provider placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedPio;

impl UnsupportedPio {
    /// Creates a new unsupported programmable-IO provider placeholder.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl PioBaseContract for UnsupportedPio {
    fn support(&self) -> PioSupport {
        PioSupport::unsupported()
    }

    fn engines(&self) -> &'static [PioEngineDescriptor] {
        &[]
    }

    fn lanes(&self, _engine: PioEngineId) -> &'static [PioLaneDescriptor] {
        &[]
    }
}

impl PioControlContract for UnsupportedPio {
    fn claim_engine(&self, _engine: PioEngineId) -> Result<PioEngineClaim, PioError> {
        Err(PioError::unsupported())
    }

    fn release_engine(&self, _claim: PioEngineClaim) -> Result<(), PioError> {
        Err(PioError::unsupported())
    }

    fn claim_lanes(
        &self,
        _engine: PioEngineId,
        _lanes: PioLaneMask,
    ) -> Result<PioLaneClaim, PioError> {
        Err(PioError::unsupported())
    }

    fn release_lanes(&self, _claim: PioLaneClaim) -> Result<(), PioError> {
        Err(PioError::unsupported())
    }

    fn load_program(
        &self,
        _claim: &PioEngineClaim,
        _image: &PioProgramImage<'_>,
    ) -> Result<PioProgramLease, PioError> {
        Err(PioError::unsupported())
    }

    fn unload_program(
        &self,
        _claim: &PioEngineClaim,
        _lease: PioProgramLease,
    ) -> Result<(), PioError> {
        Err(PioError::unsupported())
    }

    fn start_lanes(&self, _claim: &PioLaneClaim) -> Result<(), PioError> {
        Err(PioError::unsupported())
    }

    fn stop_lanes(&self, _claim: &PioLaneClaim) -> Result<(), PioError> {
        Err(PioError::unsupported())
    }

    fn restart_lanes(&self, _claim: &PioLaneClaim) -> Result<(), PioError> {
        Err(PioError::unsupported())
    }

    fn write_tx_fifo(
        &self,
        _claim: &PioLaneClaim,
        _lane: super::PioLaneId,
        _word: u32,
    ) -> Result<(), PioError> {
        Err(PioError::unsupported())
    }

    fn read_rx_fifo(
        &self,
        _claim: &PioLaneClaim,
        _lane: super::PioLaneId,
    ) -> Result<u32, PioError> {
        Err(PioError::unsupported())
    }
}
