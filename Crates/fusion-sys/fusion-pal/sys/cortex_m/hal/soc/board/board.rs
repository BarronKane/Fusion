#![allow(clippy::doc_markdown)]

//! Cortex-M SoC board contract and generic helpers.

use crate::pal::hal::{
    HardwareAuthoritySet,
    HardwareError,
    HardwareGuarantee,
    HardwareTopologyCaps,
    HardwareTopologySummary,
    HardwareTopologySupport,
    HardwareWriteSummary,
};
use crate::pal::mem::{CachePolicy, MemResourceBackingKind, Protect, RegionAttrs};
use crate::pal::thread::{
    ThreadAuthoritySet,
    ThreadCoreId,
    ThreadError,
    ThreadExecutionLocation,
    ThreadId,
    ThreadLogicalCpuId,
    ThreadProcessorGroupId,
};
use core::time::Duration;

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

/// Runtime per-device identity surface available from the selected Cortex-M board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CortexMSocDeviceIdSupport {
    /// No truthful runtime per-device identity path is available.
    Unsupported,
    /// The board can surface a stable device identity through firmware or ROM services.
    FirmwareReadable,
    /// The board can surface a stable device identity through OTP or other non-volatile storage.
    OtpReadable,
    /// The board can surface a stable device identity through a memory-mapped register block.
    RegisterReadable,
    /// The board can surface a stable device identity through external flash or other board-local
    /// non-volatile storage.
    BoardStorageReadable,
}

/// Opaque per-device identity payload surfaced by the selected Cortex-M board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CortexMSocDeviceIdentity {
    /// Opaque identity bytes in board-defined order.
    pub bytes: [u8; 16],
    /// Number of meaningful bytes in `bytes`.
    pub len: u8,
    /// Whether the surfaced identifier is intentionally public rather than access-restricted.
    pub public: bool,
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

/// Coarse class for a board-visible Cortex-M IRQ line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CortexMIrqClass {
    /// Timer compare or alarm interrupt output.
    Timer,
    /// PWM wrap or pacing interrupt output.
    Pwm,
    /// DMA completion or channel-group interrupt output.
    Dma,
    /// USB controller interrupt output.
    Usb,
    /// PIO state-machine interrupt output.
    Pio,
    /// GPIO bank or IO interrupt output.
    Gpio,
    /// SIO-local FIFO, bell, or timer interrupt output.
    Sio,
    /// Clock or oscillator interrupt output.
    Clock,
    /// SPI controller interrupt output.
    Spi,
    /// UART controller interrupt output.
    Uart,
    /// ADC interrupt output.
    Adc,
    /// I2C controller interrupt output.
    I2c,
    /// OTP controller interrupt output.
    Otp,
    /// TRNG interrupt output.
    Trng,
    /// Core trace or CTI interrupt output.
    CoreTrace,
    /// PLL interrupt output.
    Pll,
    /// Power-management interrupt output.
    Power,
    /// Reserved or spare interrupt slot.
    Spare,
}

/// Static IRQ descriptor surfaced by a Cortex-M SoC board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CortexMIrqDescriptor {
    /// Human-readable IRQ line name.
    pub name: &'static str,
    /// NVIC external interrupt number.
    pub irqn: u16,
    /// Peripheral or block associated with this IRQ line when one exists.
    pub peripheral: Option<&'static str>,
    /// Coarse IRQ classification.
    pub class: CortexMIrqClass,
    /// Peripheral-local endpoint selector when the line is one of several outputs.
    pub endpoint: Option<&'static str>,
    /// Whether this line belongs to a non-secure view of the peripheral block.
    pub nonsecure: bool,
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

bitflags::bitflags! {
    /// Supported DMA transfer shapes surfaced by a Cortex-M SoC board.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct CortexMDmaTransferCaps: u32 {
        /// DMA can copy between ordinary memory endpoints.
        const MEMORY_TO_MEMORY     = 1 << 0;
        /// DMA can copy from memory to one peripheral endpoint.
        const MEMORY_TO_PERIPHERAL = 1 << 1;
        /// DMA can copy from one peripheral endpoint to memory.
        const PERIPHERAL_TO_MEMORY = 1 << 2;
        /// DMA can chain or trigger one channel from another.
        const CHANNEL_CHAINING     = 1 << 3;
    }
}

