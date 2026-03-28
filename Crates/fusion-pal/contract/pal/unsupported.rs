//! Backend-neutral unsupported hardware-query implementation.

use crate::contract::pal::runtime::thread::{
    ThreadClusterId,
    ThreadCoreClassId,
    ThreadCoreId,
    ThreadLogicalCpuId,
};

use super::{
    HardwareBase,
    HardwareCpuDescription,
    HardwareCpuQuery,
    HardwareError,
    HardwareStackAbi,
    HardwareSupport,
    HardwareTopologyNodeId,
    HardwareTopologyQuery,
    HardwareTopologySummary,
    HardwareWriteSummary,
};

/// Unsupported hardware-query provider placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedHardware;

impl UnsupportedHardware {
    /// Creates a new unsupported hardware-query provider placeholder.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl HardwareBase for UnsupportedHardware {
    fn support(&self) -> HardwareSupport {
        HardwareSupport::unsupported()
    }
}

impl HardwareCpuQuery for UnsupportedHardware {
    fn cpu_description(&self) -> Result<HardwareCpuDescription, HardwareError> {
        Err(HardwareError::unsupported())
    }

    fn stack_abi(&self) -> Result<HardwareStackAbi, HardwareError> {
        Err(HardwareError::unsupported())
    }
}

impl HardwareTopologyQuery for UnsupportedHardware {
    fn topology_summary(&self) -> Result<HardwareTopologySummary, HardwareError> {
        Err(HardwareError::unsupported())
    }

    fn write_logical_cpus(
        &self,
        _output: &mut [ThreadLogicalCpuId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        Err(HardwareError::unsupported())
    }

    fn write_cores(
        &self,
        _output: &mut [ThreadCoreId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        Err(HardwareError::unsupported())
    }

    fn write_clusters(
        &self,
        _output: &mut [ThreadClusterId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        Err(HardwareError::unsupported())
    }

    fn write_packages(
        &self,
        _output: &mut [HardwareTopologyNodeId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        Err(HardwareError::unsupported())
    }

    fn write_numa_nodes(
        &self,
        _output: &mut [HardwareTopologyNodeId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        Err(HardwareError::unsupported())
    }

    fn write_core_classes(
        &self,
        _output: &mut [ThreadCoreClassId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        Err(HardwareError::unsupported())
    }
}
