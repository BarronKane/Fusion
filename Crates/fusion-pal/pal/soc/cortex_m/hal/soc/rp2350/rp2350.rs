#![allow(clippy::doc_markdown)]

//! RP2350 Cortex-M SoC descriptor.
//!
//! This module is where verified RP2350 memory-map, peripheral, and clock-tree facts belong.
//! The current implementation wires the architected topology, the major static memory regions,
//! the major peripheral blocks, and the board-visible clock domains from the RP2350 datasheet
//! and Pico SDK clock model.

use core::arch::asm;
use core::ptr;
use core::sync::atomic::{
    AtomicBool,
    AtomicU8,
    Ordering,
    compiler_fence,
};
use core::time::Duration;

use crate::contract::pal::mem::MemTopologyNodeId;
use crate::contract::pal::mem::{
    CachePolicy,
    MemResourceBackingKind,
    Protect,
    RegionAttrs,
};
use crate::contract::pal::power::{
    PowerModeDepth,
    PowerModeDescriptor,
};
use crate::contract::pal::runtime::thread::{
    ThreadAuthoritySet,
    ThreadClusterId,
    ThreadCoreClassId,
    ThreadCoreId,
    ThreadError,
    ThreadExecutionLocation,
    ThreadId,
    ThreadLogicalCpuId,
    ThreadProcessorGroupId,
};
use crate::contract::pal::{
    HardwareAuthoritySet,
    HardwareError,
    HardwareTopologySummary,
    HardwareWriteSummary,
};
pub use super::board_contract::{
    CortexMClockDescriptor,
    CortexMDmaControllerDescriptor,
    CortexMDmaRequestClass,
    CortexMDmaRequestDescriptor,
    CortexMDmaTransferCaps,
    CortexMEventTimeoutImplementation,
    CortexMEventTimeoutSupport,
    CortexMExceptionStackObservation,
    CortexMFlashRegionDescriptor,
    CortexMIrqClass,
    CortexMIrqDescriptor,
    CortexMMemoryRegionDescriptor,
    CortexMMemoryRegionKind,
    CortexMPeripheralBus,
    CortexMPeripheralDescriptor,
    CortexMPowerModeDescriptor,
    CortexMSocBoard as CortexMSoc,
    CortexMSocChipIdSupport,
    CortexMSocChipIdentity,
    CortexMSocDescriptor,
    CortexMSocDeviceIdSupport,
    CortexMSocDeviceIdentity,
    CortexMSocExecutionObservation,
    CortexMSocMonotonicTimeImpact,
    CortexMSocOverclockProfile,
    CortexMSocOverclockSupport,
};
use super::board_contract::{
    self,
    CortexMSocBoard,
};
use super::pio::{
    PioCaps as PcuCaps,
    PioClockDescriptor as PcuClockDescriptor,
    PioEngineClaim as PcuEngineClaim,
    PioEngineDescriptor as PcuEngineDescriptor,
    PioEngineId as PcuEngineId,
    PioError as PcuError,
    PioFifoDescriptor as PcuFifoDescriptor,
    PioFifoDirection as PcuFifoDirection,
    PioFifoId as PcuFifoId,
    PioImplementationKind as PcuImplementationKind,
    PioInstructionMemoryDescriptor as PcuInstructionMemoryDescriptor,
    PioLaneClaim as PcuLaneClaim,
    PioLaneDescriptor as PcuLaneDescriptor,
    PioLaneId as PcuLaneId,
    PioLaneMask as PcuLaneMask,
    PioPinMappingCaps as PcuPinMappingCaps,
    PioProgramImage as PcuProgramImage,
    PioProgramLease as PcuProgramLease,
    PioSupport as PcuSupport,
};

mod descriptors;
pub use descriptors::*;

/// RP2350 SoC provider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rp2350Soc;

/// Raw RP2350 GPIO-summary snapshot for one IO-bank IRQ line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rp2350GpioIrqSummary {
    word_count: u8,
    words: [u32; RP2350_GPIO_BANK0_SUMMARY_WORDS],
}

impl Rp2350GpioIrqSummary {
    /// Returns the number of summary words that are valid for this bank.
    #[must_use]
    pub const fn word_count(self) -> usize {
        self.word_count as usize
    }

    /// Returns one raw summary word when it exists.
    #[must_use]
    pub const fn word(self, index: usize) -> Option<u32> {
        if index < self.word_count as usize {
            Some(self.words[index])
        } else {
            None
        }
    }

    /// Returns the raw 4-bit event nibble for one bank-local GPIO line.
    #[must_use]
    pub const fn line_events(self, line_index: u8) -> Option<u8> {
        let word_index = (line_index / 8) as usize;
        if word_index >= self.word_count as usize {
            return None;
        }
        let shift = ((line_index % 8) * 4) as u32;
        Some(((self.words[word_index] >> shift) & 0x0f) as u8)
    }
}

/// Raw RP2350 PIO-summary snapshot for one PIO IRQ line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rp2350PioIrqSummary {
    raw: u16,
}

impl Rp2350PioIrqSummary {
    /// Returns the raw summary bits as surfaced by `PIO_IRQx_INTS`.
    #[must_use]
    pub const fn raw(self) -> u16 {
        self.raw
    }

    /// Returns the internal PIO IRQ flags that can be cleared through `PIO_IRQ`.
    #[must_use]
    pub const fn internal_irq_flags(self) -> u8 {
        (self.raw >> 8) as u8
    }

    /// Returns the state-machine TX-not-full summary bits.
    #[must_use]
    pub const fn tx_not_full_mask(self) -> u8 {
        ((self.raw >> 4) & 0x0f) as u8
    }

    /// Returns the state-machine RX-not-empty summary bits.
    #[must_use]
    pub const fn rx_not_empty_mask(self) -> u8 {
        (self.raw & 0x0f) as u8
    }
}

/// Raw RP2350 SPI-summary snapshot for one SPI IRQ line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rp2350SpiIrqSummary {
    raw: u8,
}

impl Rp2350SpiIrqSummary {
    /// Returns the raw masked interrupt summary bits from `SPI_SSPMIS`.
    #[must_use]
    pub const fn raw(self) -> u8 {
        self.raw
    }

    /// Returns whether TX threshold readiness is asserted.
    #[must_use]
    pub const fn tx(self) -> bool {
        (self.raw & 0x8) != 0
    }

    /// Returns whether RX threshold readiness is asserted.
    #[must_use]
    pub const fn rx(self) -> bool {
        (self.raw & 0x4) != 0
    }

    /// Returns whether receive timeout is asserted.
    #[must_use]
    pub const fn receive_timeout(self) -> bool {
        (self.raw & 0x2) != 0
    }

    /// Returns whether receive overrun is asserted.
    #[must_use]
    pub const fn receive_overrun(self) -> bool {
        (self.raw & 0x1) != 0
    }

    /// Returns the subset of pending causes that the shared SPI clear register can acknowledge.
    #[must_use]
    pub const fn clearable_mask(self) -> u8 {
        self.raw & 0x3
    }
}

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

const fn rp2350_chip_revision(raw_chip_id: u32) -> u8 {
    ((raw_chip_id >> 28) & 0x0f) as u8
}

const fn rp2350_chip_part(raw_chip_id: u32) -> u16 {
    ((raw_chip_id >> 12) & 0xffff) as u16
}

const fn rp2350_chip_manufacturer(raw_chip_id: u32) -> u16 {
    ((raw_chip_id >> 1) & 0x07ff) as u16
}

fn rp2350_chip_identity() -> CortexMSocChipIdentity {
    let raw_chip_id = unsafe { ptr::read_volatile(RP2350_SYSINFO_CHIP_ID) };
    let platform = unsafe { ptr::read_volatile(RP2350_SYSINFO_PLATFORM) };
    let source_revision = unsafe { ptr::read_volatile(RP2350_SYSINFO_GITREF_RP2350) };
    let chip_info = unsafe { ptr::read_volatile(RP2350_SYSINFO_CHIP_INFO) };

    CortexMSocChipIdentity {
        raw_chip_id,
        revision: Some(rp2350_chip_revision(raw_chip_id)),
        part: Some(rp2350_chip_part(raw_chip_id)),
        manufacturer: Some(rp2350_chip_manufacturer(raw_chip_id)),
        package: Some(chip_info & 0x1),
        platform: Some(platform),
        source_revision: Some(source_revision),
    }
}

const fn rp2350_public_device_id_from_words(words: [u16; 4]) -> u64 {
    (words[0] as u64)
        | ((words[1] as u64) << 16)
        | ((words[2] as u64) << 32)
        | ((words[3] as u64) << 48)
}

