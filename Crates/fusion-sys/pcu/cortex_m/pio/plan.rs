//! Engine-level programmable-IO staging and pipeline vocabulary.

use crate::event::EventSourceHandle;

use super::{
    PcuEngineClaim,
    PcuEngineId,
    PcuFifoDirection,
    PcuFifoId,
    PcuIrExecutionConfig,
    PcuIrProgram,
    PcuLaneClaim,
    PcuLaneMask,
    PcuProgramId,
    PcuProgramImage,
    PcuProgramLease,
};

/// Program source for one pipeline stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuProgramSource<'a> {
    /// One backend-native instruction image.
    Native(&'a PcuProgramImage<'a>),
    /// One portable deterministic IO kernel requiring backend lowering.
    Ir(&'a PcuIrProgram<'a>),
}

impl PcuProgramSource<'_> {
    /// Returns the stable caller-supplied program identifier.
    #[must_use]
    pub const fn id(self) -> PcuProgramId {
        match self {
            Self::Native(image) => image.id,
            Self::Ir(program) => program.id,
        }
    }
}

/// Trigger policy for one pipeline stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuPipelineTrigger {
    /// Stage start is driven manually by the caller.
    Manual,
    /// Stage is ready to start immediately once resources are live.
    Immediate,
    /// Stage should start after the indexed stage completes its handoff.
    AfterStage(usize),
    /// Stage start is gated by one event source.
    Event(EventSourceHandle),
}

/// Handoff policy between pipeline stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuPipelineHandoff {
    /// Caller drives stop/start sequencing manually.
    Manual,
    /// Stop the current lanes before the next stage begins.
    StopThenStart,
    /// Preload the next engine image, then start it before stopping the current stage.
    PreloadedStartThenStop,
    /// Restart the same claimed lanes with the next loaded image.
    RestartClaimedLanes,
}

/// One engine-level pipeline stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuPipelineStage<'a> {
    /// Human-readable stage name.
    pub name: &'a str,
    /// Preferred engine, or `None` when the runtime may choose.
    pub engine: Option<PcuEngineId>,
    /// Lanes or state machines needed by this stage.
    pub lanes: PcuLaneMask,
    /// Program source to load or lower for this stage.
    pub program: PcuProgramSource<'a>,
    /// Optional execution-state override for this stage.
    ///
    /// When `program` is [`PcuProgramSource::Ir`], the stage uses the IR program's embedded
    /// execution config when this field is `None`.
    pub execution: Option<PcuIrExecutionConfig>,
    /// Trigger policy for starting this stage.
    pub trigger: PcuPipelineTrigger,
    /// Handoff policy from the previous stage.
    pub handoff: PcuPipelineHandoff,
}

/// One ordered programmable-IO pipeline plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuPipelinePlan<'a> {
    /// Human-readable pipeline name.
    pub name: &'a str,
    /// Ordered engine-level stage plan.
    pub stages: &'a [PcuPipelineStage<'a>],
}

/// One DMA pacing attachment for one PCU FIFO endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuDmaAttachment {
    /// FIFO endpoint being paced by the returned request selector.
    pub fifo: PcuFifoId,
    /// Platform DMA request selector for this endpoint.
    pub dreq_selector: u16,
}

impl PcuDmaAttachment {
    /// Creates one TX-side DMA attachment for the supplied lane.
    #[must_use]
    pub const fn tx_for_lane(lane: super::PcuLaneId, dreq_selector: u16) -> Self {
        Self {
            fifo: PcuFifoId {
                lane,
                direction: PcuFifoDirection::Tx,
            },
            dreq_selector,
        }
    }

    /// Creates one RX-side DMA attachment for the supplied lane.
    #[must_use]
    pub const fn rx_for_lane(lane: super::PcuLaneId, dreq_selector: u16) -> Self {
        Self {
            fifo: PcuFifoId {
                lane,
                direction: PcuFifoDirection::Rx,
            },
            dreq_selector,
        }
    }
}

/// One event-source attachment for one PCU engine-local IRQ output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuEventAttachment {
    /// Owning engine.
    pub engine: PcuEngineId,
    /// Hardware IRQ line surfaced by the engine.
    pub irqn: u16,
    /// Event-system source handle derived from the IRQ line.
    pub source: EventSourceHandle,
}

/// Prepared pipeline-stage lease holding the claimed resources for one stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuPipelineStageLease<'a> {
    /// Human-readable stage name.
    pub name: &'a str,
    /// Claimed engine hosting this stage.
    pub engine_claim: PcuEngineClaim,
    /// Claimed participating lanes.
    pub lane_claim: PcuLaneClaim,
    /// Loaded program image currently resident on the engine.
    pub program_lease: PcuProgramLease,
    /// Trigger policy for starting the stage.
    pub trigger: PcuPipelineTrigger,
    /// Handoff policy into this stage.
    pub handoff: PcuPipelineHandoff,
}
