//! Engine-level programmable-IO staging and pipeline vocabulary.

use crate::event::EventSourceHandle;

use super::{PcuEngineId, PcuIrProgram, PcuLaneMask, PcuProgramId, PcuProgramImage};

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
