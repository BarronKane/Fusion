//! fusion-sys programmable-IO wrapper over the selected fusion-pal backend.

#[cfg(all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350"))]
use fusion_pal::sys::soc::cortex_m::hal::soc::pio::lower_rp2350_program;
use fusion_pal::sys::soc::cortex_m::hal::soc::pio::{
    PlatformPio,
    rp2350_build_execution_registers,
    rp2350_execution_is_default,
    system_pio as pal_system_pio,
};

use crate::event::EventSourceHandle;
use super::{
    PcuBaseContract,
    PcuControlContract,
    PcuDmaAttachment,
    PcuEngineClaim,
    PcuEngineDescriptor,
    PcuEngineId,
    PcuError,
    PcuEventAttachment,
    PcuIrExecutionConfig,
    PcuIrInstruction,
    PcuIrProgram,
    PcuLaneClaim,
    PcuLaneDescriptor,
    PcuLaneId,
    PcuLaneMask,
    PcuPipelineHandoff,
    PcuPipelineStage,
    PcuPipelineStageLease,
    PcuProgramImage,
    PcuProgramLease,
    PcuProgramSource,
    PcuSupport,
};

/// fusion-sys programmable-IO wrapper around the selected backend.
#[derive(Debug, Clone, Copy)]
pub struct PcuSystem {
    inner: PlatformPio,
}