/// Static DMA controller descriptor surfaced by a Cortex-M SoC board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CortexMDmaControllerDescriptor {
    /// Human-readable DMA controller name.
    pub name: &'static str,
    /// Base address of the controller register block.
    pub base: usize,
    /// Number of hardware channels exposed by the controller.
    pub channel_count: u8,
    /// Coarse transfer capabilities supported by the controller.
    pub transfer_caps: CortexMDmaTransferCaps,
}

/// Static DMA request descriptor surfaced by a Cortex-M SoC board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CortexMDmaRequestClass {
    /// Peripheral transmit-side pacing or drain request.
    PeripheralTx,
    /// Peripheral receive-side pacing or fill request.
    PeripheralRx,
    /// Peripheral-generated pacing request that is not a plain TX/RX FIFO endpoint.
    PeripheralPacer,
    /// DMA timer pacing source.
    TimerPacer,
    /// Unconditional software-force request.
    Force,
}

/// Static DMA request descriptor surfaced by a Cortex-M SoC board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CortexMDmaRequestDescriptor {
    /// Human-readable request-line name.
    pub name: &'static str,
    /// Board-defined request-line selector.
    pub request_line: u16,
    /// Peripheral associated with the request line when one exists.
    pub peripheral: Option<&'static str>,
    /// Coarse request classification for routing and pacing semantics.
    pub class: CortexMDmaRequestClass,
    /// Peripheral-local endpoint selector when one exists.
    pub endpoint: Option<&'static str>,
    /// Coarse transfer capabilities supported by this request line.
    pub transfer_caps: CortexMDmaTransferCaps,
}

/// Static sleep/power-mode descriptor surfaced by a Cortex-M SoC board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CortexMPowerModeDescriptor {
    /// Human-readable power-mode name.
    pub name: &'static str,
    /// Whether the mode is entered through `WFI`.
    pub uses_wfi: bool,
    /// Whether the mode is entered through `WFE`.
    pub uses_wfe: bool,
    /// Whether the mode asserts the architectural deep-sleep bit.
    pub deep_sleep: bool,
    /// Coarse wake sources supported by this mode.
    pub wake_sources: &'static [&'static str],
    /// Coarse clock domains or sinks typically gated in this mode.
    pub gated_domains: &'static [&'static str],
}