fn rp2350_read_otp_data_word(offset: usize) -> u16 {
    // SAFETY: OTP_DATA is a fixed memory-mapped APB window. The RP2350 datasheet defines
    // CHIPID0..3 at offsets 0x000..0x003 inside this word-addressed window; reading the lower
    // 16 bits of each row surfaces the public 64-bit device identifier.
    unsafe { (ptr::read_volatile(RP2350_OTP_DATA.add(offset)) & 0xffff) as u16 }
}

fn rp2350_device_identity() -> CortexMSocDeviceIdentity {
    let public_device_id = rp2350_public_device_id_from_words([
        rp2350_read_otp_data_word(0),
        rp2350_read_otp_data_word(1),
        rp2350_read_otp_data_word(2),
        rp2350_read_otp_data_word(3),
    ]);

    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(&public_device_id.to_le_bytes());

    CortexMSocDeviceIdentity {
        bytes,
        len: 8,
        public: true,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rp2350PowerModeAction {
    SleepWfi,
    DeepSleepWfi,
}

fn rp2350_power_mode_action(name: &str) -> Option<Rp2350PowerModeAction> {
    match name {
        "sleep-wfi" => Some(Rp2350PowerModeAction::SleepWfi),
        "deep-sleep-wfi" => Some(Rp2350PowerModeAction::DeepSleepWfi),
        _ => None,
    }
}

fn rp2350_irq_is_known(irqn: u16) -> bool {
    IRQS.iter().any(|descriptor| descriptor.irqn == irqn)
}

fn rp2350_timer_base_and_alarm_bit(irqn: u16) -> Option<(usize, u32)> {
    match irqn {
        0..=3 => Some((RP2350_TIMER0_BASE, 1_u32 << u32::from(irqn))),
        4..=7 => Some((RP2350_TIMER1_BASE, 1_u32 << u32::from(irqn - 4))),
        _ => None,
    }
}

const fn rp2350_dma_irq_group(irqn: u16) -> Option<usize> {
    match irqn {
        10 => Some(0),
        11 => Some(1),
        12 => Some(2),
        13 => Some(3),
        _ => None,
    }
}

const fn rp2350_uart_base(irqn: u16) -> Option<usize> {
    match irqn {
        33 => Some(RP2350_UART0_BASE),
        34 => Some(RP2350_UART1_BASE),
        _ => None,
    }
}

const fn rp2350_i2c_base(irqn: u16) -> Option<usize> {
    match irqn {
        36 => Some(RP2350_I2C0_BASE),
        37 => Some(RP2350_I2C1_BASE),
        _ => None,
    }
}

const fn rp2350_spi_base(irqn: u16) -> Option<usize> {
    match irqn {
        31 => Some(RP2350_SPI0_BASE),
        32 => Some(RP2350_SPI1_BASE),
        _ => None,
    }
}

const fn rp2350_pio_base_and_irq_index(irqn: u16) -> Option<(usize, usize)> {
    match irqn {
        15 => Some((RP2350_PIO0_BASE, 0)),
        16 => Some((RP2350_PIO0_BASE, 1)),
        17 => Some((RP2350_PIO1_BASE, 0)),
        18 => Some((RP2350_PIO1_BASE, 1)),
        19 => Some((RP2350_PIO2_BASE, 0)),
        20 => Some((RP2350_PIO2_BASE, 1)),
        _ => None,
    }
}

const fn rp2350_gpio_irq_bank(irqn: u16) -> Option<(usize, usize, usize)> {
    match irqn {
        21 | 22 => Some((
            RP2350_IO_BANK0_BASE,
            RP2350_IO_BANK0_INTR0_OFFSET,
            RP2350_GPIO_BANK0_SUMMARY_WORDS,
        )),
        23 | 24 => Some((
            RP2350_IO_QSPI_BASE,
            RP2350_IO_QSPI_INTR_OFFSET,
            RP2350_GPIO_QSPI_SUMMARY_WORDS,
        )),
        _ => None,
    }
}

fn rp2350_gpio_irq_summary_snapshot(irqn: u16) -> Result<Rp2350GpioIrqSummary, HardwareError> {
    let Some((base, intr0_offset, word_count)) = rp2350_gpio_irq_bank(irqn) else {
        return Err(HardwareError::invalid());
    };

    let mut words = [0_u32; RP2350_GPIO_BANK0_SUMMARY_WORDS];
    for (index, word) in words.iter_mut().take(word_count).enumerate() {
        let register = (base + intr0_offset + (index * RP2350_IO_IRQ_WORD_STRIDE)) as *const u32;
        // SAFETY: these are fixed RP2350 IO-bank raw-interrupt summary registers. Reads are
        // side-effect free and simply snapshot the shared-summary state for driver-local handling.
        *word = unsafe { ptr::read_volatile(register) };
    }

    Ok(Rp2350GpioIrqSummary {
        word_count: word_count as u8,
        words,
    })
}

fn rp2350_gpio_irq_clear_edges(
    irqn: u16,
    word_index: usize,
    edge_mask: u32,
) -> Result<(), HardwareError> {
    let Some((base, intr0_offset, word_count)) = rp2350_gpio_irq_bank(irqn) else {
        return Err(HardwareError::invalid());
    };
    if word_index >= word_count {
        return Err(HardwareError::invalid());
    }

    let register = (base + intr0_offset + (word_index * RP2350_IO_IRQ_WORD_STRIDE)) as *mut u32;
    let clear_mask = edge_mask & RP2350_GPIO_EDGE_EVENT_MASK;
    // SAFETY: the RP2350 IO-bank `INTR` registers use write-clear semantics for edge bits only.
    // Masking to the architected edge subset avoids fabricating clears for level-triggered state.
    unsafe { ptr::write_volatile(register, clear_mask) };
    rp2350_nvic_write(CORTEX_M_NVIC_ICPR, irqn);
    Ok(())
}

fn rp2350_pio_irq_summary_snapshot(irqn: u16) -> Result<Rp2350PioIrqSummary, HardwareError> {
    let Some((base, irq_index)) = rp2350_pio_base_and_irq_index(irqn) else {
        return Err(HardwareError::invalid());
    };
    let offset = if irq_index == 0 {
        RP2350_PIO_IRQ0_INTS_OFFSET
    } else {
        RP2350_PIO_IRQ1_INTS_OFFSET
    };
    let register = (base + offset) as *const u32;
    // SAFETY: `PIO_IRQx_INTS` is the read-only processor-facing summary for one PIO block.
    let raw = unsafe { ptr::read_volatile(register) as u16 };
    Ok(Rp2350PioIrqSummary { raw })
}

fn rp2350_pio_irq_clear_internal_flags(irqn: u16, flags: u8) -> Result<(), HardwareError> {
    let Some((base, _)) = rp2350_pio_base_and_irq_index(irqn) else {
        return Err(HardwareError::invalid());
    };
    let register = (base + RP2350_PIO_IRQ_OFFSET) as *mut u32;
    // SAFETY: `PIO_IRQ` is the shared write-clear register for the PIO internal IRQ flags only.
    unsafe { ptr::write_volatile(register, u32::from(flags)) };
    rp2350_nvic_write(CORTEX_M_NVIC_ICPR, irqn);
    Ok(())
}

const fn rp2350_pio_base(engine: PcuEngineId) -> Option<usize> {
    match engine.0 {
        0 => Some(RP2350_PIO0_BASE),
        1 => Some(RP2350_PIO1_BASE),
        2 => Some(RP2350_PIO2_BASE),
        _ => None,
    }
}

const fn rp2350_pio_reset_bit(engine: PcuEngineId) -> Option<u32> {
    match engine.0 {
        0 => Some(RP2350_RESETS_PIO0_BIT),
        1 => Some(RP2350_RESETS_PIO1_BIT),
        2 => Some(RP2350_RESETS_PIO2_BIT),
        _ => None,
    }
}

fn rp2350_unreset_pio_engine(engine: PcuEngineId) -> Result<(), PcuError> {
    let bit = rp2350_pio_reset_bit(engine).ok_or_else(PcuError::invalid)?;
    let reset_register = RP2350_RESETS_BASE + RP2350_RESETS_RESET_OFFSET;
    let reset_done_register = (RP2350_RESETS_BASE + RP2350_RESETS_RESET_DONE_OFFSET) as *const u32;
    rp2350_atomic_register_clear(reset_register, bit);
    for _ in 0..1024 {
        // SAFETY: RESET_DONE is a read-only reset-controller summary register.
        let state = unsafe { ptr::read_volatile(reset_done_register) };
        if state & bit != 0 {
            return Ok(());
        }
        compiler_fence(Ordering::SeqCst);
    }
    Err(PcuError::busy())
}

const fn rp2350_pio_lane_descriptors(engine: PcuEngineId) -> &'static [PcuLaneDescriptor] {
    match engine.0 {
        0 => &PIO0_LANES,
        1 => &PIO1_LANES,
        2 => &PIO2_LANES,
        _ => &[],
    }
}

const fn rp2350_valid_lane_mask(mask: PcuLaneMask) -> bool {
    let bits = mask.bits();
    bits != 0 && (bits & !RP2350_PIO_VALID_LANE_MASK) == 0
}

fn rp2350_validate_engine_claim(claim: &PcuEngineClaim) -> Result<usize, PcuError> {
    let engine_index = usize::from(claim.engine().0);
    if engine_index >= RP2350_PIO_ENGINE_COUNT {
        return Err(PcuError::invalid());
    }
    if !RP2350_PIO_ENGINE_CLAIMS[engine_index].load(Ordering::Acquire) {
        return Err(PcuError::state_conflict());
    }
    Ok(engine_index)
}

fn rp2350_validate_lane_claim(claim: &PcuLaneClaim) -> Result<(usize, u8), PcuError> {
    let engine_index = usize::from(claim.engine().0);
    let bits = claim.lanes().bits();
    if engine_index >= RP2350_PIO_ENGINE_COUNT || !rp2350_valid_lane_mask(claim.lanes()) {
        return Err(PcuError::invalid());
    }
    let claimed = RP2350_PIO_LANE_CLAIMS[engine_index].load(Ordering::Acquire);
    if claimed & bits != bits {
        return Err(PcuError::state_conflict());
    }
    Ok((engine_index, bits))
}

fn rp2350_atomic_register_set(register: usize, bits: u32) {
    let alias = (register + RP2350_REG_ALIAS_SET_OFFSET) as *mut u32;
    // SAFETY: RP2350 APB atomic-set aliases update only the targeted register bits without a
    // read-modify-write race against other writers.
    unsafe { ptr::write_volatile(alias, bits) };
}

fn rp2350_atomic_register_clear(register: usize, bits: u32) {
    let alias = (register + RP2350_REG_ALIAS_CLR_OFFSET) as *mut u32;
    // SAFETY: RP2350 APB atomic-clear aliases update only the targeted register bits without a
    // read-modify-write race against other writers.
    unsafe { ptr::write_volatile(alias, bits) };
}

fn rp2350_spi_irq_summary_snapshot(irqn: u16) -> Result<Rp2350SpiIrqSummary, HardwareError> {
    let Some(base) = rp2350_spi_base(irqn) else {
        return Err(HardwareError::invalid());
    };
    let register = (base + RP2350_SPI_SSPMIS_OFFSET) as *const u32;
    // SAFETY: `SPI_SSPMIS` is the masked interrupt summary register for one SPI instance.
    let raw = unsafe { ptr::read_volatile(register) as u8 };
    Ok(Rp2350SpiIrqSummary { raw })
}

fn rp2350_spi_irq_acknowledge_clearable(irqn: u16) -> Result<u8, HardwareError> {
    let Some(base) = rp2350_spi_base(irqn) else {
        return Err(HardwareError::invalid());
    };
    let mis = (base + RP2350_SPI_SSPMIS_OFFSET) as *const u32;
    let icr = (base + RP2350_SPI_SSPICR_OFFSET) as *mut u32;
    // SAFETY: `SPI_SSPICR` is a write-clear register for RT/ROR causes only.
    let clear_mask = unsafe { (ptr::read_volatile(mis) & RP2350_SPI_SSPICR_CLEARABLE_MASK) as u8 };
    if clear_mask != 0 {
        unsafe { ptr::write_volatile(icr, u32::from(clear_mask)) };
        rp2350_nvic_write(CORTEX_M_NVIC_ICPR, irqn);
    }
    Ok(clear_mask)
}

fn rp2350_nvic_write(register_base: *mut u32, irqn: u16) {
    let register_index = usize::from(irqn / 32);
    let bit = u32::from(irqn % 32);
    // SAFETY: NVIC ISER/ICER/ICPR are architected Cortex-M register blocks. These writes affect
    // only the selected IRQ line and do not require aliasing any Rust-managed memory.
    unsafe { ptr::write_volatile(register_base.add(register_index), 1_u32 << bit) };
}

fn rp2350_irq_enable_line(irqn: u16) -> Result<(), HardwareError> {
    if !rp2350_irq_is_known(irqn) {
        return Err(HardwareError::invalid());
    }

    rp2350_nvic_write(CORTEX_M_NVIC_ISER, irqn);
    Ok(())
}

fn rp2350_irq_disable_line(irqn: u16) -> Result<(), HardwareError> {
    if !rp2350_irq_is_known(irqn) {
        return Err(HardwareError::invalid());
    }

    rp2350_nvic_write(CORTEX_M_NVIC_ICER, irqn);
    Ok(())
}

fn rp2350_irq_set_priority(irqn: u16, priority: u8) -> Result<(), HardwareError> {
    if !rp2350_irq_is_known(irqn) {
        return Err(HardwareError::invalid());
    }

    // SAFETY: NVIC IPR is the architected external-interrupt priority byte array. Each IRQ owns
    // one byte. Writing it affects only the selected line's raw priority field.
    unsafe { ptr::write_volatile(CORTEX_M_NVIC_IPR.add(usize::from(irqn)), priority) };
    Ok(())
}

fn rp2350_irq_clear_pending_line(irqn: u16) -> Result<(), HardwareError> {
    if !rp2350_irq_is_known(irqn) {
        return Err(HardwareError::invalid());
    }

    rp2350_nvic_write(CORTEX_M_NVIC_ICPR, irqn);
    Ok(())
}

fn rp2350_irq_set_pending_line(irqn: u16) -> Result<(), HardwareError> {
    if !rp2350_irq_is_known(irqn) {
        return Err(HardwareError::invalid());
    }

    rp2350_nvic_write(CORTEX_M_NVIC_ISPR, irqn);
    Ok(())
}

fn rp2350_irq_acknowledge_line(irqn: u16) -> Result<(), HardwareError> {
    if let Some((timer_base, alarm_bit)) = rp2350_timer_base_and_alarm_bit(irqn) {
        let intr = (timer_base + RP2350_TIMER_INTR_OFFSET) as *mut u32;
        // SAFETY: TIMERx_INTR is the timer raw-interrupt write-clear register for one RP2350 timer
        // block. Writing the selected alarm bit acknowledges that timer interrupt source.
        unsafe { ptr::write_volatile(intr, alarm_bit) };
        rp2350_nvic_write(CORTEX_M_NVIC_ICPR, irqn);
        return Ok(());
    }

    if let Some(group) = rp2350_dma_irq_group(irqn) {
        let ints = (RP2350_DMA_BASE + RP2350_DMA_INTS0_OFFSET + (group * 0x10)) as *mut u32;
        // SAFETY: DMA_INTSn is the RP2350 DMA masked interrupt-status register for one IRQ group.
        // It is write-clear, so writing back the currently asserted channel mask acknowledges the
        // surfaced DMA group without fabricating per-channel semantics the board contract does not
        // own.
        unsafe {
            let pending_mask = ptr::read_volatile(ints);
            if pending_mask != 0 {
                ptr::write_volatile(ints, pending_mask);
            }
        }
        rp2350_nvic_write(CORTEX_M_NVIC_ICPR, irqn);
        return Ok(());
    }

    if let Some(uart_base) = rp2350_uart_base(irqn) {
        let mis = (uart_base + RP2350_UARTMIS_OFFSET) as *const u32;
        let icr = (uart_base + RP2350_UARTICR_OFFSET) as *mut u32;
        // SAFETY: UARTMIS exposes the masked interrupt status for one RP2350 UART instance, and
        // UARTICR is the matching write-clear register. Writing back the asserted clearable bits
        // acknowledges exactly the generic UART interrupt causes this board contract can own.
        unsafe {
            let pending_mask = ptr::read_volatile(mis) & RP2350_UARTICR_CLEARABLE_BITS;
            if pending_mask != 0 {
                ptr::write_volatile(icr, pending_mask);
            }
        }
        rp2350_nvic_write(CORTEX_M_NVIC_ICPR, irqn);
        return Ok(());
    }

    if let Some(i2c_base) = rp2350_i2c_base(irqn) {
        let intr_stat = (i2c_base + RP2350_I2C_IC_INTR_STAT_OFFSET) as *const u32;
        let clr_intr = (i2c_base + RP2350_I2C_IC_CLR_INTR_OFFSET) as *const u32;
        // SAFETY: IC_INTR_STAT exposes the current masked interrupt causes for one RP2350 I2C
        // instance, and reading IC_CLR_INTR clears the board-visible latched causes without
        // inventing finer-grained semantics than the shared IRQ line actually has.
        unsafe {
            if ptr::read_volatile(intr_stat) != 0 {
                let _ = ptr::read_volatile(clr_intr);
            }
        }
        rp2350_nvic_write(CORTEX_M_NVIC_ICPR, irqn);
        return Ok(());
    }

    Err(HardwareError::unsupported())
}

fn rp2350_event_timeout_deadline(timeout: Duration) -> u32 {
    let micros = timeout.as_micros();
    let delta = u32::try_from(micros).unwrap_or(u32::MAX);
    let now = rp2350_monotonic_now_ticks() as u32;
    now.wrapping_add(delta.max(1))
}

fn rp2350_ensure_timer0_tick_started() {
    loop {
        match RP2350_TIMER0_TICK_STATE.load(Ordering::Acquire) {
            RP2350_TIMER0_TICK_STATE_READY => return,
            RP2350_TIMER0_TICK_STATE_INITIALIZING => core::hint::spin_loop(),
            RP2350_TIMER0_TICK_STATE_UNINITIALIZED => {
                if RP2350_TIMER0_TICK_STATE
                    .compare_exchange(
                        RP2350_TIMER0_TICK_STATE_UNINITIALIZED,
                        RP2350_TIMER0_TICK_STATE_INITIALIZING,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    let timer0_ctrl =
                        (RP2350_TICKS_BASE + RP2350_TICKS_TIMER0_CTRL_OFFSET) as *mut u32;
                    let timer0_cycles =
                        (RP2350_TICKS_BASE + RP2350_TICKS_TIMER0_CYCLES_OFFSET) as *mut u32;

                    unsafe {
                        ptr::write_volatile(timer0_ctrl, 0);
                        ptr::write_volatile(timer0_cycles, RP2350_TIMER_TICK_CYCLES);
                        ptr::write_volatile(timer0_ctrl, RP2350_TICKS_CTRL_ENABLE);
                    }

                    while unsafe { ptr::read_volatile(timer0_ctrl) } & RP2350_TICKS_CTRL_RUNNING
                        == 0
                    {
                        core::hint::spin_loop();
                    }

                    RP2350_TIMER0_TICK_STATE
                        .store(RP2350_TIMER0_TICK_STATE_READY, Ordering::Release);
                    return;
                }
            }
            _ => unreachable!(),
        }
    }
}

fn rp2350_monotonic_now_ticks() -> u64 {
    rp2350_ensure_timer0_tick_started();

    let timer_base = RP2350_EVENT_TIMEOUT_TIMER_BASE;
    let timerawh = (timer_base + RP2350_TIMER_TIMERAWH_OFFSET) as *const u32;
    let timerawl = (timer_base + RP2350_TIMER_TIMERAWL_OFFSET) as *const u32;

    loop {
        // SAFETY: TIMERAWH/TIMERAWL are side-effect-free raw reads of the RP2350 timer register
        // pair. Reading high/low/high until the high word is stable yields one coherent 64-bit
        // monotonic tick snapshot.
        let high_before = unsafe { ptr::read_volatile(timerawh) };
        let low = unsafe { ptr::read_volatile(timerawl) };
        let high_after = unsafe { ptr::read_volatile(timerawh) };
        if high_before == high_after {
            return (u64::from(high_before) << 32) | u64::from(low);
        }
    }
}

fn rp2350_monotonic_now() -> Duration {
    Duration::from_micros(rp2350_monotonic_now_ticks())
}

fn rp2350_arm_event_timeout(timeout: Duration) -> Result<(), HardwareError> {
    rp2350_ensure_timer0_tick_started();
    RP2350_EVENT_TIMEOUT_FIRED.store(false, Ordering::Release);

    let deadline = rp2350_event_timeout_deadline(timeout);
    let alarm_bit = 1_u32 << u32::from(RP2350_EVENT_TIMEOUT_ALARM_INDEX);
    let timer_base = RP2350_EVENT_TIMEOUT_TIMER_BASE;
    let interrupt_clear = (timer_base + RP2350_TIMER_INTR_OFFSET) as *mut u32;
    let interrupt_enable = timer_base + RP2350_TIMER_INTE_OFFSET;
    let alarm = (timer_base
        + RP2350_TIMER_ALARM0_OFFSET
        + (usize::from(RP2350_EVENT_TIMEOUT_ALARM_INDEX) * 4)) as *mut u32;

    rp2350_irq_enable_line(RP2350_EVENT_TIMEOUT_IRQN)?;

    // SAFETY: these are the RP2350 timer interrupt-clear, interrupt-enable, and alarm registers
    // for the reserved backend timeout alarm.
    unsafe {
        ptr::write_volatile(interrupt_clear, alarm_bit);
        ptr::write_volatile(alarm, deadline);
    }
    rp2350_atomic_register_set(interrupt_enable, alarm_bit);
    rp2350_nvic_write(CORTEX_M_NVIC_ICPR, RP2350_EVENT_TIMEOUT_IRQN);
    Ok(())
}

fn rp2350_cancel_event_timeout_alarm() -> Result<(), HardwareError> {
    RP2350_EVENT_TIMEOUT_FIRED.store(false, Ordering::Release);

    let alarm_bit = 1_u32 << u32::from(RP2350_EVENT_TIMEOUT_ALARM_INDEX);
    let timer_base = RP2350_EVENT_TIMEOUT_TIMER_BASE;
    let armed = (timer_base + RP2350_TIMER_ARMED_OFFSET) as *mut u32;
    let interrupt_clear = (timer_base + RP2350_TIMER_INTR_OFFSET) as *mut u32;
    let interrupt_enable = timer_base + RP2350_TIMER_INTE_OFFSET;

    // SAFETY: these are the RP2350 timer armed, interrupt-clear, and interrupt-enable registers
    // for the reserved backend timeout alarm.
    unsafe {
        ptr::write_volatile(armed, alarm_bit);
        ptr::write_volatile(interrupt_clear, alarm_bit);
    }
    rp2350_atomic_register_clear(interrupt_enable, alarm_bit);
    rp2350_irq_disable_line(RP2350_EVENT_TIMEOUT_IRQN)?;
    Ok(())
}

fn rp2350_event_timeout_fired_now() -> bool {
    if RP2350_EVENT_TIMEOUT_FIRED.load(Ordering::Acquire) {
        return true;
    }

    let alarm_bit = 1_u32 << u32::from(RP2350_EVENT_TIMEOUT_ALARM_INDEX);
    let ints = (RP2350_EVENT_TIMEOUT_TIMER_BASE + RP2350_TIMER_INTS_OFFSET) as *const u32;
    // SAFETY: TIMERx_INTS is a side-effect-free masked status register for the reserved backend
    // timeout alarm.
    let masked_status = unsafe { ptr::read_volatile(ints) };
    (masked_status & alarm_bit) != 0
}

fn rp2350_service_event_timeout_irq() -> Result<(), HardwareError> {
    RP2350_EVENT_TIMEOUT_FIRED.store(true, Ordering::Release);
    rp2350_irq_acknowledge_line(RP2350_EVENT_TIMEOUT_IRQN)
}

fn rp2350_enter_power_action(action: Rp2350PowerModeAction) {
    let (use_wfi, deep_sleep) = match action {
        Rp2350PowerModeAction::SleepWfi => (true, false),
        Rp2350PowerModeAction::DeepSleepWfi => (true, true),
    };

    let previous_scr = unsafe { ptr::read_volatile(CORTEX_M_SCB_SCR) };
    let next_scr = if deep_sleep {
        previous_scr | CORTEX_M_SCB_SCR_SLEEPDEEP
    } else {
        previous_scr & !CORTEX_M_SCB_SCR_SLEEPDEEP
    };

    unsafe {
        ptr::write_volatile(CORTEX_M_SCB_SCR, next_scr);
    }
    compiler_fence(Ordering::SeqCst);

    // SAFETY: `WFI` / `WFE` are the architected Cortex-M low-power entry instructions. The SCR
    // deep-sleep bit is restored immediately after wake so callers do not inherit a sticky mode
    // change by surprise.
    unsafe {
        if use_wfi {
            asm!(
                "dsb",
                "wfi",
                "isb",
                options(nomem, nostack, preserves_flags)
            );
        } else {
            asm!(
                "dsb",
                "wfe",
                "isb",
                options(nomem, nostack, preserves_flags)
            );
        }
        ptr::write_volatile(CORTEX_M_SCB_SCR, previous_scr);
    }
    compiler_fence(Ordering::SeqCst);
}

impl CortexMSocBoard for Rp2350Soc {
    fn descriptor(&self) -> CortexMSocDescriptor {
        DESCRIPTOR
    }

    fn local_critical_section_sync_safe(&self) -> bool {
        LOCAL_CRITICAL_SECTION_SYNC_SAFE
    }

    #[allow(clippy::cast_possible_truncation)]
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

    #[allow(clippy::cast_possible_truncation)]
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

    #[allow(clippy::cast_possible_truncation)]
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
                cluster: Some(ThreadClusterId(0)),
                package: Some(MemTopologyNodeId(0)),
                numa_node: None,
                core_class: Some(ThreadCoreClassId(0)),
            },
            authorities: ThreadAuthoritySet::FIRMWARE | ThreadAuthoritySet::TOPOLOGY,
        })
    }

    fn chip_identity(&self) -> Result<CortexMSocChipIdentity, HardwareError> {
        Ok(rp2350_chip_identity())
    }

    fn device_identity_support(&self) -> CortexMSocDeviceIdSupport {
        DEVICE_ID_SUPPORT
    }

    fn device_identity(&self) -> Result<CortexMSocDeviceIdentity, HardwareError> {
        Ok(rp2350_device_identity())
    }

    fn memory_map(&self) -> &'static [CortexMMemoryRegionDescriptor] {
        &MEMORY_MAP
    }

    fn owned_memory_region_count(&self) -> usize {
        usize::from(rp2350_owned_sram_region().is_some())
    }

    fn owned_memory_region(&self, index: usize) -> Option<CortexMMemoryRegionDescriptor> {
        match index {
            0 => rp2350_owned_sram_region(),
            _ => None,
        }
    }

    fn peripherals(&self) -> &'static [CortexMPeripheralDescriptor] {
        &PERIPHERALS
    }

    fn irqs(&self) -> &'static [CortexMIrqDescriptor] {
        &IRQS
    }

    fn irq_enable(&self, irqn: u16) -> Result<(), HardwareError> {
        rp2350_irq_enable_line(irqn)
    }

    fn irq_disable(&self, irqn: u16) -> Result<(), HardwareError> {
        rp2350_irq_disable_line(irqn)
    }

    fn irq_priority_supported(&self, irqn: u16) -> bool {
        rp2350_irq_is_known(irqn)
    }

    fn irq_implemented_priority_bits(&self) -> u8 {
        4
    }

    fn irq_set_priority(&self, irqn: u16, priority: u8) -> Result<(), HardwareError> {
        rp2350_irq_set_priority(irqn, priority)
    }

    fn irq_clear_pending(&self, irqn: u16) -> Result<(), HardwareError> {
        rp2350_irq_clear_pending_line(irqn)
    }

    fn irq_set_pending(&self, irqn: u16) -> Result<(), HardwareError> {
        rp2350_irq_set_pending_line(irqn)
    }

    fn irq_acknowledge_supported(&self, irqn: u16) -> bool {
        rp2350_timer_base_and_alarm_bit(irqn).is_some()
            || rp2350_dma_irq_group(irqn).is_some()
            || rp2350_uart_base(irqn).is_some()
            || rp2350_i2c_base(irqn).is_some()
    }

    fn irq_acknowledge(&self, irqn: u16) -> Result<(), HardwareError> {
        rp2350_irq_acknowledge_line(irqn)
    }

    fn clock_tree(&self) -> &'static [CortexMClockDescriptor] {
        &CLOCK_TREE
    }

    fn dma_controllers(&self) -> &'static [CortexMDmaControllerDescriptor] {
        &DMA_CONTROLLERS
    }

    fn dma_requests(&self) -> &'static [CortexMDmaRequestDescriptor] {
        &DMA_REQUESTS
    }

    fn power_modes(&self) -> &'static [CortexMPowerModeDescriptor] {
        &POWER_MODES
    }

    fn event_timeout_support(&self) -> Option<CortexMEventTimeoutSupport> {
        Some(RP2350_EVENT_TIMEOUT_SUPPORT)
    }

    fn arm_event_timeout(&self, timeout: Duration) -> Result<(), HardwareError> {
        rp2350_arm_event_timeout(timeout)
    }

    fn cancel_event_timeout(&self) -> Result<(), HardwareError> {
        rp2350_cancel_event_timeout_alarm()
    }

    fn event_timeout_fired(&self) -> Result<bool, HardwareError> {
        Ok(rp2350_event_timeout_fired_now())
    }

    fn inline_current_exception_stack_allows(&self, required_bytes: usize) -> bool {
        rp2350_inline_current_exception_stack_allows(required_bytes)
    }

    fn exception_stack_observation(
        &self,
    ) -> Result<CortexMExceptionStackObservation, HardwareError> {
        Ok(rp2350_exception_stack_observation())
    }

    fn monotonic_now_supported(&self) -> bool {
        true
    }

    fn monotonic_now(&self) -> Result<Duration, HardwareError> {
        Ok(rp2350_monotonic_now())
    }

    fn monotonic_raw_bits(&self) -> Option<u32> {
        Some(64)
    }

    fn monotonic_tick_hz(&self) -> Option<u64> {
        Some(1_000_000)
    }

    fn monotonic_raw_now(&self) -> Result<u64, HardwareError> {
        Ok(rp2350_monotonic_now_ticks())
    }

    fn overclock_support(&self) -> CortexMSocOverclockSupport {
        CortexMSocOverclockSupport::ProfileCatalog
    }

    fn overclock_profiles(&self) -> &'static [CortexMSocOverclockProfile] {
        RP2350_OVERCLOCK_PROFILES
    }

    fn current_sys_clock_hz(&self) -> Option<u64> {
        Some(150_000_000)
    }

    fn active_overclock_profile(&self) -> Option<&'static str> {
        Some("stock-150mhz")
    }

    fn pal_power_modes(&self) -> &'static [PowerModeDescriptor] {
        &PAL_POWER_MODES
    }

    fn enter_power_mode(&self, name: &str) -> Result<(), HardwareError> {
        let action = rp2350_power_mode_action(name).ok_or_else(HardwareError::unsupported)?;
        rp2350_enter_power_action(action);
        Ok(())
    }

    fn flash_regions(&self) -> &'static [CortexMFlashRegionDescriptor] {
        &FLASH_REGIONS
    }
}

