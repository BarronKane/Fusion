#![allow(clippy::doc_markdown)]

//! STM32H7 Cortex-M SoC family scaffold.
//!
//! This family remains conservative until part-selection is modeled explicitly. The public
//! surface matches the RP2350 board so higher layers can consume a uniform selected-board API,
//! but the board facts stay empty rather than making up a family-wide memory map.

use crate::pal::hal::{
    HardwareAuthoritySet, HardwareError, HardwareTopologySummary, HardwareWriteSummary,
};
use crate::pal::thread::{ThreadCoreId, ThreadError, ThreadId, ThreadLogicalCpuId};

use super::board_contract::{self, CortexMSocBoard};

pub use super::board_contract::{
    CortexMClockDescriptor, CortexMMemoryRegionDescriptor, CortexMMemoryRegionKind,
    CortexMPeripheralDescriptor, CortexMSocBoard as CortexMSoc, CortexMSocChipIdSupport,
    CortexMSocChipIdentity, CortexMSocDescriptor, CortexMSocExecutionObservation,
};

/// Compile-time descriptor for the STM32H7 family scaffold.
pub const DESCRIPTOR: CortexMSocDescriptor = CortexMSocDescriptor {
    name: "stm32h7",
    topology_summary: None,
    topology_authorities: HardwareAuthoritySet::empty(),
    chip_id_support: CortexMSocChipIdSupport::Unsupported,
};

/// STM32H7 SoC provider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stm32h7Soc;

impl Stm32h7Soc {
    /// Creates a new STM32H7 SoC provider.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

/// Selected SoC provider type for STM32H7 builds.
pub type SocDevice = Stm32h7Soc;

/// Returns the selected STM32H7 SoC provider.
#[must_use]
pub const fn system_soc() -> SocDevice {
    SocDevice::new()
}

impl CortexMSocBoard for Stm32h7Soc {
    fn descriptor(&self) -> CortexMSocDescriptor {
        DESCRIPTOR
    }
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

/// Returns the runtime chip identity for the selected SoC.
///
/// # Errors
///
/// Returns an error if the selected SoC cannot surface a truthful chip identity.
pub fn chip_identity() -> Result<CortexMSocChipIdentity, HardwareError> {
    board_contract::chip_identity(system_soc())
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

/// Returns the selected STM32H7 memory map.
#[must_use]
pub fn memory_map() -> &'static [CortexMMemoryRegionDescriptor] {
    board_contract::memory_map(system_soc())
}

/// Returns the selected STM32H7 peripheral descriptors.
#[must_use]
pub fn peripherals() -> &'static [CortexMPeripheralDescriptor] {
    board_contract::peripherals(system_soc())
}

/// Returns the selected STM32H7 clock-tree descriptors.
#[must_use]
pub fn clock_tree() -> &'static [CortexMClockDescriptor] {
    board_contract::clock_tree(system_soc())
}