impl PcuSystem {
    /// Creates a wrapper for the selected programmable-IO backend.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: pal_system_pio(),
        }
    }

    /// Reports the truthful programmable-IO surface for the selected backend.
    #[must_use]
    pub fn support(&self) -> PcuSupport {
        PcuBaseContract::support(&self.inner)
    }

    /// Returns the surfaced engine descriptors.
    #[must_use]
    pub fn engines(&self) -> &'static [PcuEngineDescriptor] {
        PcuBaseContract::engines(&self.inner)
    }

    /// Returns the surfaced lane descriptors for one engine.
    #[must_use]
    pub fn lanes(&self, engine: PcuEngineId) -> &'static [PcuLaneDescriptor] {
        PcuBaseContract::lanes(&self.inner, engine)
    }

    fn engine_descriptor(
        self,
        engine: PcuEngineId,
    ) -> Result<&'static PcuEngineDescriptor, PcuError> {
        self.engines()
            .iter()
            .find(|descriptor| descriptor.id == engine)
            .ok_or_else(PcuError::invalid)
    }

    fn resolved_execution_for_source(
        source: PcuProgramSource<'_>,
        execution_override: Option<PcuIrExecutionConfig>,
    ) -> Option<PcuIrExecutionConfig> {
        execution_override.or(match source {
            PcuProgramSource::Native(_) => None,
            PcuProgramSource::Ir(program) => Some(program.execution),
        })
    }

    /// Claims one engine exclusively.
    ///
    /// # Errors
    ///
    /// Returns any honest backend claim failure.
    pub fn claim_engine(&self, engine: PcuEngineId) -> Result<PcuEngineClaim, PcuError> {
        PcuControlContract::claim_engine(&self.inner, engine)
    }

    /// Releases one previously claimed engine.
    ///
    /// # Errors
    ///
    /// Returns any honest backend release failure.
    pub fn release_engine(&self, claim: PcuEngineClaim) -> Result<(), PcuError> {
        PcuControlContract::release_engine(&self.inner, claim)
    }

    /// Claims one or more lanes within one engine.
    ///
    /// # Errors
    ///
    /// Returns any honest backend claim failure.
    pub fn claim_lanes(
        &self,
        engine: PcuEngineId,
        lanes: PcuLaneMask,
    ) -> Result<PcuLaneClaim, PcuError> {
        PcuControlContract::claim_lanes(&self.inner, engine, lanes)
    }

    /// Returns one DMA pacing attachment for the TX FIFO of the supplied lane.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the engine is unknown or does not expose per-lane TX DREQ
    /// selectors.
    pub fn tx_dma_attachment(&self, lane: PcuLaneId) -> Result<PcuDmaAttachment, PcuError> {
        let engine = self.engine_descriptor(lane.engine)?;
        if lane.index >= engine.lane_count {
            return Err(PcuError::invalid());
        }
        let base = engine.tx_dreq_base.ok_or_else(PcuError::unsupported)?;
        Ok(PcuDmaAttachment::tx_for_lane(
            lane,
            base + u16::from(lane.index),
        ))
    }

    /// Returns one DMA pacing attachment for the RX FIFO of the supplied lane.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the engine is unknown or does not expose per-lane RX DREQ
    /// selectors.
    pub fn rx_dma_attachment(&self, lane: PcuLaneId) -> Result<PcuDmaAttachment, PcuError> {
        let engine = self.engine_descriptor(lane.engine)?;
        if lane.index >= engine.lane_count {
            return Err(PcuError::invalid());
        }
        let base = engine.rx_dreq_base.ok_or_else(PcuError::unsupported)?;
        Ok(PcuDmaAttachment::rx_for_lane(
            lane,
            base + u16::from(lane.index),
        ))
    }

    /// Returns one event attachment for one engine-local IRQ output.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the engine is unknown or the requested IRQ output does not
    /// exist.
    pub fn engine_event_attachment(
        &self,
        engine: PcuEngineId,
        line_index: usize,
    ) -> Result<PcuEventAttachment, PcuError> {
        let descriptor = self.engine_descriptor(engine)?;
        let irqn = *descriptor
            .irq_lines
            .get(line_index)
            .ok_or_else(PcuError::invalid)?;
        Ok(PcuEventAttachment {
            engine,
            irqn,
            source: EventSourceHandle(usize::from(irqn)),
        })
    }

    /// Releases one previously claimed lane mask.
    ///
    /// # Errors
    ///
    /// Returns any honest backend release failure.
    pub fn release_lanes(&self, claim: PcuLaneClaim) -> Result<(), PcuError> {
        PcuControlContract::release_lanes(&self.inner, claim)
    }

    /// Loads one backend-native program image into one claimed engine.
    ///
    /// # Errors
    ///
    /// Returns any honest backend load failure.
    pub fn load_program(
        &self,
        claim: &PcuEngineClaim,
        image: &PcuProgramImage<'_>,
    ) -> Result<PcuProgramLease, PcuError> {
        PcuControlContract::load_program(&self.inner, claim, image)
    }

    /// Lowers one portable deterministic IO kernel into backend-native words.
    ///
    /// # Errors
    ///
    /// Returns `unsupported` when the selected backend does not yet implement portable lowering
    /// for the supplied kernel.
    #[cfg(all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350"))]
    pub fn lower_program<'a>(
        &self,
        program: &PcuIrProgram<'_>,
        storage: &'a mut [u16],
    ) -> Result<PcuProgramImage<'a>, PcuError> {
        lower_rp2350_program(program, storage)
    }

    /// Lowers one portable deterministic IO kernel into backend-native words.
    ///
    /// # Errors
    ///
    /// Returns `unsupported` when the selected backend does not yet implement portable lowering
    /// for the supplied kernel.
    #[cfg(not(all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
    pub const fn lower_program<'a>(
        &self,
        program: &PcuIrProgram<'_>,
        storage: &'a mut [u16],
    ) -> Result<PcuProgramImage<'a>, PcuError> {
        let _ = storage;
        let _ = program;
        Err(PcuError::unsupported())
    }

    /// Loads one native or portable program source into one claimed engine.
    ///
    /// # Errors
    ///
    /// Returns any honest backend load or lowering failure.
    pub fn load_program_source(
        &self,
        claim: &PcuEngineClaim,
        source: PcuProgramSource<'_>,
        lowering_storage: &mut [u16],
    ) -> Result<PcuProgramLease, PcuError> {
        self.load_program_source_with_execution(claim, None, source, None, lowering_storage)
    }

    /// Loads one native or portable program source into one claimed engine and applies any
    /// supplied execution-state configuration.
    ///
    /// # Errors
    ///
    /// Returns any honest backend load, lowering, or execution-configuration failure.
    pub fn load_program_source_with_execution(
        &self,
        claim: &PcuEngineClaim,
        lane_claim: Option<&PcuLaneClaim>,
        source: PcuProgramSource<'_>,
        execution_override: Option<PcuIrExecutionConfig>,
        lowering_storage: &mut [u16],
    ) -> Result<PcuProgramLease, PcuError> {
        match source {
            PcuProgramSource::Native(image) => {
                let lease = self.load_program(claim, image)?;
                if let (Some(lane_claim), Some(execution)) = (
                    lane_claim,
                    Self::resolved_execution_for_source(
                        PcuProgramSource::Native(image),
                        execution_override,
                    ),
                ) {
                    Self::apply_execution_config_for_program(*lane_claim, &execution, None)?;
                }
                Ok(lease)
            }
            PcuProgramSource::Ir(program) => {
                let image = self.lower_program(program, lowering_storage)?;
                let lease = self.load_program(claim, &image)?;
                if let (Some(lane_claim), Some(execution)) = (
                    lane_claim,
                    Self::resolved_execution_for_source(
                        PcuProgramSource::Ir(program),
                        execution_override,
                    ),
                ) {
                    Self::apply_execution_config_for_program(
                        *lane_claim,
                        &execution,
                        Some(program.instructions),
                    )?;
                }
                Ok(lease)
            }
        }
    }

    /// Unloads one previously loaded program image.
    ///
    /// # Errors
    ///
    /// Returns any honest backend unload failure.
    pub fn unload_program(
        &self,
        claim: &PcuEngineClaim,
        lease: PcuProgramLease,
    ) -> Result<(), PcuError> {
        PcuControlContract::unload_program(&self.inner, claim, lease)
    }

    /// Starts one claimed lane set.
    ///
    /// # Errors
    ///
    /// Returns any honest backend control failure.
    pub fn start_lanes(&self, claim: &PcuLaneClaim) -> Result<(), PcuError> {
        PcuControlContract::start_lanes(&self.inner, claim)
    }

    /// Stops one claimed lane set.
    ///
    /// # Errors
    ///
    /// Returns any honest backend control failure.
    pub fn stop_lanes(&self, claim: &PcuLaneClaim) -> Result<(), PcuError> {
        PcuControlContract::stop_lanes(&self.inner, claim)
    }

    /// Restarts one claimed lane set.
    ///
    /// # Errors
    ///
    /// Returns any honest backend control failure.
    pub fn restart_lanes(&self, claim: &PcuLaneClaim) -> Result<(), PcuError> {
        PcuControlContract::restart_lanes(&self.inner, claim)
    }

    /// Applies the backend-equivalent lane initialization sequence for one loaded program.
    ///
    /// # Errors
    ///
    /// Returns any honest backend initialization failure.
    pub fn initialize_lanes(&self, claim: &PcuLaneClaim, initial_pc: u8) -> Result<(), PcuError> {
        #[cfg(all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350"))]
        {
            return fusion_pal::sys::soc::cortex_m::hal::soc::board::initialize_pcu_lanes(
                claim, initial_pc,
            );
        }

        #[cfg(not(all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
        {
            let _ = claim;
            let _ = initial_pc;
            Err(PcuError::unsupported())
        }
    }

    /// Writes one word into one claimed TX FIFO.
    ///
    /// # Errors
    ///
    /// Returns any honest backend FIFO failure.
    pub fn write_tx_fifo(
        &self,
        claim: &PcuLaneClaim,
        lane: PcuLaneId,
        word: u32,
    ) -> Result<(), PcuError> {
        PcuControlContract::write_tx_fifo(&self.inner, claim, lane, word)
    }

    /// Reads one word from one claimed RX FIFO.
    ///
    /// # Errors
    ///
    /// Returns any honest backend FIFO failure.
    pub fn read_rx_fifo(&self, claim: &PcuLaneClaim, lane: PcuLaneId) -> Result<u32, PcuError> {
        PcuControlContract::read_rx_fifo(&self.inner, claim, lane)
    }

    /// Applies one execution-state bundle to a claimed lane set.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the selected backend cannot realize the requested execution
    /// state.
    pub fn configure_execution(
        &self,
        claim: &PcuLaneClaim,
        execution: &PcuIrExecutionConfig,
    ) -> Result<(), PcuError> {
        Self::apply_execution_config_for_program(*claim, execution, None)
    }

    #[allow(clippy::missing_const_for_fn)]
    fn apply_execution_config_for_program(
        claim: PcuLaneClaim,
        execution: &PcuIrExecutionConfig,
        instructions: Option<&[PcuIrInstruction]>,
    ) -> Result<(), PcuError> {
        if rp2350_execution_is_default(execution) {
            return Ok(());
        }
        #[cfg(all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350"))]
        {
            let (clkdiv, execctrl, shiftctrl, pinctrl) =
                rp2350_build_execution_registers(execution, instructions)?;
            return fusion_pal::sys::soc::cortex_m::hal::soc::board::apply_pcu_execution_config(
                &claim, clkdiv, execctrl, shiftctrl, pinctrl,
            );
        }
        #[cfg(not(all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
        {
            let _ = claim;
            let _ = execution;
            let _ = instructions;
            Err(PcuError::unsupported())
        }
    }

    fn claim_pipeline_resources(
        self,
        stage: &PcuPipelineStage<'_>,
    ) -> Result<(PcuEngineClaim, PcuLaneClaim), PcuError> {
        let requested_lanes = u8::try_from(stage.lanes.bits().count_ones()).unwrap_or(u8::MAX);
        if let Some(engine) = stage.engine {
            let engine_claim = self.claim_engine(engine)?;
            match self.claim_lanes(engine, stage.lanes) {
                Ok(lane_claim) => return Ok((engine_claim, lane_claim)),
                Err(error) => {
                    let _ = self.release_engine(engine_claim);
                    return Err(error);
                }
            }
        }

        let mut last_error = PcuError::unsupported();
        for descriptor in self.engines() {
            if requested_lanes > descriptor.lane_count {
                continue;
            }
            let Ok(engine_claim) = self.claim_engine(descriptor.id) else {
                continue;
            };
            match self.claim_lanes(descriptor.id, stage.lanes) {
                Ok(lane_claim) => return Ok((engine_claim, lane_claim)),
                Err(error) => {
                    last_error = error;
                    let _ = self.release_engine(engine_claim);
                }
            }
        }
        Err(last_error)
    }

    /// Claims, loads, and configures one pipeline stage.
    ///
    /// # Errors
    ///
    /// Returns any honest claim, load, or execution-configuration failure.
    pub fn prepare_pipeline_stage<'a>(
        &self,
        stage: &PcuPipelineStage<'a>,
        lowering_storage: &mut [u16],
    ) -> Result<PcuPipelineStageLease<'a>, PcuError> {
        let (engine_claim, lane_claim) = self.claim_pipeline_resources(stage)?;
        let program_lease = match self.load_program_source_with_execution(
            &engine_claim,
            Some(&lane_claim),
            stage.program,
            stage.execution,
            lowering_storage,
        ) {
            Ok(lease) => lease,
            Err(error) => {
                let _ = self.release_lanes(lane_claim);
                let _ = self.release_engine(engine_claim);
                return Err(error);
            }
        };

        Ok(PcuPipelineStageLease {
            name: stage.name,
            engine_claim,
            lane_claim,
            program_lease,
            trigger: stage.trigger,
            handoff: stage.handoff,
        })
    }

    /// Starts one prepared pipeline stage.
    ///
    /// # Errors
    ///
    /// Returns any honest backend control failure.
    pub fn activate_pipeline_stage(
        &self,
        stage: &PcuPipelineStageLease<'_>,
    ) -> Result<(), PcuError> {
        self.start_lanes(&stage.lane_claim)
    }

    /// Releases one prepared pipeline stage and its claimed resources.
    ///
    /// # Errors
    ///
    /// Returns the first honest failure encountered while stopping, unloading, or releasing the
    /// stage.
    pub fn release_pipeline_stage(&self, stage: PcuPipelineStageLease<'_>) -> Result<(), PcuError> {
        self.stop_lanes(&stage.lane_claim)?;
        self.unload_program(&stage.engine_claim, stage.program_lease)?;
        self.release_lanes(stage.lane_claim)?;
        self.release_engine(stage.engine_claim)
    }

    /// Performs one staged handoff from the current stage into the next stage.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the selected handoff mode is unsupported for the prepared
    /// stages.
    pub fn handoff_pipeline_stage(
        &self,
        current: &PcuPipelineStageLease<'_>,
        next: &PcuPipelineStageLease<'_>,
    ) -> Result<(), PcuError> {
        match next.handoff {
            PcuPipelineHandoff::Manual => Ok(()),
            PcuPipelineHandoff::StopThenStart => {
                self.stop_lanes(&current.lane_claim)?;
                self.start_lanes(&next.lane_claim)
            }
            PcuPipelineHandoff::PreloadedStartThenStop => {
                if current.engine_claim.engine() == next.engine_claim.engine() {
                    return Err(PcuError::unsupported());
                }
                self.start_lanes(&next.lane_claim)?;
                self.stop_lanes(&current.lane_claim)
            }
            PcuPipelineHandoff::RestartClaimedLanes => Err(PcuError::unsupported()),
        }
    }

    /// Reprograms one prepared stage in place and restarts the claimed lanes.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the next stage is incompatible with the current claim or the
    /// backend cannot reload or restart the lanes.
    pub fn reprogram_pipeline_stage<'a>(
        &self,
        current: &mut PcuPipelineStageLease<'a>,
        next: &PcuPipelineStage<'a>,
        lowering_storage: &mut [u16],
    ) -> Result<(), PcuError> {
        let next_engine = next.engine.unwrap_or_else(|| current.engine_claim.engine());
        if next_engine != current.engine_claim.engine()
            || next.lanes.bits() != current.lane_claim.lanes().bits()
        {
            return Err(PcuError::unsupported());
        }

        self.stop_lanes(&current.lane_claim)?;
        self.unload_program(&current.engine_claim, current.program_lease)?;
        let program_lease = self.load_program_source_with_execution(
            &current.engine_claim,
            Some(&current.lane_claim),
            next.program,
            next.execution,
            lowering_storage,
        )?;
        self.restart_lanes(&current.lane_claim)?;
        self.start_lanes(&current.lane_claim)?;
        current.name = next.name;
        current.program_lease = program_lease;
        current.trigger = next.trigger;
        current.handoff = next.handoff;
        Ok(())
    }
}

/// Returns a wrapper for the selected Cortex-M programmable-IO backend.
#[must_use]
pub const fn system_pio() -> PcuSystem {
    PcuSystem::new()
}

impl Default for PcuSystem {
    fn default() -> Self {
        Self::new()
    }
}