/// Static flash/XIP region descriptor surfaced by a Cortex-M SoC board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CortexMFlashRegionDescriptor {
    /// Human-readable flash region name.
    pub name: &'static str,
    /// Base address of the flash or XIP-backed region.
    pub base: usize,
    /// Length of the represented region in bytes.
    pub len: usize,
    /// Erase block size in bytes when the region is erasable.
    pub erase_block_bytes: usize,
    /// Minimum programming granule in bytes when the region is writable.
    pub program_granule_bytes: usize,
    /// Whether the region is visible through an execute-in-place alias.
    pub xip: bool,
    /// Whether the region can be programmed at runtime through a board-defined path.
    pub writable: bool,
    /// Whether writes require quiescing or remapping the active XIP path.
    pub requires_xip_quiesce: bool,
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

    /// Returns the runtime per-device identity support class for this board.
    #[must_use]
    fn device_identity_support(&self) -> CortexMSocDeviceIdSupport {
        CortexMSocDeviceIdSupport::Unsupported
    }

    /// Returns the current per-device identity when this board can surface it honestly.
    ///
    /// # Errors
    ///
    /// Returns an error if the selected board cannot surface a truthful device identity.
    fn device_identity(&self) -> Result<CortexMSocDeviceIdentity, HardwareError> {
        Err(HardwareError::unsupported())
    }

    /// Returns whether a local interrupt-masked critical section is sufficient to serialize
    /// local synchronization on this board.
    ///
    /// Boards should return `true` only when:
    /// - execution is effectively single-core for the surfaced runtime contract, and
    /// - local interrupt masking is enough to exclude all competing execution contexts that may
    ///   touch these primitives.
    ///
    /// Multi-core Cortex-M boards such as RP2350 must keep this `false`, because masking
    /// interrupts on one core does not stop the other core from wandering in and ruining the day.
    #[must_use]
    fn local_critical_section_sync_safe(&self) -> bool {
        false
    }

    /// Returns the static memory-region descriptors surfaced by this SoC board.
    #[must_use]
    fn memory_map(&self) -> &'static [CortexMMemoryRegionDescriptor] {
        &[]
    }

    /// Returns the number of board-owned runtime memory regions surfaced in addition to the
    /// static SoC memory map.
    ///
    /// These regions are intended for linker- or board-defined free-memory carveouts such as
    /// allocator-owned SRAM windows. They may overlap coarse SoC apertures in `memory_map()`,
    /// but carry a more precise ownership contract.
    #[must_use]
    fn owned_memory_region_count(&self) -> usize {
        0
    }

    /// Returns one board-owned runtime memory region surfaced in addition to the static SoC
    /// memory map.
    #[must_use]
    fn owned_memory_region(&self, _index: usize) -> Option<CortexMMemoryRegionDescriptor> {
        None
    }

    /// Returns the named peripheral blocks surfaced by this SoC board.
    #[must_use]
    fn peripherals(&self) -> &'static [CortexMPeripheralDescriptor] {
        &[]
    }

    /// Returns the named IRQ lines surfaced by this SoC board.
    #[must_use]
    fn irqs(&self) -> &'static [CortexMIrqDescriptor] {
        &[]
    }

    /// Enables one named external IRQ line for this board.
    ///
    /// # Errors
    ///
    /// Returns an error if the IRQ line is unknown or cannot be enabled honestly.
    fn irq_enable(&self, _irqn: u16) -> Result<(), HardwareError> {
        Err(HardwareError::unsupported())
    }

    /// Disables one named external IRQ line for this board.
    ///
    /// # Errors
    ///
    /// Returns an error if the IRQ line is unknown or cannot be disabled honestly.
    fn irq_disable(&self, _irqn: u16) -> Result<(), HardwareError> {
        Err(HardwareError::unsupported())
    }

    /// Returns whether one IRQ line can be acknowledged generically by this board contract.
    ///
    /// Generic acknowledgement is only appropriate when the board can clear the surfaced source
    /// honestly without additional driver-local register context.
    #[must_use]
    fn irq_acknowledge_supported(&self, _irqn: u16) -> bool {
        false
    }

    /// Acknowledges one IRQ line surfaced by this board.
    ///
    /// # Errors
    ///
    /// Returns an error if the IRQ line cannot be acknowledged generically by this board
    /// contract.
    fn irq_acknowledge(&self, _irqn: u16) -> Result<(), HardwareError> {
        Err(HardwareError::unsupported())
    }

    /// Returns whether this board exposes one truthful finite-timeout event source for the
    /// shared Cortex-M event backend.
    #[must_use]
    fn event_timeout_supported(&self) -> bool {
        false
    }

    /// Returns the board-reserved IRQ line used by the shared Cortex-M event timeout source, when
    /// one exists.
    #[must_use]
    fn event_timeout_irq(&self) -> Option<u16> {
        None
    }

    /// Arms the board-defined event timeout source.
    ///
    /// # Errors
    ///
    /// Returns an error if the board does not expose a truthful finite-timeout event source.
    fn arm_event_timeout(&self, _timeout: Duration) -> Result<(), HardwareError> {
        Err(HardwareError::unsupported())
    }

    /// Cancels the board-defined event timeout source.
    ///
    /// # Errors
    ///
    /// Returns an error if the board does not expose a truthful finite-timeout event source.
    fn cancel_event_timeout(&self) -> Result<(), HardwareError> {
        Err(HardwareError::unsupported())
    }

    /// Returns whether the board-defined event timeout source has fired.
    ///
    /// # Errors
    ///
    /// Returns an error if the board does not expose a truthful finite-timeout event source.
    fn event_timeout_fired(&self) -> Result<bool, HardwareError> {
        Err(HardwareError::unsupported())
    }

    /// Returns the major board-visible clock descriptors surfaced by this SoC board.
    #[must_use]
    fn clock_tree(&self) -> &'static [CortexMClockDescriptor] {
        &[]
    }

    /// Returns DMA controller descriptors surfaced by this SoC board.
    #[must_use]
    fn dma_controllers(&self) -> &'static [CortexMDmaControllerDescriptor] {
        &[]
    }

    /// Returns DMA request-line descriptors surfaced by this SoC board.
    #[must_use]
    fn dma_requests(&self) -> &'static [CortexMDmaRequestDescriptor] {
        &[]
    }

    /// Returns power/sleep mode descriptors surfaced by this SoC board.
    #[must_use]
    fn power_modes(&self) -> &'static [CortexMPowerModeDescriptor] {
        &[]
    }

    /// Returns the selected board's generic PAL-facing power descriptors.
    #[must_use]
    fn pal_power_modes(&self) -> &'static [crate::pal::power::PowerModeDescriptor] {
        &[]
    }

    /// Enters one named power mode surfaced by this SoC board.
    ///
    /// # Errors
    ///
    /// Returns an error if the mode name is unknown for this board or the board cannot enter
    /// the requested mode honestly.
    fn enter_power_mode(&self, _name: &str) -> Result<(), HardwareError> {
        Err(HardwareError::unsupported())
    }

    /// Returns flash/XIP region descriptors surfaced by this SoC board.
    #[must_use]
    fn flash_regions(&self) -> &'static [CortexMFlashRegionDescriptor] {
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