/// Returns the RP2350 programmable-IO support surface.
#[must_use]
pub const fn pcu_support() -> PcuSupport {
    RP2350_PIO_SUPPORT
}

/// Returns the RP2350 programmable-IO engine descriptors.
#[must_use]
pub fn pcu_engines() -> &'static [PcuEngineDescriptor] {
    &PIO_ENGINES
}

/// Returns the RP2350 programmable-IO lane descriptors for one engine.
#[must_use]
pub fn pcu_lanes(engine: PcuEngineId) -> &'static [PcuLaneDescriptor] {
    rp2350_pio_lane_descriptors(engine)
}

/// Claims one RP2350 PIO engine exclusively.
pub fn claim_pcu_engine(engine: PcuEngineId) -> Result<PcuEngineClaim, PcuError> {
    let engine_index = usize::from(engine.0);
    if engine_index >= RP2350_PIO_ENGINE_COUNT {
        return Err(PcuError::invalid());
    }
    RP2350_PIO_ENGINE_CLAIMS[engine_index]
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| PcuError::busy())?;
    if let Err(error) = rp2350_unreset_pio_engine(engine) {
        RP2350_PIO_ENGINE_CLAIMS[engine_index].store(false, Ordering::Release);
        return Err(error);
    }
    Ok(PcuEngineClaim { engine })
}

