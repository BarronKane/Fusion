//! Kernel identity and signature vocabulary for the PCU IR core.

use super::{PcuBinding, PcuInvocationModel, PcuPort};

/// Stable caller-supplied identifier for one generic PCU kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuKernelId(pub u32);

/// Coarse profile family carried by one generic PCU kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuIrKind {
    Dispatch,
    Stream,
}

/// Kernel-facing signature over memory truth, dataflow truth, and invocation truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuKernelSignature<'a> {
    pub bindings: &'a [PcuBinding<'a>],
    pub ports: &'a [PcuPort<'a>],
    pub invocation: PcuInvocationModel,
}

/// Minimal trait implemented by generic coprocessor IR payloads.
pub trait PcuKernelIr {
    /// Returns the stable caller-supplied kernel identifier.
    fn id(&self) -> PcuKernelId;

    /// Reports the coarse IR family carried by this kernel payload.
    fn kind(&self) -> PcuIrKind;

    /// Returns the entry-point name used for dispatch.
    fn entry_point(&self) -> &str;

    /// Returns the typed kernel signature for this profile.
    fn signature(&self) -> PcuKernelSignature<'_>;
}
