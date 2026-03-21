//! fusion-sys programmable-IO wrappers, planning vocabulary, and portable IR.
//!
//! `fusion-sys::pcu` sits one layer above the raw fusion-pal PCU contract. It keeps backend
//! capabilities honest, adds engine-level pipeline planning vocabulary, and defines a small
//! deterministic IO-kernel IR without pretending every programmable-IO engine shares one ISA.

mod ir;
mod plan;
mod system;

pub use ir::*;
pub use plan::*;
pub use system::*;

pub use fusion_pal::sys::pcu::{
    PcuBase,
    PcuCaps,
    PcuClockDescriptor,
    PcuControl,
    PcuEngineClaim,
    PcuEngineDescriptor,
    PcuEngineId,
    PcuError,
    PcuErrorKind,
    PcuFifoDescriptor,
    PcuFifoDirection,
    PcuFifoId,
    PcuImplementationKind,
    PcuInstructionMemoryDescriptor,
    PcuLaneClaim,
    PcuLaneDescriptor,
    PcuLaneId,
    PcuLaneMask,
    PcuPinMappingCaps,
    PcuProgramId,
    PcuProgramImage,
    PcuProgramLease,
    PcuSupport,
};