/// Releases one RP2350 PIO engine claim.
pub fn release_pcu_engine(claim: PcuEngineClaim) -> Result<(), PcuError> {
    let engine_index = usize::from(claim.engine().0);
    if engine_index >= RP2350_PIO_ENGINE_COUNT {
        return Err(PcuError::invalid());
    }
    if !RP2350_PIO_ENGINE_CLAIMS[engine_index].swap(false, Ordering::AcqRel) {
        return Err(PcuError::state_conflict());
    }
    Ok(())
}

/// Claims one or more RP2350 PIO lanes.
pub fn claim_pcu_lanes(engine: PcuEngineId, lanes: PcuLaneMask) -> Result<PcuLaneClaim, PcuError> {
    let engine_index = usize::from(engine.0);
    let bits = lanes.bits();
    if engine_index >= RP2350_PIO_ENGINE_COUNT || !rp2350_valid_lane_mask(lanes) {
        return Err(PcuError::invalid());
    }

    let claims = &RP2350_PIO_LANE_CLAIMS[engine_index];
    loop {
        let current = claims.load(Ordering::Acquire);
        if current & bits != 0 {
            return Err(PcuError::busy());
        }
        let next = current | bits;
        if claims
            .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return Ok(PcuLaneClaim { engine, lanes });
        }
    }
}

