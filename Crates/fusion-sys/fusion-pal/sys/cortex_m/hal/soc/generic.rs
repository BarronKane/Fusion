#![allow(clippy::doc_markdown)]

//! Generic Cortex-M SoC fallback used when no board feature is selected.

use crate::pal::hal::{
    HardwareAuthoritySet,
    HardwareError,
    HardwareTopologySummary,
    HardwareWriteSummary,
};
use crate::pal::thread::{ThreadCoreId, ThreadError, ThreadId, ThreadLogicalCpuId};
use crate::pal::{
    hal::HardwareTopologyNodeId,
    thread::{ThreadClusterId, ThreadCoreClassId},
};
use crate::pcu::{
    PcuEngineClaim,
    PcuEngineDescriptor,
    PcuEngineId,
    PcuError,
    PcuLaneClaim,
    PcuLaneDescriptor,
    PcuLaneId,
    PcuLaneMask,
    PcuProgramImage,
    PcuProgramLease,
    PcuSupport,
};
use core::time::Duration;

use super::board_contract::{self, CortexMSocBoard};

pub use super::board_contract::{
    CortexMClockDescriptor,
    CortexMDmaControllerDescriptor,
    CortexMDmaRequestClass,
    CortexMDmaRequestDescriptor,
    CortexMExceptionStackObservation,
    CortexMFlashRegionDescriptor,
    CortexMIrqClass,
    CortexMIrqDescriptor,
    CortexMMemoryRegionDescriptor,
    CortexMMemoryRegionKind,
    CortexMPeripheralDescriptor,
    CortexMPowerModeDescriptor,
    CortexMSocBoard as CortexMSoc,
    CortexMSocChipIdSupport,
    CortexMSocChipIdentity,
    CortexMSocDescriptor,
    CortexMSocDeviceIdSupport,
    CortexMSocDeviceIdentity,
    CortexMSocExecutionObservation,
};