/// Returns the runtime per-device identity support class for the selected board.
#[must_use]
pub fn selected_soc_device_id_support<T: CortexMSocBoard>(soc: T) -> CortexMSocDeviceIdSupport {
    soc.device_identity_support()
}

/// Returns the runtime per-device identity for the selected board.
///
/// # Errors
///
/// Returns an error if the selected board cannot surface a truthful device identity.
pub fn device_identity<T: CortexMSocBoard>(
    soc: T,
) -> Result<CortexMSocDeviceIdentity, HardwareError> {
    soc.device_identity()
}

/// Returns whether a local interrupt-masked critical section is sufficient to serialize local
/// synchronization on the selected SoC board.
#[must_use]
pub fn local_critical_section_sync_safe<T: CortexMSocBoard>(soc: T) -> bool {
    soc.local_critical_section_sync_safe()
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

/// Returns the number of board-owned runtime memory regions for the selected SoC board.
#[must_use]
pub fn owned_memory_region_count<T: CortexMSocBoard>(soc: T) -> usize {
    soc.owned_memory_region_count()
}

/// Returns one board-owned runtime memory region for the selected SoC board.
#[must_use]
pub fn owned_memory_region<T: CortexMSocBoard>(
    soc: T,
    index: usize,
) -> Option<CortexMMemoryRegionDescriptor> {
    soc.owned_memory_region(index)
}

/// Returns the named peripheral blocks for the selected SoC board.
#[must_use]
pub fn peripherals<T: CortexMSocBoard>(soc: T) -> &'static [CortexMPeripheralDescriptor] {
    soc.peripherals()
}

/// Returns the named IRQ lines for the selected SoC board.
#[must_use]
pub fn irqs<T: CortexMSocBoard>(soc: T) -> &'static [CortexMIrqDescriptor] {
    soc.irqs()
}

/// Enables one named external IRQ line for the selected SoC board.
///
/// # Errors
///
/// Returns an error if the selected board cannot enable the requested IRQ honestly.
pub fn irq_enable<T: CortexMSocBoard>(soc: T, irqn: u16) -> Result<(), HardwareError> {
    soc.irq_enable(irqn)
}

/// Disables one named external IRQ line for the selected SoC board.
///
/// # Errors
///
/// Returns an error if the selected board cannot disable the requested IRQ honestly.
pub fn irq_disable<T: CortexMSocBoard>(soc: T, irqn: u16) -> Result<(), HardwareError> {
    soc.irq_disable(irqn)
}