/// Releases one RP2350 PIO lane claim.
pub fn release_pcu_lanes(claim: PcuLaneClaim) -> Result<(), PcuError> {
    let (engine_index, bits) = rp2350_validate_lane_claim(&claim)?;
    let claims = &RP2350_PIO_LANE_CLAIMS[engine_index];
    loop {
        let current = claims.load(Ordering::Acquire);
        if current & bits != bits {
            return Err(PcuError::state_conflict());
        }
        let next = current & !bits;
        if claims
            .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return Ok(());
        }
    }
}

/// Loads one native RP2350 PIO program image into a claimed engine.
pub fn load_pcu_program(
    claim: &PcuEngineClaim,
    image: &PcuProgramImage<'_>,
) -> Result<PcuProgramLease, PcuError> {
    let _engine_index = rp2350_validate_engine_claim(claim)?;
    let base = rp2350_pio_base(claim.engine()).ok_or_else(PcuError::invalid)?;
    if image.words.is_empty() {
        return Err(PcuError::invalid());
    }
    if image.words.len() > usize::from(RP2350_PIO_INSTRUCTION_WORDS) {
        return Err(PcuError::resource_exhausted());
    }

    for (index, word) in image.words.iter().enumerate() {
        let register = (base + RP2350_PIO_INSTR_MEM0_OFFSET + (index * core::mem::size_of::<u32>()))
            as *mut u32;
        // SAFETY: these are RP2350 PIO instruction-memory write-only registers. The contract
        // requires a claimed engine, and each write programs one 16-bit native instruction word.
        unsafe { ptr::write_volatile(register, u32::from(*word)) };
    }
    for index in image.words.len()..usize::from(RP2350_PIO_INSTRUCTION_WORDS) {
        let register = (base + RP2350_PIO_INSTR_MEM0_OFFSET + (index * core::mem::size_of::<u32>()))
            as *mut u32;
        // SAFETY: clearing the unused tail keeps the engine image honest instead of preserving
        // stale instructions from an earlier load.
        unsafe { ptr::write_volatile(register, 0) };
    }

    Ok(PcuProgramLease {
        engine: claim.engine(),
        program: image.id,
        word_count: image.words.len() as u16,
    })
}

