//! Fusion-owned PCU IR core and profile vocabulary.

mod binding;
mod invocation;
mod kernel;
mod op;
mod parameter;
mod port;
#[path = "profile/profile.rs"]
mod profile;
mod types;

pub use binding::*;
pub use invocation::*;
pub use kernel::*;
pub use op::*;
pub use parameter::*;
pub use port::*;
pub use profile::*;
pub use types::*;

/// One generic semantic PCU kernel composed from one profile over the shared core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuKernel<'a> {
    Dispatch(PcuDispatchKernelIr<'a>),
    Stream(PcuStreamKernelIr<'a>),
}

impl<'a> PcuKernel<'a> {
    /// Returns the contained stream kernel, when this is one stream-profile payload.
    #[must_use]
    pub const fn as_stream(self) -> Option<PcuStreamKernelIr<'a>> {
        match self {
            Self::Dispatch(_) => None,
            Self::Stream(kernel) => Some(kernel),
        }
    }

    /// Returns the contained dispatch kernel, when this is one dispatch-profile payload.
    #[must_use]
    pub const fn as_dispatch(self) -> Option<PcuDispatchKernelIr<'a>> {
        match self {
            Self::Dispatch(kernel) => Some(kernel),
            Self::Stream(_) => None,
        }
    }

    /// Back-compat alias while the repo stops saying “compute” when it means “dispatch profile.”
    #[must_use]
    pub const fn as_compute(self) -> Option<PcuDispatchKernelIr<'a>> {
        self.as_dispatch()
    }
}

impl PcuKernelIr for PcuKernel<'_> {
    fn id(&self) -> PcuKernelId {
        match self {
            Self::Dispatch(kernel) => kernel.id(),
            Self::Stream(kernel) => kernel.id(),
        }
    }

    fn kind(&self) -> PcuIrKind {
        match self {
            Self::Dispatch(kernel) => kernel.kind(),
            Self::Stream(kernel) => kernel.kind(),
        }
    }

    fn entry_point(&self) -> &str {
        match self {
            Self::Dispatch(kernel) => kernel.entry_point(),
            Self::Stream(kernel) => kernel.entry_point(),
        }
    }

    fn signature(&self) -> PcuKernelSignature<'_> {
        match self {
            Self::Dispatch(kernel) => kernel.signature(),
            Self::Stream(kernel) => kernel.signature(),
        }
    }
}
