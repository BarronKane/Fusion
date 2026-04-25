//! GPU-oriented workload vocabulary over PCU kernels.
//!
//! This is the first explicit bridge from `fusion-gpu` to `fusion-pcu`:
//! - compute-fill work remains one dispatch kernel
//! - future raster/mesh/ray-trace work flows through PCU render families

use fusion_pcu::{
    PcuDispatchKernelIr,
    PcuKernel,
    PcuRenderKernel,
};

/// GPU-facing work item backed by one PCU kernel family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuWorkload<'a> {
    ComputeFill(&'a PcuDispatchKernelIr<'a>),
    Render(&'a PcuRenderKernel<'a>),
}

impl<'a> GpuWorkload<'a> {
    #[must_use]
    pub const fn pcu_kernel(self) -> PcuKernel<'a> {
        match self {
            Self::ComputeFill(kernel) => PcuKernel::Dispatch(*kernel),
            Self::Render(kernel) => PcuKernel::Render(*kernel),
        }
    }

    #[must_use]
    pub const fn is_compute_fill(self) -> bool {
        matches!(self, Self::ComputeFill(_))
    }

    #[must_use]
    pub const fn is_render(self) -> bool {
        matches!(self, Self::Render(_))
    }
}

#[cfg(test)]
mod tests {
    use super::GpuWorkload;
    use fusion_pcu::{
        PcuKernel,
        PcuKernelId,
        PcuRasterFeatureCaps,
        PcuRasterKernelIr,
        PcuRenderKernel,
        PcuValueTypeCaps,
    };
    use fusion_pcu::model::PcuDispatchKernelBuilder;

    #[test]
    fn compute_fill_workload_wraps_dispatch_kernel() {
        let builder = PcuDispatchKernelBuilder::<1>::new(1, "fill", [1, 1, 1])
            .with_type_caps(PcuValueTypeCaps::UINT32 | PcuValueTypeCaps::SCALAR_VALUES)
            .with_arithmetic_op(fusion_pcu::PcuDispatchAluOp::Add)
            .expect("test dispatch builder should accept one op");
        let kernel = builder.ir();

        let workload = GpuWorkload::ComputeFill(&kernel);

        assert!(workload.is_compute_fill());
        assert!(matches!(workload.pcu_kernel(), PcuKernel::Dispatch(_)));
    }

    #[test]
    fn render_workload_wraps_render_kernel() {
        let kernel = PcuRenderKernel::Raster(PcuRasterKernelIr {
            id: PcuKernelId(2),
            entry_point: "raster",
            bindings: &[],
            ports: &[],
            parameters: &[],
            vertex_entry: "vs_main",
            fragment_entry: Some("fs_main"),
            type_caps: PcuValueTypeCaps::FLOAT32 | PcuValueTypeCaps::VECTOR_VALUES,
            features: PcuRasterFeatureCaps::VERTEX_STAGE
                .union(PcuRasterFeatureCaps::FRAGMENT_STAGE),
        });

        let workload = GpuWorkload::Render(&kernel);

        assert!(workload.is_render());
        assert!(matches!(workload.pcu_kernel(), PcuKernel::Render(_)));
    }
}
