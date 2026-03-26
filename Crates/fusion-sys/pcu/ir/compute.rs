//! Minimal semantic compute-kernel IR vocabulary for coprocessor dispatch.

use super::{PcuIrKind, PcuKernelId, PcuKernelIr};

/// Scalar element types surfaced by the current compute IR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuComputeScalarType {
    Bool,
    I32,
    U32,
    F16,
    F32,
}

/// Value shapes surfaced by the current compute IR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuComputeValueType {
    Scalar(PcuComputeScalarType),
    Vector {
        scalar: PcuComputeScalarType,
        lanes: u8,
    },
}

/// Storage-class vocabulary for one compute resource binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuComputeStorageClass {
    Input,
    Output,
    Uniform,
    Storage,
    Workgroup,
    PushConstant,
    Private,
}

/// Builtin values surfaced by the current compute IR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuComputeBuiltin {
    GlobalInvocationId,
    LocalInvocationId,
    WorkgroupId,
    NumWorkgroups,
    LocalInvocationIndex,
}

bitflags::bitflags! {
    /// Coarse compute-shader capabilities required by one kernel.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PcuComputeCapabilities: u32 {
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

/// One explicit resource binding used by one compute shader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuComputeBinding<'a> {
    pub name: Option<&'a str>,
    pub set: u32,
    pub binding: u32,
    pub storage: PcuComputeStorageClass,
    pub value_type: PcuComputeValueType,
    pub builtin: Option<PcuComputeBuiltin>,
}

/// One entry-point descriptor for one compute kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuComputeEntryPoint<'a> {
    pub name: &'a str,
    pub workgroup_size: [u32; 3],
}

/// Minimal semantic compute-kernel IR payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuComputeKernelIr<'a> {
    pub id: PcuKernelId,
    pub entry: PcuComputeEntryPoint<'a>,
    pub bindings: &'a [PcuComputeBinding<'a>],
    pub capabilities: PcuComputeCapabilities,
}

impl PcuKernelIr for PcuComputeKernelIr<'_> {
    fn id(&self) -> PcuKernelId {
        self.id
    }

    fn kind(&self) -> PcuIrKind {
        PcuIrKind::Compute
    }

    fn entry_point(&self) -> &str {
        self.entry.name
    }
}

/// Back-compat alias while the higher layers stop saying "shader" when they mean "kernel".
pub type PcuComputeShaderIr<'a> = PcuComputeKernelIr<'a>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_kernel_ir_reports_generic_kernel_shape() {
        let kernel = PcuComputeKernelIr {
            id: PcuKernelId(11),
            entry: PcuComputeEntryPoint {
                name: "main",
                workgroup_size: [8, 1, 1],
            },
            bindings: &[],
            capabilities: PcuComputeCapabilities::INT32 | PcuComputeCapabilities::STORAGE_BUFFERS,
        };

        assert_eq!(kernel.kind(), PcuIrKind::Compute);
        assert_eq!(kernel.entry_point(), "main");
    }
}