const DESCRIPTOR: CortexMSocDescriptor = CortexMSocDescriptor {
    name: "generic-cortex-m",
    topology_summary: Some(HardwareTopologySummary {
        logical_cpu_count: None,
        core_count: None,
        cluster_count: Some(1),
        package_count: Some(1),
        numa_node_count: None,
        core_class_count: Some(1),
    }),
    topology_authorities: HardwareAuthoritySet::TOPOLOGY,
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

/// Whether local interrupt masking is sufficient to serialize local synchronization on this
/// generic Cortex-M target.
pub const LOCAL_CRITICAL_SECTION_SYNC_SAFE: bool = false;

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

/// Returns whether the selected SoC currently has enough honest exception-stack headroom for one
/// inline urgent handler body.
#[must_use]
pub fn inline_current_exception_stack_allows(required_bytes: usize) -> bool {
    board_contract::inline_current_exception_stack_allows(system_soc(), required_bytes)
}

/// Returns one observation of the selected SoC board's main/exception stack window.
///
/// # Errors
///
/// Returns an error if the selected board cannot surface the reserved stack window and current MSP
/// honestly.
pub fn exception_stack_observation() -> Result<CortexMExceptionStackObservation, HardwareError> {
    board_contract::exception_stack_observation(system_soc())
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

/// Returns the runtime per-device identity support class for the selected board.
#[must_use]
pub fn selected_soc_device_id_support() -> CortexMSocDeviceIdSupport {
    board_contract::selected_soc_device_id_support(system_soc())
}

/// Returns the runtime per-device identity for the selected board.
///
/// # Errors
///
/// Returns an error because the generic fallback cannot surface a truthful board identity.
pub fn device_identity() -> Result<CortexMSocDeviceIdentity, HardwareError> {
    board_contract::device_identity(system_soc())
}

/// Returns whether local interrupt masking is sufficient to serialize local synchronization on the
/// selected generic Cortex-M target.
#[must_use]
pub fn local_critical_section_sync_safe() -> bool {
    board_contract::local_critical_section_sync_safe(system_soc())
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

/// Writes topology-defined cluster identifiers for the selected generic Cortex-M SoC.
///
/// # Errors
///
/// Returns an error if the selected SoC does not expose cluster identities honestly.
pub fn write_clusters(
    output: &mut [ThreadClusterId],
) -> Result<HardwareWriteSummary, HardwareError> {
    board_contract::write_clusters(system_soc(), output)
}

/// Writes topology-defined package identifiers for the selected generic Cortex-M SoC.
///
/// # Errors
///
/// Returns an error if the selected SoC does not expose package identities honestly.
pub fn write_packages(
    output: &mut [HardwareTopologyNodeId],
) -> Result<HardwareWriteSummary, HardwareError> {
    board_contract::write_packages(system_soc(), output)
}

/// Writes topology-defined core-class identifiers for the selected generic Cortex-M SoC.
///
/// # Errors
///
/// Returns an error if the selected SoC does not expose core classes honestly.
pub fn write_core_classes(
    output: &mut [ThreadCoreClassId],
) -> Result<HardwareWriteSummary, HardwareError> {
    board_contract::write_core_classes(system_soc(), output)
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

/// Returns the selected generic Cortex-M memory map.
#[must_use]
pub fn memory_map() -> &'static [CortexMMemoryRegionDescriptor] {
    board_contract::memory_map(system_soc())
}

/// Returns the number of board-owned runtime memory regions for the selected generic Cortex-M
/// board.
#[must_use]
pub fn owned_memory_region_count() -> usize {
    board_contract::owned_memory_region_count(system_soc())
}

/// Returns one board-owned runtime memory region for the selected generic Cortex-M board.
#[must_use]
pub fn owned_memory_region(index: usize) -> Option<CortexMMemoryRegionDescriptor> {
    board_contract::owned_memory_region(system_soc(), index)
}

/// Returns the selected generic Cortex-M peripheral descriptors.
#[must_use]
pub fn peripherals() -> &'static [CortexMPeripheralDescriptor] {
    board_contract::peripherals(system_soc())
}

/// Returns the selected generic Cortex-M IRQ descriptors.
#[must_use]
pub fn irqs() -> &'static [CortexMIrqDescriptor] {
    board_contract::irqs(system_soc())
}

/// Enables one named external IRQ line on the selected generic Cortex-M board.
///
/// # Errors
///
/// Returns an error because the generic fallback cannot enable board-specific IRQ lines
/// honestly.
pub fn irq_enable(irqn: u16) -> Result<(), HardwareError> {
    board_contract::irq_enable(system_soc(), irqn)
}

/// Disables one named external IRQ line on the selected generic Cortex-M board.
///
/// # Errors
///
/// Returns an error because the generic fallback cannot disable board-specific IRQ lines
/// honestly.
pub fn irq_disable(irqn: u16) -> Result<(), HardwareError> {
    board_contract::irq_disable(system_soc(), irqn)
}

/// Returns whether one IRQ line can be acknowledged generically on the selected generic
/// Cortex-M board.
#[must_use]
pub fn irq_acknowledge_supported(irqn: u16) -> bool {
    board_contract::irq_acknowledge_supported(system_soc(), irqn)
}

/// Acknowledges one IRQ line on the selected generic Cortex-M board.
///
/// # Errors
///
/// Returns an error because the generic fallback cannot acknowledge board-specific IRQ lines
/// honestly.
pub fn irq_acknowledge(irqn: u16) -> Result<(), HardwareError> {
    board_contract::irq_acknowledge(system_soc(), irqn)
}

/// Returns whether the selected generic Cortex-M board exposes a truthful finite event-timeout
/// source.
#[must_use]
pub fn event_timeout_supported() -> bool {
    board_contract::event_timeout_supported(system_soc())
}

/// Returns the board-reserved IRQ line used by the selected generic Cortex-M board's event
/// timeout source.
#[must_use]
pub fn event_timeout_irq() -> Option<u16> {
    board_contract::event_timeout_irq(system_soc())
}

/// Arms the selected generic Cortex-M board's event-timeout source.
///
/// # Errors
///
/// Returns an error because the generic fallback cannot surface a truthful finite event-timeout
/// source.
pub fn arm_event_timeout(timeout: Duration) -> Result<(), HardwareError> {
    board_contract::arm_event_timeout(system_soc(), timeout)
}

/// Cancels the selected generic Cortex-M board's event-timeout source.
///
/// # Errors
///
/// Returns an error because the generic fallback cannot surface a truthful finite event-timeout
/// source.
pub fn cancel_event_timeout() -> Result<(), HardwareError> {
    board_contract::cancel_event_timeout(system_soc())
}

/// Returns whether the selected generic Cortex-M board's event-timeout source has fired.
///
/// # Errors
///
/// Returns an error because the generic fallback cannot surface a truthful finite event-timeout
/// source.
pub fn event_timeout_fired() -> Result<bool, HardwareError> {
    board_contract::event_timeout_fired(system_soc())
}

/// Returns the selected generic Cortex-M clock descriptors.
#[must_use]
pub fn clock_tree() -> &'static [CortexMClockDescriptor] {
    board_contract::clock_tree(system_soc())
}

/// Returns the selected generic Cortex-M DMA controller descriptors.
#[must_use]
pub fn dma_controllers() -> &'static [CortexMDmaControllerDescriptor] {
    board_contract::dma_controllers(system_soc())
}

/// Returns the selected generic Cortex-M DMA request descriptors.
#[must_use]
pub fn dma_requests() -> &'static [CortexMDmaRequestDescriptor] {
    board_contract::dma_requests(system_soc())
}

