//! Dispatch-oriented profile layered over the PCU IR core.

use super::super::{
    PcuBinding,
    PcuInvocationModel,
    PcuIrKind,
    PcuKernelId,
    PcuKernelIr,
    PcuKernelSignature,
    PcuParameter,
    PcuPort,
};

bitflags::bitflags! {
    /// Coarse dispatch-profile capabilities required by one kernel.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PcuDispatchCapabilities: u32 {
        const INT32            = 1 << 0;
        const UINT32           = 1 << 1;
        const FLOAT16          = 1 << 2;
        const FLOAT32          = 1 << 3;
        const STORAGE_BUFFERS  = 1 << 4;
        const UNIFORM_BUFFERS  = 1 << 5;
        const PUSH_CONSTANTS   = 1 << 6;
        const WORKGROUP_MEMORY = 1 << 7;
    }
}

/// One entry-point descriptor for one dispatch profile kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuDispatchEntryPoint<'a> {
    pub name: &'a str,
    pub workgroup_size: [u32; 3],
}

/// Dispatch-oriented kernel profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuDispatchKernelIr<'a> {
    pub id: PcuKernelId,
    pub entry: PcuDispatchEntryPoint<'a>,
    pub bindings: &'a [PcuBinding<'a>],
    pub ports: &'a [PcuPort<'a>],
    pub parameters: &'a [PcuParameter<'a>],
    pub capabilities: PcuDispatchCapabilities,
}

impl PcuKernelIr for PcuDispatchKernelIr<'_> {
    fn id(&self) -> PcuKernelId {
        self.id
    }

    fn kind(&self) -> PcuIrKind {
        PcuIrKind::Dispatch
    }

    fn entry_point(&self) -> &str {
        self.entry.name
    }

    fn signature(&self) -> PcuKernelSignature<'_> {
        PcuKernelSignature {
            bindings: self.bindings,
            ports: self.ports,
            parameters: self.parameters,
            invocation: PcuInvocationModel::grid(self.entry.workgroup_size),
        }
    }
}

/// Back-compat alias while the higher layers stop saying "compute" when they really mean
/// "dispatch profile over the PCU core".
pub type PcuComputeEntryPoint<'a> = PcuDispatchEntryPoint<'a>;

/// Back-compat alias while the higher layers stop saying "compute" when they really mean
/// "dispatch profile over the PCU core".
pub type PcuComputeCapabilities = PcuDispatchCapabilities;

/// Back-compat alias while the higher layers stop saying "compute" when they really mean
/// "dispatch profile over the PCU core".
pub type PcuComputeKernelIr<'a> = PcuDispatchKernelIr<'a>;

/// Back-compat alias while the higher layers stop saying "shader" when they mean one dispatch
/// profile kernel.
pub type PcuComputeShaderIr<'a> = PcuDispatchKernelIr<'a>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_profile_reports_generic_kernel_shape() {
        let kernel = PcuDispatchKernelIr {
            id: PcuKernelId(11),
            entry: PcuDispatchEntryPoint {
                name: "main",
                workgroup_size: [8, 1, 1],
            },
            bindings: &[],
            ports: &[],
            parameters: &[],
            capabilities: PcuDispatchCapabilities::INT32 | PcuDispatchCapabilities::STORAGE_BUFFERS,
        };

        assert_eq!(kernel.kind(), PcuIrKind::Dispatch);
        assert_eq!(kernel.entry_point(), "main");
        assert_eq!(
            kernel.signature().invocation,
            PcuInvocationModel::grid([8, 1, 1])
        );
    }
}