/// Unloads one native RP2350 PIO program image from a claimed engine.
pub fn unload_pcu_program(claim: &PcuEngineClaim, lease: PcuProgramLease) -> Result<(), PcuError> {
    let _engine_index = rp2350_validate_engine_claim(claim)?;
    if claim.engine().0 != lease.engine().0 {
        return Err(PcuError::invalid());
    }
    let base = rp2350_pio_base(claim.engine()).ok_or_else(PcuError::invalid)?;
    for index in 0..usize::from(RP2350_PIO_INSTRUCTION_WORDS) {
        let register = (base + RP2350_PIO_INSTR_MEM0_OFFSET + (index * core::mem::size_of::<u32>()))
            as *mut u32;
        // SAFETY: clearing the full instruction memory is the honest unload path for RP2350's
        // shared program store.
        unsafe { ptr::write_volatile(register, 0) };
    }
    Ok(())
}

/// Starts one claimed RP2350 PIO lane set.
pub fn start_pcu_lanes(claim: &PcuLaneClaim) -> Result<(), PcuError> {
    let (_engine_index, bits) = rp2350_validate_lane_claim(claim)?;
    let register =
        rp2350_pio_base(claim.engine()).ok_or_else(PcuError::invalid)? + RP2350_PIO_CTRL_OFFSET;
    rp2350_atomic_register_set(register, u32::from(bits) & RP2350_PIO_CTRL_SM_ENABLE_MASK);
    Ok(())
}

/// Stops one claimed RP2350 PIO lane set.
pub fn stop_pcu_lanes(claim: &PcuLaneClaim) -> Result<(), PcuError> {
    let (_engine_index, bits) = rp2350_validate_lane_claim(claim)?;
    let register =
        rp2350_pio_base(claim.engine()).ok_or_else(PcuError::invalid)? + RP2350_PIO_CTRL_OFFSET;
    rp2350_atomic_register_clear(register, u32::from(bits) & RP2350_PIO_CTRL_SM_ENABLE_MASK);
    Ok(())
}

/// Restarts one claimed RP2350 PIO lane set.
pub fn restart_pcu_lanes(claim: &PcuLaneClaim) -> Result<(), PcuError> {
    let (_engine_index, bits) = rp2350_validate_lane_claim(claim)?;
    let register =
        rp2350_pio_base(claim.engine()).ok_or_else(PcuError::invalid)? + RP2350_PIO_CTRL_OFFSET;
    let bits = u32::from(bits) & RP2350_PIO_CTRL_SM_ENABLE_MASK;
    rp2350_atomic_register_set(
        register,
        (bits << RP2350_PIO_CTRL_SM_RESTART_SHIFT) | (bits << RP2350_PIO_CTRL_CLKDIV_RESTART_SHIFT),
    );
    Ok(())
}

/// Writes one word to one claimed RP2350 PIO TX FIFO.
pub fn write_pcu_tx_fifo(claim: &PcuLaneClaim, lane: PcuLaneId, word: u32) -> Result<(), PcuError> {
    let (_engine_index, _) = rp2350_validate_lane_claim(claim)?;
    if !claim.contains_lane(lane) {
        return Err(PcuError::invalid());
    }
    let base = rp2350_pio_base(claim.engine()).ok_or_else(PcuError::invalid)?;
    let fstat = (base + RP2350_PIO_FSTAT_OFFSET) as *const u32;
    // SAFETY: FSTAT is a read-only summary register for lane FIFO occupancy.
    let state = unsafe { ptr::read_volatile(fstat) };
    if state & (1_u32 << (RP2350_PIO_FSTAT_TXFULL_SHIFT + u32::from(lane.index))) != 0 {
        return Err(PcuError::busy());
    }
    let register = (base
        + RP2350_PIO_TXF0_OFFSET
        + (usize::from(lane.index) * core::mem::size_of::<u32>())) as *mut u32;
    // SAFETY: TXF registers are RP2350 lane-local write-only FIFOs.
    unsafe { ptr::write_volatile(register, word) };
    Ok(())
}

/// Reads one word from one claimed RP2350 PIO RX FIFO.
pub fn read_pcu_rx_fifo(claim: &PcuLaneClaim, lane: PcuLaneId) -> Result<u32, PcuError> {
    let (_engine_index, _) = rp2350_validate_lane_claim(claim)?;
    if !claim.contains_lane(lane) {
        return Err(PcuError::invalid());
    }
    let base = rp2350_pio_base(claim.engine()).ok_or_else(PcuError::invalid)?;
    let fstat = (base + RP2350_PIO_FSTAT_OFFSET) as *const u32;
    // SAFETY: FSTAT is a read-only summary register for lane FIFO occupancy.
    let state = unsafe { ptr::read_volatile(fstat) };
    if state & (1_u32 << (RP2350_PIO_FSTAT_RXEMPTY_SHIFT + u32::from(lane.index))) != 0 {
        return Err(PcuError::busy());
    }
    let register = (base
        + RP2350_PIO_RXF0_OFFSET
        + (usize::from(lane.index) * core::mem::size_of::<u32>())) as *const u32;
    // SAFETY: RXF registers are RP2350 lane-local read-only FIFOs.
    Ok(unsafe { ptr::read_volatile(register) })
}

const fn rp2350_pio_sm_register(base: usize, lane_index: u8, offset: usize) -> usize {
    base + offset + (lane_index as usize * RP2350_PIO_SM_STRIDE)
}

const fn rp2350_encode_pcu_jmp(target: u8) -> u16 {
    target as u16
}

fn rp2350_clear_pcu_fifos(base: usize, bits: u8) {
    for lane_index in 0..RP2350_PIO_LANES_PER_ENGINE as u8 {
        if bits & (1u8 << lane_index) == 0 {
            continue;
        }
        let shiftctrl_register =
            rp2350_pio_sm_register(base, lane_index, RP2350_PIO_SM0_SHIFTCTRL_OFFSET) as *mut u32;
        // SAFETY: the lane claim serializes access to one concrete state machine's control
        // register block. Toggling FJOIN_RX and then restoring the original value is the
        // documented FIFO flush sequence for RP2350 PIO state machines.
        unsafe {
            let original = ptr::read_volatile(shiftctrl_register);
            ptr::write_volatile(
                shiftctrl_register,
                original ^ RP2350_PIO_SM_SHIFTCTRL_FJOIN_RX_BIT,
            );
            ptr::write_volatile(shiftctrl_register, original);
        }
    }
}

