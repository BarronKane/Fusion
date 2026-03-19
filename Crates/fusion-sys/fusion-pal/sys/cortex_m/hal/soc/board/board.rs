#![allow(clippy::doc_markdown)]

//! Cortex-M SoC board contract and generic helpers.

use crate::pal::hal::{
    HardwareAuthoritySet, HardwareError, HardwareGuarantee, HardwareTopologyCaps,
    HardwareTopologySummary, HardwareTopologySupport, HardwareWriteSummary,
};
use crate::pal::mem::{CachePolicy, MemResourceBackingKind, Protect, RegionAttrs};
use crate::pal::thread::{
    ThreadAuthoritySet, ThreadCoreId, ThreadError, ThreadExecutionLocation, ThreadId,
    ThreadLogicalCpuId, ThreadProcessorGroupId,
};

/// Runtime chip-identity surface available from the selected SoC board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CortexMSocChipIdSupport {
    /// No truthful runtime SoC chip-identity path is available.
    Unsupported,
    /// The SoC can surface a chip identity through firmware or ROM services.
    FirmwareReadable,
    /// The SoC can surface a chip identity through a memory-mapped register block.
    RegisterReadable,
}

/// Runtime chip-identity payload surfaced by the selected SoC board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CortexMSocChipIdentity {
    /// Raw board-defined chip-identity word.
    pub raw_chip_id: u32,
    /// Parsed silicon revision when the SoC exposes one.
    pub revision: Option<u8>,
    /// Parsed part identifier when the SoC exposes one.
    pub part: Option<u16>,
    /// Parsed manufacturer identifier when the SoC exposes one.
    pub manufacturer: Option<u16>,
    /// Board-defined package selector when the SoC exposes one.
    pub package: Option<u32>,
    /// Board-defined platform selector when the SoC exposes one.
    pub platform: Option<u32>,
    /// Board-defined implementation or source revision when the SoC exposes one.
    pub source_revision: Option<u32>,
}

/// Selected SoC descriptor used by the Cortex-M HAL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CortexMSocDescriptor {
    pub name: &'static str,
    pub topology_summary: Option<HardwareTopologySummary>,
    pub topology_authorities: HardwareAuthoritySet,
    pub chip_id_support: CortexMSocChipIdSupport,
}

/// Runtime execution-location observation provided by the selected SoC board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CortexMSocExecutionObservation {
    pub location: ThreadExecutionLocation,
    pub authorities: ThreadAuthoritySet,
}

/// Coarse kind of board-visible Cortex-M memory region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CortexMMemoryRegionKind {
    /// On-chip boot ROM or immutable mask ROM.
    Rom,
    /// Execute-in-place flash or external-memory alias window.
    Xip,
    /// On-chip SRAM visible to the CPU cores.
    Sram,
    /// MMIO, peripheral, or control window.
    Mmio,
}

/// Static memory-region descriptor for a Cortex-M SoC board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CortexMMemoryRegionDescriptor {
    /// Human-readable region name.
    pub name: &'static str,
    /// Coarse region kind.
    pub kind: CortexMMemoryRegionKind,
    /// Base address of the region.
    pub base: usize,
    /// Region length in bytes.
    pub len: usize,
    /// Effective protection contract for the region.
    pub protect: Protect,
    /// Effective region attributes.
    pub attrs: RegionAttrs,
    /// Effective cache policy.
    pub cache: CachePolicy,
    /// Coarse resource backing classification.
    pub backing: MemResourceBackingKind,
    /// Whether the region can be treated as allocator-usable backing.
    pub allocatable: bool,
}

/// Bus fabric segment for a named Cortex-M peripheral block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CortexMPeripheralBus {
    /// Low-bandwidth APB peripheral segment.
    Apb,
    /// High-bandwidth AHB peripheral segment.
    Ahb,
    /// Core-local single-cycle IO segment.
    Sio,
    /// Cortex private peripheral bus.
    Ppb,
}

/// Static peripheral descriptor for a Cortex-M SoC board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CortexMPeripheralDescriptor {
    /// Human-readable peripheral block name.
    pub name: &'static str,
    /// Fabric segment the peripheral is attached to.
    pub bus: CortexMPeripheralBus,
    /// Base address of the peripheral block.
    pub base: usize,
    /// Block length in bytes.
    pub len: usize,
}

/// Static clock descriptor for a Cortex-M SoC board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CortexMClockDescriptor {
    /// Clock name as surfaced by the board.
    pub name: &'static str,
    /// Primary clock-source selectors for this clock.
    pub main_sources: &'static [&'static str],
    /// Auxiliary clock-source selectors used by staged muxes for this clock.
    pub aux_sources: &'static [&'static str],
    /// Major consumers or sinks served by this clock.
    pub consumers: &'static [&'static str],
}

