//! GPU-oriented workload vocabulary over PCU kernels.
//!
//! This is the first explicit bridge from `fusion-gpu` to `fusion-pcu`:
//! - compute-fill work remains one dispatch kernel

use fusion_pcu::{
    PcuDispatchKernelIr,
    PcuKernel,
};

/// GPU-facing work item backed by one PCU kernel family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuWorkload<'a> {
    ComputeFill(&'a PcuDispatchKernelIr<'a>),
}

impl<'a> GpuWorkload<'a> {
    #[must_use]
    pub const fn pcu_kernel(self) -> PcuKernel<'a> {
        match self {
            Self::ComputeFill(kernel) => PcuKernel::Dispatch(*kernel),
        }
    }

    #[must_use]
    pub const fn is_compute_fill(self) -> bool {
        matches!(self, Self::ComputeFill(_))
    }
}

#[cfg(test)]
mod tests {
    use super::GpuWorkload;
    use fusion_pcu::{
        PcuKernel,
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
}
