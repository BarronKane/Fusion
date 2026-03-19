//! Generic Cortex-M SoC fallback used when no board feature is selected.

use crate::pal::hal::{
    HardwareAuthoritySet, HardwareError, HardwareTopologySummary, HardwareWriteSummary,
};
use crate::pal::thread::{ThreadCoreId, ThreadError, ThreadId, ThreadLogicalCpuId};

use super::board_contract::{self, CortexMSocBoard};

pub use super::board_contract::{
    CortexMSocBoard as CortexMSoc, CortexMSocChipIdSupport, CortexMSocDescriptor,
    CortexMSocExecutionObservation,
};

const DESCRIPTOR: CortexMSocDescriptor = CortexMSocDescriptor {
    name: "generic-cortex-m",
    topology_summary: None,
    topology_authorities: HardwareAuthoritySet::empty(),
    chip_id_support: CortexMSocChipIdSupport::Unsupported,
};

/// Generic Cortex-M SoC placeholder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GenericCortexMSoc;

impl GenericCortexMSoc {
    /// Creates a new generic Cortex-M SoC placeholder.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl CortexMSocBoard for GenericCortexMSoc {
    fn descriptor(&self) -> CortexMSocDescriptor {
        DESCRIPTOR
    }
}

/// Selected SoC provider type for generic Cortex-M builds.
pub type SocDevice = GenericCortexMSoc;

/// Returns the selected generic Cortex-M SoC provider.
#[must_use]
pub const fn system_soc() -> SocDevice {
    SocDevice::new()
}

/// Returns the compile-time selected Cortex-M SoC descriptor.
#[must_use]
pub fn selected_soc() -> CortexMSocDescriptor {
    board_contract::selected_soc(system_soc())
}

/// Returns a coarse human-readable name for the selected SoC family.
#[must_use]
pub fn selected_soc_name() -> &'static str {
    board_contract::selected_soc_name(system_soc())
}

/// Returns the runtime chip-identity support class for the selected SoC.
#[must_use]
pub fn selected_soc_chip_id_support() -> CortexMSocChipIdSupport {
    board_contract::selected_soc_chip_id_support(system_soc())
}

/// Returns the truthful topology summary for the selected Cortex-M SoC.
///
/// # Errors
///
/// Returns an error if no SoC-specific topology summary is available.
pub fn topology_summary() -> Result<HardwareTopologySummary, HardwareError> {
    board_contract::topology_summary(system_soc())
}

/// Writes scheduler-visible logical CPU identifiers for the selected SoC.
///
/// # Errors
///
/// Returns an error if the selected SoC does not expose a truthful logical-CPU model.
pub fn write_logical_cpus(
    output: &mut [ThreadLogicalCpuId],
) -> Result<HardwareWriteSummary, HardwareError> {
    board_contract::write_logical_cpus(system_soc(), output)
}

/// Writes topology-defined core identifiers for the selected SoC.
///
/// # Errors
///
/// Returns an error if the selected SoC does not expose a truthful core model.
pub fn write_cores(output: &mut [ThreadCoreId]) -> Result<HardwareWriteSummary, HardwareError> {
    board_contract::write_cores(system_soc(), output)
}

/// Returns the current execution location when the selected SoC can surface it honestly.
///
/// # Errors
///
/// Returns an error if the selected SoC cannot identify the currently executing core.
pub fn current_execution_location() -> Result<CortexMSocExecutionObservation, ThreadError> {
    board_contract::current_execution_location(system_soc())
}

/// Returns the current execution-context identifier when the selected SoC can surface it
/// honestly.
///
/// # Errors
///
/// Returns an error if the selected SoC cannot identify the currently executing core.
pub fn current_thread_id() -> Result<ThreadId, ThreadError> {
    board_contract::current_thread_id(system_soc())
}