impl CortexMSocDescriptor {
    /// Returns the truthful topology support surface for this SoC descriptor.
    #[must_use]
    pub fn topology_support(self) -> HardwareTopologySupport {
        let Some(summary) = self.topology_summary else {
            return HardwareTopologySupport::unsupported();
        };

        let mut caps = HardwareTopologyCaps::SUMMARY;
        let summary_guarantee = HardwareGuarantee::Verified;
        let mut logical_cpus = HardwareGuarantee::Unsupported;
        let mut cores = HardwareGuarantee::Unsupported;
        let clusters = HardwareGuarantee::Unsupported;
        let packages = HardwareGuarantee::Unsupported;
        let numa_nodes = HardwareGuarantee::Unsupported;
        let core_classes = HardwareGuarantee::Unsupported;

        if summary.logical_cpu_count.is_some() {
            caps |= HardwareTopologyCaps::LOGICAL_CPUS;
            logical_cpus = HardwareGuarantee::Verified;
        }

        if summary.core_count.is_some() {
            caps |= HardwareTopologyCaps::CORES;
            cores = HardwareGuarantee::Verified;
        }

        HardwareTopologySupport {
            caps,
            summary: summary_guarantee,
            logical_cpus,
            cores,
            clusters,
            packages,
            numa_nodes,
            core_classes,
            authorities: self.topology_authorities,
            implementation: crate::pal::hal::HardwareImplementationKind::Native,
        }
    }
}

/// Trait contract implemented by Cortex-M SoC board modules.
pub trait CortexMSocBoard: Copy {
    /// Returns the static descriptor for this SoC family.
    fn descriptor(&self) -> CortexMSocDescriptor;

    /// Returns a truthful topology summary for this SoC family.
    ///
    /// # Errors
    ///
    /// Returns an error if this SoC family cannot surface topology honestly.
    fn topology_summary(&self) -> Result<HardwareTopologySummary, HardwareError> {
        self.descriptor()
            .topology_summary
            .ok_or_else(HardwareError::unsupported)
    }

    /// Writes scheduler-visible logical CPU identifiers for this SoC family.
    ///
    /// # Errors
    ///
    /// Returns an error if this SoC family cannot surface logical CPUs honestly.
    #[allow(clippy::cast_possible_truncation)]
    fn write_logical_cpus(
        &self,
        output: &mut [ThreadLogicalCpuId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        let total = self
            .topology_summary()?
            .logical_cpu_count
            .ok_or_else(HardwareError::unsupported)?;
        let written = output.len().min(total);

        for (index, slot) in output.iter_mut().take(written).enumerate() {
            *slot = ThreadLogicalCpuId {
                group: ThreadProcessorGroupId(0),
                index: index as u16,
            };
        }

        Ok(HardwareWriteSummary::new(total, written))
    }

    /// Writes topology-defined core identifiers for this SoC family.
    ///
    /// # Errors
    ///
    /// Returns an error if this SoC family cannot surface core identities honestly.
    #[allow(clippy::cast_possible_truncation)]
    fn write_cores(
        &self,
        output: &mut [ThreadCoreId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        let total = self
            .topology_summary()?
            .core_count
            .ok_or_else(HardwareError::unsupported)?;
        let written = output.len().min(total);

        for (index, slot) in output.iter_mut().take(written).enumerate() {
            *slot = ThreadCoreId(index as u32);
        }

        Ok(HardwareWriteSummary::new(total, written))
    }

    /// Returns the current execution location when this SoC can surface it honestly.
    ///
    /// # Errors
    ///
    /// Returns an error if the active core cannot be identified honestly.
    fn current_execution_location(&self) -> Result<CortexMSocExecutionObservation, ThreadError> {
        generic_single_core_observation(self.descriptor()).ok_or_else(ThreadError::unsupported)
    }

    /// Returns the current chip identity when this SoC can surface it honestly.
    ///
    /// # Errors
    ///
    /// Returns an error if the selected SoC cannot surface a truthful chip identity.
    fn chip_identity(&self) -> Result<CortexMSocChipIdentity, HardwareError> {
        Err(HardwareError::unsupported())
    }

    /// Returns the static memory-region descriptors surfaced by this SoC board.
    #[must_use]
    fn memory_map(&self) -> &'static [CortexMMemoryRegionDescriptor] {
        &[]
    }

    /// Returns the named peripheral blocks surfaced by this SoC board.
    #[must_use]
    fn peripherals(&self) -> &'static [CortexMPeripheralDescriptor] {
        &[]
    }

    /// Returns the major board-visible clock descriptors surfaced by this SoC board.
    #[must_use]
    fn clock_tree(&self) -> &'static [CortexMClockDescriptor] {
        &[]
    }
}

/// Returns the descriptor for the selected SoC provider.
#[must_use]
pub fn selected_soc<T: CortexMSocBoard>(soc: T) -> CortexMSocDescriptor {
    soc.descriptor()
}

/// Returns a coarse human-readable name for the selected SoC family.
#[must_use]
pub fn selected_soc_name<T: CortexMSocBoard>(soc: T) -> &'static str {
    selected_soc(soc).name
}

