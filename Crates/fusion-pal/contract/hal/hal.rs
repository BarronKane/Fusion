pub mod caps;
pub mod cpu;
pub mod error;
pub mod unsupported;
#[path = "vector/vector.rs"]
pub mod vector;

pub use caps::*;
pub use cpu::*;
pub use error::*;
pub use unsupported::*;
pub use vector::*;

use crate::contract::runtime::thread::{
    ThreadClusterId,
    ThreadCoreClassId,
    ThreadCoreId,
    ThreadLogicalCpuId,
};

/// Common hardware-support surface for a selected HAL provider.
pub trait HardwareBase {
    /// Reports the truthful hardware-support surface available to the provider.
    fn support(&self) -> HardwareSupport;
}

/// CPU- and ABI-oriented hardware queries.
pub trait HardwareCpuQuery: HardwareBase {
    /// Returns a stable description of the selected target's CPU-facing execution model.
    ///
    /// # Errors
    ///
    /// Returns an error if the provider cannot characterize the selected target honestly.
    fn cpu_description(&self) -> Result<HardwareCpuDescription, HardwareError>;

    /// Returns stack-ABI facts relevant to user-space context setup and green-thread stacks.
    ///
    /// # Errors
    ///
    /// Returns an error if the provider cannot characterize the stack ABI honestly.
    fn stack_abi(&self) -> Result<HardwareStackAbi, HardwareError>;
}

/// Topology-oriented hardware queries.
pub trait HardwareTopologyQuery: HardwareBase {
    /// Returns a coarse topology summary for the current machine when one can be surfaced
    /// honestly without allocation.
    ///
    /// # Errors
    ///
    /// Returns an error if topology summary is unsupported.
    fn topology_summary(&self) -> Result<HardwareTopologySummary, HardwareError>;

    /// Writes scheduler-visible logical CPU identifiers into `output`.
    ///
    /// # Errors
    ///
    /// Returns an error if logical-CPU enumeration is unsupported.
    fn write_logical_cpus(
        &self,
        output: &mut [ThreadLogicalCpuId],
    ) -> Result<HardwareWriteSummary, HardwareError>;

    /// Writes physical or topology-defined core identifiers into `output`.
    ///
    /// # Errors
    ///
    /// Returns an error if core enumeration is unsupported.
    fn write_cores(
        &self,
        output: &mut [ThreadCoreId],
    ) -> Result<HardwareWriteSummary, HardwareError>;

    /// Writes cluster or LLC-domain identifiers into `output`.
    ///
    /// # Errors
    ///
    /// Returns an error if cluster enumeration is unsupported.
    fn write_clusters(
        &self,
        output: &mut [ThreadClusterId],
    ) -> Result<HardwareWriteSummary, HardwareError>;

    /// Writes package or socket topology identifiers into `output`.
    ///
    /// # Errors
    ///
    /// Returns an error if package enumeration is unsupported.
    fn write_packages(
        &self,
        output: &mut [HardwareTopologyNodeId],
    ) -> Result<HardwareWriteSummary, HardwareError>;

    /// Writes NUMA-node topology identifiers into `output`.
    ///
    /// # Errors
    ///
    /// Returns an error if NUMA-node enumeration is unsupported.
    fn write_numa_nodes(
        &self,
        output: &mut [HardwareTopologyNodeId],
    ) -> Result<HardwareWriteSummary, HardwareError>;

    /// Writes heterogeneous core-class identifiers into `output`.
    ///
    /// # Errors
    ///
    /// Returns an error if core-class enumeration is unsupported.
    fn write_core_classes(
        &self,
        output: &mut [ThreadCoreClassId],
    ) -> Result<HardwareWriteSummary, HardwareError>;
}
