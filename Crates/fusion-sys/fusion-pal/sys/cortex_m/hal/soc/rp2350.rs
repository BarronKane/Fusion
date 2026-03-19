//! RP2350 Cortex-M SoC descriptor.
//!
//! This module is where verified RP2350 memory-map, peripheral, and clock-tree facts belong.
//! The current scaffold keeps the truthful pieces wired first: topology and chip-identity
//! support class. Register-level identity reads should be added here once they are traced
//! against the RP2350 technical reference material instead of being reconstructed from memory
//! and vibes.

use core::ptr;

use crate::pal::hal::{
    HardwareAuthoritySet, HardwareError, HardwareTopologySummary, HardwareWriteSummary,
};
use crate::pal::thread::{
    ThreadAuthoritySet, ThreadCoreId, ThreadError, ThreadExecutionLocation, ThreadId,
    ThreadLogicalCpuId, ThreadProcessorGroupId,
};

use super::board_contract::{self, CortexMSocBoard};

pub use super::board_contract::{
    CortexMSocBoard as CortexMSoc, CortexMSocChipIdSupport, CortexMSocDescriptor,
    CortexMSocExecutionObservation,
};

/// Compile-time descriptor for the RP2350 SoC family.
pub const DESCRIPTOR: CortexMSocDescriptor = CortexMSocDescriptor {
    name: "rp2350",
    topology_summary: Some(HardwareTopologySummary {
        logical_cpu_count: Some(2),
        core_count: Some(2),
        cluster_count: None,
        package_count: None,
        numa_node_count: None,
        core_class_count: None,
    }),
    topology_authorities: HardwareAuthoritySet::TOPOLOGY,
    chip_id_support: CortexMSocChipIdSupport::RegisterReadable,
};

/// Placeholder RP2350 memory-map descriptor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rp2350MemoryMap;

/// Placeholder RP2350 peripheral-set descriptor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rp2350Peripherals;

/// Placeholder RP2350 clock-tree descriptor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rp2350ClockTree;

/// Returns the selected RP2350 memory-map descriptor.
#[must_use]
pub const fn memory_map() -> Rp2350MemoryMap {
    Rp2350MemoryMap
}

/// Returns the selected RP2350 peripheral descriptor set.
#[must_use]
pub const fn peripherals() -> Rp2350Peripherals {
    Rp2350Peripherals
}

/// Returns the selected RP2350 clock-tree descriptor.
#[must_use]
pub const fn clock_tree() -> Rp2350ClockTree {
    Rp2350ClockTree
}

/// RP2350 SoC provider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rp2350Soc;

impl Rp2350Soc {
    /// Creates a new RP2350 SoC provider.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

/// Selected SoC provider type for RP2350 builds.
pub type SocDevice = Rp2350Soc;

/// Returns the selected RP2350 SoC provider.
#[must_use]
pub const fn system_soc() -> SocDevice {
    SocDevice::new()
}

const RP2350_SIO_CPUID: *const u32 = 0xd000_0000 as *const u32;

impl CortexMSocBoard for Rp2350Soc {
    fn descriptor(&self) -> CortexMSocDescriptor {
        DESCRIPTOR
    }

    fn write_logical_cpus(
        &self,
        output: &mut [ThreadLogicalCpuId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        let total = 2;
        let written = output.len().min(total);

        for (index, slot) in output.iter_mut().take(written).enumerate() {
            *slot = ThreadLogicalCpuId {
                group: ThreadProcessorGroupId(0),
                index: index as u16,
            };
        }

        Ok(HardwareWriteSummary::new(total, written))
    }

    fn write_cores(
        &self,
        output: &mut [ThreadCoreId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        let total = 2;
        let written = output.len().min(total);

        for (index, slot) in output.iter_mut().take(written).enumerate() {
            *slot = ThreadCoreId(index as u32);
        }

        Ok(HardwareWriteSummary::new(total, written))
    }

    fn current_execution_location(&self) -> Result<CortexMSocExecutionObservation, ThreadError> {
        let current_core = unsafe { ptr::read_volatile(RP2350_SIO_CPUID) };
        if current_core > 1 {
            return Err(ThreadError::invalid());
        }

        let logical_cpu = ThreadLogicalCpuId {
            group: ThreadProcessorGroupId(0),
            index: current_core as u16,
        };

        Ok(CortexMSocExecutionObservation {
            location: ThreadExecutionLocation {
                logical_cpu: Some(logical_cpu),
                core: Some(ThreadCoreId(current_core)),
                cluster: None,
                package: None,
                numa_node: None,
                core_class: None,
            },
            authorities: ThreadAuthoritySet::FIRMWARE | ThreadAuthoritySet::TOPOLOGY,
        })
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