fn rp2350_clear_pcu_fifo_debug(base: usize, bits: u8) {
    let register = (base + RP2350_PIO_FDEBUG_OFFSET) as *mut u32;
    let bit_mask = u32::from(bits);
    let clear_mask = (bit_mask << RP2350_PIO_FDEBUG_TXSTALL_SHIFT)
        | (bit_mask << RP2350_PIO_FDEBUG_TXOVER_SHIFT)
        | (bit_mask << RP2350_PIO_FDEBUG_RXUNDER_SHIFT)
        | (bit_mask << RP2350_PIO_FDEBUG_RXSTALL_SHIFT);
    // SAFETY: FDEBUG is a write-clear register. Writing a lane mask only clears sticky FIFO
    // debug bits for the selected state machines.
    unsafe { ptr::write_volatile(register, clear_mask) };
}

fn rp2350_prime_pcu_program_counter(base: usize, bits: u8, initial_pc: u8) {
    let jmp = u32::from(rp2350_encode_pcu_jmp(initial_pc));
    for lane_index in 0..RP2350_PIO_LANES_PER_ENGINE as u8 {
        if bits & (1u8 << lane_index) == 0 {
            continue;
        }
        let instr_register =
            rp2350_pio_sm_register(base, lane_index, RP2350_PIO_SM0_INSTR_OFFSET) as *mut u32;
        // SAFETY: writing to SMx_INSTR executes one instruction immediately for the selected
        // state machine. A single unconditional JMP establishes a known entry PC before enable.
        unsafe { ptr::write_volatile(instr_register, jmp) };
    }
}

/// Applies the RP2350-equivalent `pio_sm_init()` sequence to one claimed lane set.
pub fn initialize_pcu_lanes(claim: &PcuLaneClaim, initial_pc: u8) -> Result<(), PcuError> {
    if initial_pc >= RP2350_PIO_INSTRUCTION_WORDS as u8 {
        return Err(PcuError::invalid());
    }
    let (_engine_index, bits) = rp2350_validate_lane_claim(claim)?;
    let base = rp2350_pio_base(claim.engine()).ok_or_else(PcuError::invalid)?;
    stop_pcu_lanes(claim)?;
    rp2350_clear_pcu_fifos(base, bits);
    rp2350_clear_pcu_fifo_debug(base, bits);
    restart_pcu_lanes(claim)?;
    rp2350_prime_pcu_program_counter(base, bits, initial_pc);
    Ok(())
}

/// Applies one RP2350 PIO execution-state bundle to all lanes in the supplied claim.
pub fn apply_pcu_execution_config(
    claim: &PcuLaneClaim,
    clkdiv: u32,
    execctrl: u32,
    shiftctrl: u32,
    pinctrl: u32,
) -> Result<(), PcuError> {
    let (_engine_index, bits) = rp2350_validate_lane_claim(claim)?;
    let base = rp2350_pio_base(claim.engine()).ok_or_else(PcuError::invalid)?;

    for lane_index in 0..RP2350_PIO_LANES_PER_ENGINE as u8 {
        if bits & (1u8 << lane_index) == 0 {
            continue;
        }
        let clkdiv_register =
            rp2350_pio_sm_register(base, lane_index, RP2350_PIO_SM0_CLKDIV_OFFSET) as *mut u32;
        let execctrl_register =
            rp2350_pio_sm_register(base, lane_index, RP2350_PIO_SM0_EXECCTRL_OFFSET) as *mut u32;
        let shiftctrl_register =
            rp2350_pio_sm_register(base, lane_index, RP2350_PIO_SM0_SHIFTCTRL_OFFSET) as *mut u32;
        let pinctrl_register =
            rp2350_pio_sm_register(base, lane_index, RP2350_PIO_SM0_PINCTRL_OFFSET) as *mut u32;

        // SAFETY: the caller holds a truthful lane claim for these state machines. These are the
        // RP2350 per-lane execution-control registers for the selected PIO engine.
        unsafe {
            ptr::write_volatile(clkdiv_register, clkdiv);
            ptr::write_volatile(execctrl_register, execctrl);
            ptr::write_volatile(shiftctrl_register, shiftctrl);
            ptr::write_volatile(pinctrl_register, pinctrl);
        }
    }

    Ok(())
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

/// Returns one observation of the selected RP2350 board-owned main/exception stack window.
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
/// Returns an error if the selected board cannot surface a truthful device identity.
pub fn device_identity() -> Result<CortexMSocDeviceIdentity, HardwareError> {
    board_contract::device_identity(system_soc())
}

/// Returns whether local interrupt masking is sufficient to serialize local synchronization on the
/// selected RP2350 target.
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

/// Writes topology-defined cluster identifiers for the selected RP2350 SoC.
///
/// # Errors
///
/// Returns an error if the selected SoC does not expose cluster identities honestly.
pub fn write_clusters(
    output: &mut [ThreadClusterId],
) -> Result<HardwareWriteSummary, HardwareError> {
    board_contract::write_clusters(system_soc(), output)
}

/// Writes topology-defined package identifiers for the selected RP2350 SoC.
///
/// # Errors
///
/// Returns an error if the selected SoC does not expose package identities honestly.
pub fn write_packages(
    output: &mut [MemTopologyNodeId],
) -> Result<HardwareWriteSummary, HardwareError> {
    board_contract::write_packages(system_soc(), output)
}

/// Writes topology-defined core-class identifiers for the selected RP2350 SoC.
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

/// Returns the selected RP2350 memory map.
#[must_use]
pub fn memory_map() -> &'static [CortexMMemoryRegionDescriptor] {
    board_contract::memory_map(system_soc())
}

/// Returns the number of board-owned runtime memory regions for the selected RP2350 board.
#[must_use]
pub fn owned_memory_region_count() -> usize {
    board_contract::owned_memory_region_count(system_soc())
}

/// Returns one board-owned runtime memory region for the selected RP2350 board.
#[must_use]
pub fn owned_memory_region(index: usize) -> Option<CortexMMemoryRegionDescriptor> {
    board_contract::owned_memory_region(system_soc(), index)
}

/// Returns the selected RP2350 peripheral descriptors.
#[must_use]
pub fn peripherals() -> &'static [CortexMPeripheralDescriptor] {
    board_contract::peripherals(system_soc())
}

/// Returns the selected RP2350 IRQ descriptors.
#[must_use]
pub fn irqs() -> &'static [CortexMIrqDescriptor] {
    board_contract::irqs(system_soc())
}

/// Enables one named external IRQ line on the selected RP2350 board.
///
/// # Errors
///
/// Returns an error if the requested IRQ line is unknown.
pub fn irq_enable(irqn: u16) -> Result<(), HardwareError> {
    board_contract::irq_enable(system_soc(), irqn)
}

/// Disables one named external IRQ line on the selected RP2350 board.
///
/// # Errors
///
/// Returns an error if the requested IRQ line is unknown.
pub fn irq_disable(irqn: u16) -> Result<(), HardwareError> {
    board_contract::irq_disable(system_soc(), irqn)
}

/// Returns whether one RP2350 IRQ line supports raw NVIC priority control.
#[must_use]
pub fn irq_priority_supported(irqn: u16) -> bool {
    board_contract::irq_priority_supported(system_soc(), irqn)
}

/// Returns the number of implemented raw NVIC priority bits on RP2350.
#[must_use]
pub fn irq_implemented_priority_bits() -> u8 {
    board_contract::irq_implemented_priority_bits(system_soc())
}

/// Applies one raw NVIC priority byte to one RP2350 IRQ line.
///
/// # Errors
///
/// Returns an error if the requested IRQ line is unknown.
pub fn irq_set_priority(irqn: u16, priority: u8) -> Result<(), HardwareError> {
    board_contract::irq_set_priority(system_soc(), irqn, priority)
}

/// Clears the NVIC pending state for one RP2350 IRQ line.
///
/// # Errors
///
/// Returns an error if the requested IRQ line is unknown.
pub fn irq_clear_pending(irqn: u16) -> Result<(), HardwareError> {
    board_contract::irq_clear_pending(system_soc(), irqn)
}

/// Sets the NVIC pending state for one RP2350 IRQ line.
///
/// # Errors
///
/// Returns an error if the requested IRQ line is unknown.
pub fn irq_set_pending(irqn: u16) -> Result<(), HardwareError> {
    board_contract::irq_set_pending(system_soc(), irqn)
}

/// Returns whether one IRQ line can be acknowledged generically on the selected RP2350 board.
#[must_use]
pub fn irq_acknowledge_supported(irqn: u16) -> bool {
    board_contract::irq_acknowledge_supported(system_soc(), irqn)
}