/// Returns whether one IRQ line can be acknowledged generically by the selected SoC board.
#[must_use]
pub fn irq_acknowledge_supported<T: CortexMSocBoard>(soc: T, irqn: u16) -> bool {
    soc.irq_acknowledge_supported(irqn)
}

/// Acknowledges one IRQ line surfaced by the selected SoC board.
///
/// # Errors
///
/// Returns an error if the selected board cannot acknowledge the requested IRQ honestly.
pub fn irq_acknowledge<T: CortexMSocBoard>(soc: T, irqn: u16) -> Result<(), HardwareError> {
    soc.irq_acknowledge(irqn)
}

/// Returns whether the selected SoC board exposes one truthful finite-timeout event source.
#[must_use]
pub fn event_timeout_supported<T: CortexMSocBoard>(soc: T) -> bool {
    soc.event_timeout_supported()
}

/// Returns the board-reserved IRQ line used by the selected SoC board's event timeout source.
#[must_use]
pub fn event_timeout_irq<T: CortexMSocBoard>(soc: T) -> Option<u16> {
    soc.event_timeout_irq()
}

/// Arms the selected SoC board's event-timeout source.
///
/// # Errors
///
/// Returns an error if the selected board cannot surface finite event timeouts honestly.
pub fn arm_event_timeout<T: CortexMSocBoard>(
    soc: T,
    timeout: Duration,
) -> Result<(), HardwareError> {
    soc.arm_event_timeout(timeout)
}

/// Cancels the selected SoC board's event-timeout source.
///
/// # Errors
///
/// Returns an error if the selected board cannot surface finite event timeouts honestly.
pub fn cancel_event_timeout<T: CortexMSocBoard>(soc: T) -> Result<(), HardwareError> {
    soc.cancel_event_timeout()
}

/// Returns whether the selected SoC board's event-timeout source has fired.
///
/// # Errors
///
/// Returns an error if the selected board cannot surface finite event timeouts honestly.
pub fn event_timeout_fired<T: CortexMSocBoard>(soc: T) -> Result<bool, HardwareError> {
    soc.event_timeout_fired()
}

/// Returns the major clock descriptors for the selected SoC board.
#[must_use]
pub fn clock_tree<T: CortexMSocBoard>(soc: T) -> &'static [CortexMClockDescriptor] {
    soc.clock_tree()
}

/// Returns the DMA controller descriptors for the selected SoC board.
#[must_use]
pub fn dma_controllers<T: CortexMSocBoard>(soc: T) -> &'static [CortexMDmaControllerDescriptor] {
    soc.dma_controllers()
}

/// Returns the DMA request-line descriptors for the selected SoC board.
#[must_use]
pub fn dma_requests<T: CortexMSocBoard>(soc: T) -> &'static [CortexMDmaRequestDescriptor] {
    soc.dma_requests()
}

/// Returns the power/sleep mode descriptors for the selected SoC board.
#[must_use]
pub fn power_modes<T: CortexMSocBoard>(soc: T) -> &'static [CortexMPowerModeDescriptor] {
    soc.power_modes()
}

/// Returns the selected board's generic PAL-facing power descriptors.
#[must_use]
pub fn pal_power_modes<T: CortexMSocBoard>(
    soc: T,
) -> &'static [crate::pal::power::PowerModeDescriptor] {
    soc.pal_power_modes()
}

/// Enters one named power mode on the selected SoC board.
///
/// # Errors
///
/// Returns an error if the selected SoC board cannot enter the named mode honestly.
pub fn enter_power_mode<T: CortexMSocBoard>(soc: T, name: &str) -> Result<(), HardwareError> {
    soc.enter_power_mode(name)
}

/// Returns the flash/XIP region descriptors for the selected SoC board.
#[must_use]
pub fn flash_regions<T: CortexMSocBoard>(soc: T) -> &'static [CortexMFlashRegionDescriptor] {
    soc.flash_regions()
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