/// Returns the runtime chip-identity support class for the selected SoC.
#[must_use]
pub fn selected_soc_chip_id_support<T: CortexMSocBoard>(soc: T) -> CortexMSocChipIdSupport {
    selected_soc(soc).chip_id_support
}

/// Returns the runtime chip identity for the selected SoC board.
///
/// # Errors
///
/// Returns an error if the selected SoC cannot surface a truthful chip identity.
pub fn chip_identity<T: CortexMSocBoard>(soc: T) -> Result<CortexMSocChipIdentity, HardwareError> {
    soc.chip_identity()
}

/// Returns the truthful topology summary for the selected Cortex-M SoC.
///
/// # Errors
///
/// Returns an error if no SoC-specific topology summary is available.
pub fn topology_summary<T: CortexMSocBoard>(
    soc: T,
) -> Result<HardwareTopologySummary, HardwareError> {
    soc.topology_summary()
}

/// Writes scheduler-visible logical CPU identifiers for the selected SoC.
///
/// # Errors
///
/// Returns an error if the selected SoC does not expose a truthful logical-CPU model.
pub fn write_logical_cpus<T: CortexMSocBoard>(
    soc: T,
    output: &mut [ThreadLogicalCpuId],
) -> Result<HardwareWriteSummary, HardwareError> {
    soc.write_logical_cpus(output)
}

/// Writes topology-defined core identifiers for the selected SoC.
///
/// # Errors
///
/// Returns an error if the selected SoC does not expose a truthful core model.
pub fn write_cores<T: CortexMSocBoard>(
    soc: T,
    output: &mut [ThreadCoreId],
) -> Result<HardwareWriteSummary, HardwareError> {
    soc.write_cores(output)
}

/// Returns the current execution location when the selected SoC can surface it honestly.
///
/// # Errors
///
/// Returns an error if the selected SoC cannot identify the currently executing core.
pub fn current_execution_location<T: CortexMSocBoard>(
    soc: T,
) -> Result<CortexMSocExecutionObservation, ThreadError> {
    soc.current_execution_location()
}

/// Returns the current execution-context identifier when the selected SoC can surface it
/// honestly.
///
/// # Errors
///
/// Returns an error if the selected SoC cannot identify the currently executing core.
pub fn current_thread_id<T: CortexMSocBoard>(soc: T) -> Result<ThreadId, ThreadError> {
    let observation = current_execution_location(soc)?;

    if let Some(logical_cpu) = observation.location.logical_cpu {
        let group = u64::from(logical_cpu.group.0);
        let index = u64::from(logical_cpu.index);
        return Ok(ThreadId((group << 16) | index));
    }

    if let Some(core) = observation.location.core {
        return Ok(ThreadId(u64::from(core.0)));
    }

    Err(ThreadError::unsupported())
}

/// Returns the static memory-region descriptors for the selected SoC board.
#[must_use]
pub fn memory_map<T: CortexMSocBoard>(soc: T) -> &'static [CortexMMemoryRegionDescriptor] {
    soc.memory_map()
}

/// Returns the named peripheral blocks for the selected SoC board.
#[must_use]
pub fn peripherals<T: CortexMSocBoard>(soc: T) -> &'static [CortexMPeripheralDescriptor] {
    soc.peripherals()
}

/// Returns the major clock descriptors for the selected SoC board.
#[must_use]
pub fn clock_tree<T: CortexMSocBoard>(soc: T) -> &'static [CortexMClockDescriptor] {
    soc.clock_tree()
}

/// Returns a single-core execution observation when the descriptor honestly implies one.
#[must_use]
fn generic_single_core_observation(
    descriptor: CortexMSocDescriptor,
) -> Option<CortexMSocExecutionObservation> {
    match descriptor.topology_summary {
        Some(HardwareTopologySummary {
            logical_cpu_count: Some(1),
            core_count,
            ..
        }) if core_count.is_none_or(|count| count == 1) => Some(CortexMSocExecutionObservation {
            location: ThreadExecutionLocation {
                logical_cpu: Some(ThreadLogicalCpuId {
                    group: ThreadProcessorGroupId(0),
                    index: 0,
                }),
                core: Some(ThreadCoreId(0)),
                cluster: None,
                package: None,
                numa_node: None,
                core_class: None,
            },
            authorities: ThreadAuthoritySet::TOPOLOGY,
        }),
        Some(HardwareTopologySummary {
            logical_cpu_count: None,
            core_count: Some(1),
            ..
        }) => Some(CortexMSocExecutionObservation {
            location: ThreadExecutionLocation {
                logical_cpu: None,
                core: Some(ThreadCoreId(0)),
                cluster: None,
                package: None,
                numa_node: None,
                core_class: None,
            },
            authorities: ThreadAuthoritySet::TOPOLOGY,
        }),
        _ => None,
    }
}