/// Returns the selected generic Cortex-M power-mode descriptors.
#[must_use]
pub fn power_modes() -> &'static [CortexMPowerModeDescriptor] {
    board_contract::power_modes(system_soc())
}

/// Returns the selected generic Cortex-M PAL-facing power descriptors.
#[must_use]
pub fn pal_power_modes() -> &'static [crate::pal::power::PowerModeDescriptor] {
    board_contract::pal_power_modes(system_soc())
}

/// Enters one named power mode on the selected generic Cortex-M target.
///
/// # Errors
///
/// Returns an error because the generic fallback cannot honestly enter board-specific modes.
pub fn enter_power_mode(name: &str) -> Result<(), HardwareError> {
    board_contract::enter_power_mode(system_soc(), name)
}

/// Returns the selected generic Cortex-M flash-region descriptors.
#[must_use]
pub fn flash_regions() -> &'static [CortexMFlashRegionDescriptor] {
    board_contract::flash_regions(system_soc())
}

/// Returns the selected generic Cortex-M programmable-IO support surface.
#[must_use]
pub const fn pcu_support() -> PcuSupport {
    PcuSupport::unsupported()
}

/// Returns the selected generic Cortex-M programmable-IO engine descriptors.
#[must_use]
pub fn pcu_engines() -> &'static [PcuEngineDescriptor] {
    &[]
}

/// Returns the selected generic Cortex-M programmable-IO lane descriptors for one engine.
#[must_use]
pub fn pcu_lanes(_engine: PcuEngineId) -> &'static [PcuLaneDescriptor] {
    &[]
}

/// Claims one programmable-IO engine on the selected generic Cortex-M target.
pub fn claim_pcu_engine(_engine: PcuEngineId) -> Result<PcuEngineClaim, PcuError> {
    Err(PcuError::unsupported())
}

/// Releases one programmable-IO engine claim on the selected generic Cortex-M target.
pub fn release_pcu_engine(_claim: PcuEngineClaim) -> Result<(), PcuError> {
    Err(PcuError::unsupported())
}

/// Claims one programmable-IO lane set on the selected generic Cortex-M target.
pub fn claim_pcu_lanes(
    _engine: PcuEngineId,
    _lanes: PcuLaneMask,
) -> Result<PcuLaneClaim, PcuError> {
    Err(PcuError::unsupported())
}

/// Releases one programmable-IO lane claim on the selected generic Cortex-M target.
pub fn release_pcu_lanes(_claim: PcuLaneClaim) -> Result<(), PcuError> {
    Err(PcuError::unsupported())
}

/// Loads one programmable-IO program image on the selected generic Cortex-M target.
pub fn load_pcu_program(
    _claim: &PcuEngineClaim,
    _image: &PcuProgramImage<'_>,
) -> Result<PcuProgramLease, PcuError> {
    Err(PcuError::unsupported())
}

/// Unloads one programmable-IO program image on the selected generic Cortex-M target.
pub fn unload_pcu_program(
    _claim: &PcuEngineClaim,
    _lease: PcuProgramLease,
) -> Result<(), PcuError> {
    Err(PcuError::unsupported())
}

/// Starts one programmable-IO lane set on the selected generic Cortex-M target.
pub fn start_pcu_lanes(_claim: &PcuLaneClaim) -> Result<(), PcuError> {
    Err(PcuError::unsupported())
}

/// Stops one programmable-IO lane set on the selected generic Cortex-M target.
pub fn stop_pcu_lanes(_claim: &PcuLaneClaim) -> Result<(), PcuError> {
    Err(PcuError::unsupported())
}

/// Restarts one programmable-IO lane set on the selected generic Cortex-M target.
pub fn restart_pcu_lanes(_claim: &PcuLaneClaim) -> Result<(), PcuError> {
    Err(PcuError::unsupported())
}

/// Writes one word to one programmable-IO TX FIFO on the selected generic Cortex-M target.
pub fn write_pcu_tx_fifo(
    _claim: &PcuLaneClaim,
    _lane: PcuLaneId,
    _word: u32,
) -> Result<(), PcuError> {
    Err(PcuError::unsupported())
}

/// Reads one word from one programmable-IO RX FIFO on the selected generic Cortex-M target.
pub fn read_pcu_rx_fifo(_claim: &PcuLaneClaim, _lane: PcuLaneId) -> Result<u32, PcuError> {
    Err(PcuError::unsupported())
}

/// Applies one programmable-IO execution-state bundle on the selected generic Cortex-M target.
pub fn apply_pcu_execution_config(
    _claim: &PcuLaneClaim,
    _clkdiv: u32,
    _execctrl: u32,
    _shiftctrl: u32,
    _pinctrl: u32,
) -> Result<(), PcuError> {
    Err(PcuError::unsupported())
}
