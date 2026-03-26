//! Generic coprocessor IR vocabulary.

#[path = "ir/compute.rs"]
mod compute;
#[path = "ir/stream.rs"]
mod stream;

pub use compute::*;
pub use stream::*;

/// Stable caller-supplied identifier for one generic PCU kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuKernelId(pub u32);

/// Coarse IR category carried by one generic PCU kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuIrKind {
    Compute,
    Stream,
}

/// Minimal trait implemented by generic coprocessor IR payloads.
pub trait PcuKernelIr {
    /// Returns the stable caller-supplied kernel identifier.
    fn id(&self) -> PcuKernelId;

    /// Reports the coarse IR family carried by this kernel payload.
    fn kind(&self) -> PcuIrKind;

    /// Returns the entry-point name used for dispatch.
    fn entry_point(&self) -> &str;
}

/// One generic semantic PCU kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuKernel<'a> {
    Compute(PcuComputeKernelIr<'a>),
    Stream(PcuStreamKernelIr<'a>),
}

impl<'a> PcuKernel<'a> {
    /// Returns the contained stream kernel, when this is one stream-dialect payload.
    #[must_use]
    pub const fn as_stream(self) -> Option<PcuStreamKernelIr<'a>> {
        match self {
            Self::Compute(_) => None,
            Self::Stream(kernel) => Some(kernel),
        }
    }

    /// Returns the contained compute kernel, when this is one compute-dialect payload.
    #[must_use]
    pub const fn as_compute(self) -> Option<PcuComputeKernelIr<'a>> {
        match self {
            Self::Compute(kernel) => Some(kernel),
            Self::Stream(_) => None,
        }
    }
}

impl PcuKernelIr for PcuKernel<'_> {
    fn id(&self) -> PcuKernelId {
        match self {
            Self::Compute(kernel) => kernel.id(),
            Self::Stream(kernel) => kernel.id(),
        }
    }

    fn kind(&self) -> PcuIrKind {
        match self {
            Self::Compute(kernel) => kernel.kind(),
            Self::Stream(kernel) => kernel.kind(),
        }
    }

    fn entry_point(&self) -> &str {
        match self {
            Self::Compute(kernel) => kernel.entry_point(),
            Self::Stream(kernel) => kernel.entry_point(),
        }
    }
}
