//! Backend-neutral programmable-IO vocabulary.

mod caps;
mod error;
mod types;
mod unsupported;

pub use caps::*;
pub use error::*;
pub use types::*;
pub use unsupported::*;

/// Capability trait for programmable-IO backends.
pub trait PcuBase {
    /// Reports the truthful programmable-IO surface for this backend.
    fn support(&self) -> PcuSupport;

    /// Returns the engine descriptors surfaced by this backend.
    #[must_use]
    fn engines(&self) -> &'static [PcuEngineDescriptor];

    /// Returns the lane descriptors for one engine, or an empty slice when the engine is
    /// unknown.
    #[must_use]
    fn lanes(&self, engine: PcuEngineId) -> &'static [PcuLaneDescriptor];
}

/// Control contract for programmable-IO backends.
pub trait PcuControl: PcuBase {
    /// Claims one engine exclusively.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the engine is unknown, unsupported, or already claimed.
    fn claim_engine(&self, engine: PcuEngineId) -> Result<PcuEngineClaim, PcuError>;

    /// Releases one previously claimed engine.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the claim no longer matches backend state.
    fn release_engine(&self, claim: PcuEngineClaim) -> Result<(), PcuError>;

    /// Claims one or more lanes within one engine.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the engine is unknown, the mask is invalid, or any lane is
    /// already claimed.
    fn claim_lanes(
        &self,
        engine: PcuEngineId,
        lanes: PcuLaneMask,
    ) -> Result<PcuLaneClaim, PcuError>;

    /// Releases one previously claimed lane mask.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the claim no longer matches backend state.
    fn release_lanes(&self, claim: PcuLaneClaim) -> Result<(), PcuError>;

    /// Loads one native program image into one claimed engine.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the image does not fit or the engine cannot accept it.
    fn load_program(
        &self,
        claim: &PcuEngineClaim,
        image: &PcuProgramImage<'_>,
    ) -> Result<PcuProgramLease, PcuError>;

    /// Unloads one previously loaded native program image from one claimed engine.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the lease no longer matches backend state.
    fn unload_program(
        &self,
        claim: &PcuEngineClaim,
        lease: PcuProgramLease,
    ) -> Result<(), PcuError>;

    /// Starts one claimed lane set.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the backend cannot start the requested lanes.
    fn start_lanes(&self, claim: &PcuLaneClaim) -> Result<(), PcuError>;

    /// Stops one claimed lane set.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the backend cannot stop the requested lanes.
    fn stop_lanes(&self, claim: &PcuLaneClaim) -> Result<(), PcuError>;

    /// Restarts one claimed lane set.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the backend cannot restart the requested lanes.
    fn restart_lanes(&self, claim: &PcuLaneClaim) -> Result<(), PcuError>;

    /// Writes one word into one claimed TX FIFO.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the lane is not part of the claim, the FIFO is full, or the
    /// backend cannot perform the write.
    fn write_tx_fifo(
        &self,
        claim: &PcuLaneClaim,
        lane: PcuLaneId,
        word: u32,
    ) -> Result<(), PcuError>;

    /// Reads one word from one claimed RX FIFO.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the lane is not part of the claim, the FIFO is empty, or the
    /// backend cannot perform the read.
    fn read_rx_fifo(&self, claim: &PcuLaneClaim, lane: PcuLaneId) -> Result<u32, PcuError>;
}