/// Acknowledges one IRQ line on the selected RP2350 board.
///
/// # Errors
///
/// Returns an error if the IRQ line cannot be acknowledged generically by the board contract.
pub fn irq_acknowledge(irqn: u16) -> Result<(), HardwareError> {
    board_contract::irq_acknowledge(system_soc(), irqn)
}

/// Returns a raw GPIO-summary snapshot for one RP2350 IO-bank IRQ line.
///
/// This is the driver-local escape hatch for shared-summary GPIO IRQs where the generic board
/// contract intentionally refuses to lie about a universal acknowledge path.
pub fn gpio_irq_summary(irqn: u16) -> Result<Rp2350GpioIrqSummary, HardwareError> {
    rp2350_gpio_irq_summary_snapshot(irqn)
}

/// Clears edge-triggered GPIO causes for one RP2350 IO-bank IRQ summary word.
///
/// `word_index` is bank-local and `edge_mask` uses the raw nibble layout from `INTRx`.
///
/// # Errors
///
/// Returns an error when the IRQ line or word index is invalid for the selected bank.
pub fn gpio_irq_clear_edges(
    irqn: u16,
    word_index: usize,
    edge_mask: u32,
) -> Result<(), HardwareError> {
    rp2350_gpio_irq_clear_edges(irqn, word_index, edge_mask)
}

/// Returns a raw PIO-summary snapshot for one RP2350 PIO IRQ line.
pub fn pio_irq_summary(irqn: u16) -> Result<Rp2350PioIrqSummary, HardwareError> {
    rp2350_pio_irq_summary_snapshot(irqn)
}

/// Clears the internal PIO IRQ flags surfaced by one RP2350 PIO IRQ line.
///
/// This does not pretend FIFO threshold conditions are clearable; it only clears the internal
/// `PIO_IRQ` flag byte.
pub fn pio_irq_clear_internal_flags(irqn: u16, flags: u8) -> Result<(), HardwareError> {
    rp2350_pio_irq_clear_internal_flags(irqn, flags)
}

/// Returns a raw SPI-summary snapshot for one RP2350 SPI IRQ line.
pub fn spi_irq_summary(irqn: u16) -> Result<Rp2350SpiIrqSummary, HardwareError> {
    rp2350_spi_irq_summary_snapshot(irqn)
}

/// Acknowledges the clearable SPI interrupt causes for one RP2350 SPI IRQ line.
///
/// The returned mask contains the RT/ROR bits that were actually cleared.
pub fn spi_irq_acknowledge_clearable(irqn: u16) -> Result<u8, HardwareError> {
    rp2350_spi_irq_acknowledge_clearable(irqn)
}

/// Returns the selected RP2350 clock-tree descriptors.
#[must_use]
pub fn clock_tree() -> &'static [CortexMClockDescriptor] {
    board_contract::clock_tree(system_soc())
}

/// Returns the selected RP2350 DMA controller descriptors.
#[must_use]
pub fn dma_controllers() -> &'static [CortexMDmaControllerDescriptor] {
    board_contract::dma_controllers(system_soc())
}

/// Returns the selected RP2350 DMA request descriptors.
#[must_use]
pub fn dma_requests() -> &'static [CortexMDmaRequestDescriptor] {
    board_contract::dma_requests(system_soc())
}

/// Returns the selected RP2350 power-mode descriptors.
#[must_use]
pub fn power_modes() -> &'static [CortexMPowerModeDescriptor] {
    board_contract::power_modes(system_soc())
}

/// Returns whether the selected RP2350 board exposes a truthful finite event-timeout source.
#[must_use]
pub fn event_timeout_supported() -> bool {
    board_contract::event_timeout_supported(system_soc())
}

/// Returns one truthful finite event-timeout source summary for the selected RP2350 board.
#[must_use]
pub fn event_timeout_support() -> Option<CortexMEventTimeoutSupport> {
    board_contract::event_timeout_support(system_soc())
}

/// Returns the board-reserved IRQ line used by the selected RP2350 board's event timeout source.
#[must_use]
pub fn event_timeout_irq() -> Option<u16> {
    board_contract::event_timeout_irq(system_soc())
}

/// Arms the selected RP2350 board's event-timeout source.
///
/// # Errors
///
/// Returns an error if the selected board cannot surface finite event timeouts honestly.
pub fn arm_event_timeout(timeout: Duration) -> Result<(), HardwareError> {
    board_contract::arm_event_timeout(system_soc(), timeout)
}

/// Cancels the selected RP2350 board's event-timeout source.
///
/// # Errors
///
/// Returns an error if the selected board cannot surface finite event timeouts honestly.
pub fn cancel_event_timeout() -> Result<(), HardwareError> {
    board_contract::cancel_event_timeout(system_soc())
}

/// Returns whether the selected RP2350 board's event-timeout source has fired.
///
/// # Errors
///
/// Returns an error if the selected board cannot surface finite event timeouts honestly.
pub fn event_timeout_fired() -> Result<bool, HardwareError> {
    board_contract::event_timeout_fired(system_soc())
}

/// Records and acknowledges one serviced RP2350 event-timeout interrupt.
///
/// This is intended for one board-owned timer IRQ handler which wakes thread-context waiters
/// without leaving the alarm latched forever.
///
/// # Errors
///
/// Returns an error if the selected timer IRQ cannot be acknowledged on this SoC.
pub fn service_event_timeout_irq() -> Result<(), HardwareError> {
    rp2350_service_event_timeout_irq()
}

/// Returns whether the selected RP2350 board exposes one truthful monotonic timebase.
#[must_use]
pub fn monotonic_now_supported() -> bool {
    board_contract::monotonic_now_supported(system_soc())
}

/// Returns the current monotonic timebase reading for the selected RP2350 board.
///
/// # Errors
///
/// Returns an error if the selected RP2350 board cannot surface one truthful monotonic timebase.
pub fn monotonic_now() -> Result<Duration, HardwareError> {
    board_contract::monotonic_now(system_soc())
}

/// Returns the width in bits of the selected RP2350 board's raw monotonic counter, when one
/// exists.
#[must_use]
pub fn monotonic_raw_bits() -> Option<u32> {
    board_contract::monotonic_raw_bits(system_soc())
}

/// Returns the tick rate of the selected RP2350 board's raw monotonic counter, when one exists.
#[must_use]
pub fn monotonic_tick_hz() -> Option<u64> {
    board_contract::monotonic_tick_hz(system_soc())
}

/// Returns the selected RP2350 board's raw monotonic counter widened into `u64`.
///
/// # Errors
///
/// Returns an error if the selected RP2350 board cannot surface one truthful raw monotonic
/// counter.
pub fn monotonic_raw_now() -> Result<u64, HardwareError> {
    board_contract::monotonic_raw_now(system_soc())
}

/// Returns the selected RP2350 board's overclock or system-clock profile support level.
#[must_use]
pub fn overclock_support() -> CortexMSocOverclockSupport {
    board_contract::overclock_support(system_soc())
}

/// Returns the selected RP2350 board's overclock or system-clock profiles.
#[must_use]
pub fn overclock_profiles() -> &'static [CortexMSocOverclockProfile] {
    board_contract::overclock_profiles(system_soc())
}

/// Returns the selected RP2350 board's current effective system/core clock frequency, when it can
/// be surfaced honestly.
#[must_use]
pub fn current_sys_clock_hz() -> Option<u64> {
    board_contract::current_sys_clock_hz(system_soc())
}

/// Returns the selected RP2350 board's currently active overclock or system-clock profile, when
/// it can be surfaced honestly.
#[must_use]
pub fn active_overclock_profile() -> Option<&'static str> {
    board_contract::active_overclock_profile(system_soc())
}

/// Applies one named overclock or system-clock profile on the selected RP2350 target.
///
/// # Errors
///
/// Returns an error because runtime profile application is not yet implemented honestly.
pub fn apply_overclock_profile(name: &str) -> Result<(), HardwareError> {
    board_contract::apply_overclock_profile(system_soc(), name)
}

/// Returns the selected RP2350 PAL-facing power descriptors.
#[must_use]
pub fn pal_power_modes() -> &'static [PowerModeDescriptor] {
    board_contract::pal_power_modes(system_soc())
}

/// Enters one named power mode on the selected RP2350 target.
///
/// # Errors
///
/// Returns an error if the selected RP2350 target cannot honestly enter the named mode.
pub fn enter_power_mode(name: &str) -> Result<(), HardwareError> {
    board_contract::enter_power_mode(system_soc(), name)
}

/// Returns the selected RP2350 flash/XIP descriptors.
#[must_use]
pub fn flash_regions() -> &'static [CortexMFlashRegionDescriptor] {
    board_contract::flash_regions(system_soc())
}

#[cfg(test)]
mod tests;
