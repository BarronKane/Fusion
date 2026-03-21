//! Cortex-M programmable-IO backend.

use crate::pcu::{
    PcuBase,
    PcuControl,
    PcuEngineClaim,
    PcuEngineDescriptor,
    PcuEngineId,
    PcuError,
    PcuLaneClaim,
    PcuLaneDescriptor,
    PcuLaneId,
    PcuLaneMask,
    PcuProgramImage,
    PcuProgramLease,
    PcuSupport,
};

/// Cortex-M programmable-IO provider type.
#[derive(Debug, Clone, Copy, Default)]
pub struct CortexMPcu;

/// Selected Cortex-M programmable-IO provider type.
pub type PlatformPcu = CortexMPcu;

/// Returns the selected Cortex-M programmable-IO provider.
#[must_use]
pub const fn system_pcu() -> PlatformPcu {
    PlatformPcu::new()
}

impl CortexMPcu {
    /// Creates a new Cortex-M programmable-IO provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl PcuBase for CortexMPcu {
    fn support(&self) -> PcuSupport {
        super::super::hal::soc::board::pcu_support()
    }

    fn engines(&self) -> &'static [PcuEngineDescriptor] {
        super::super::hal::soc::board::pcu_engines()
    }

    fn lanes(&self, engine: PcuEngineId) -> &'static [PcuLaneDescriptor] {
        super::super::hal::soc::board::pcu_lanes(engine)
    }
}

impl PcuControl for CortexMPcu {
    fn claim_engine(&self, engine: PcuEngineId) -> Result<PcuEngineClaim, PcuError> {
        super::super::hal::soc::board::claim_pcu_engine(engine)
    }

    fn release_engine(&self, claim: PcuEngineClaim) -> Result<(), PcuError> {
        super::super::hal::soc::board::release_pcu_engine(claim)
    }

    fn claim_lanes(
        &self,
        engine: PcuEngineId,
        lanes: PcuLaneMask,
    ) -> Result<PcuLaneClaim, PcuError> {
        super::super::hal::soc::board::claim_pcu_lanes(engine, lanes)
    }

    fn release_lanes(&self, claim: PcuLaneClaim) -> Result<(), PcuError> {
        super::super::hal::soc::board::release_pcu_lanes(claim)
    }

    fn load_program(
        &self,
        claim: &PcuEngineClaim,
        image: &PcuProgramImage<'_>,
    ) -> Result<PcuProgramLease, PcuError> {
        super::super::hal::soc::board::load_pcu_program(claim, image)
    }

    fn unload_program(
        &self,
        claim: &PcuEngineClaim,
        lease: PcuProgramLease,
    ) -> Result<(), PcuError> {
        super::super::hal::soc::board::unload_pcu_program(claim, lease)
    }

    fn start_lanes(&self, claim: &PcuLaneClaim) -> Result<(), PcuError> {
        super::super::hal::soc::board::start_pcu_lanes(claim)
    }

    fn stop_lanes(&self, claim: &PcuLaneClaim) -> Result<(), PcuError> {
        super::super::hal::soc::board::stop_pcu_lanes(claim)
    }

    fn restart_lanes(&self, claim: &PcuLaneClaim) -> Result<(), PcuError> {
        super::super::hal::soc::board::restart_pcu_lanes(claim)
    }

    fn write_tx_fifo(
        &self,
        claim: &PcuLaneClaim,
        lane: PcuLaneId,
        word: u32,
    ) -> Result<(), PcuError> {
        super::super::hal::soc::board::write_pcu_tx_fifo(claim, lane, word)
    }

    fn read_rx_fifo(&self, claim: &PcuLaneClaim, lane: PcuLaneId) -> Result<u32, PcuError> {
        super::super::hal::soc::board::read_pcu_rx_fifo(claim, lane)
    }
}
