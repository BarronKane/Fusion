#![allow(clippy::doc_markdown)]

//! RP2350 Cortex-M SoC descriptor.
//!
//! This module is where verified RP2350 memory-map, peripheral, and clock-tree facts belong.
//! The current implementation wires the architected topology, the major static memory regions,
//! the major peripheral blocks, and the board-visible clock domains from the RP2350 datasheet
//! and Pico SDK clock model.

use core::arch::asm;
use core::ptr;
use core::sync::atomic::{Ordering, compiler_fence};
use core::time::Duration;

use crate::pal::hal::{
    HardwareAuthoritySet,
    HardwareError,
    HardwareTopologySummary,
    HardwareWriteSummary,
};
use crate::pal::mem::{CachePolicy, MemResourceBackingKind, Protect, RegionAttrs};
use crate::pal::power::{PowerModeDepth, PowerModeDescriptor};
use crate::pal::thread::{
    ThreadAuthoritySet,
    ThreadCoreId,
    ThreadError,
    ThreadExecutionLocation,
    ThreadId,
    ThreadLogicalCpuId,
    ThreadProcessorGroupId,
};

use super::board_contract::{self, CortexMSocBoard};

pub use super::board_contract::{
    CortexMClockDescriptor,
    CortexMDmaControllerDescriptor,
    CortexMDmaRequestClass,
    CortexMDmaRequestDescriptor,
    CortexMDmaTransferCaps,
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

/// Whether local interrupt masking is sufficient to serialize local synchronization on RP2350.
pub const LOCAL_CRITICAL_SECTION_SYNC_SAFE: bool = false;
/// Runtime per-device identity support class for RP2350 boards.
pub const DEVICE_ID_SUPPORT: CortexMSocDeviceIdSupport = CortexMSocDeviceIdSupport::OtpReadable;

const APB_SLOT_BYTES: usize = 0x0000_8000;
const AHB_SLOT_BYTES: usize = 0x0010_0000;
const ROM_BYTES: usize = 32 * 1024;
const XIP_WINDOW_BYTES: usize = 32 * 1024 * 1024;
const SRAM_BYTES: usize = 0x0008_2000;
const APB_SEGMENT_BYTES: usize = 0x0016_8000;
const AHB_SEGMENT_BYTES: usize = 0x0080_0000;
const SIO_SEGMENT_BYTES: usize = 0x0004_0000;
const PPB_SEGMENT_BYTES: usize = 0x000A_0000;

const RP2350_SYSINFO_CHIP_ID: *const u32 = 0x4000_0000 as *const u32;
const RP2350_SYSINFO_PLATFORM: *const u32 = 0x4000_0008 as *const u32;
const RP2350_SYSINFO_GITREF_RP2350: *const u32 = 0x4000_0014 as *const u32;
const RP2350_SYSINFO_CHIP_INFO: *const u32 = 0x4000_0018 as *const u32;
const RP2350_OTP_DATA: *const u32 = 0x4013_0000 as *const u32;
const RP2350_SIO_CPUID: *const u32 = 0xd000_0000 as *const u32;
const CORTEX_M_SCB_SCR: *mut u32 = 0xE000_ED10 as *mut u32;
const CORTEX_M_SCB_SCR_SLEEPDEEP: u32 = 1 << 2;
const CORTEX_M_NVIC_ISER: *mut u32 = 0xE000_E100 as *mut u32;
const CORTEX_M_NVIC_ICER: *mut u32 = 0xE000_E180 as *mut u32;
const CORTEX_M_NVIC_ICPR: *mut u32 = 0xE000_E280 as *mut u32;
const RP2350_TIMER0_BASE: usize = 0x400b_0000;
const RP2350_TIMER1_BASE: usize = 0x400b_8000;
const RP2350_IO_BANK0_BASE: usize = 0x4002_8000;
const RP2350_IO_QSPI_BASE: usize = 0x4003_0000;
const RP2350_DMA_BASE: usize = 0x5000_0000;
const RP2350_SPI0_BASE: usize = 0x4008_0000;
const RP2350_SPI1_BASE: usize = 0x4008_8000;
const RP2350_UART0_BASE: usize = 0x4007_0000;
const RP2350_UART1_BASE: usize = 0x4007_8000;
const RP2350_I2C0_BASE: usize = 0x4009_0000;
const RP2350_I2C1_BASE: usize = 0x4009_8000;
const RP2350_PIO0_BASE: usize = 0x5020_0000;
const RP2350_PIO1_BASE: usize = 0x5030_0000;
const RP2350_PIO2_BASE: usize = 0x5040_0000;
const RP2350_EVENT_TIMEOUT_TIMER_BASE: usize = RP2350_TIMER0_BASE;
const RP2350_EVENT_TIMEOUT_ALARM_INDEX: u16 = 3;
const RP2350_EVENT_TIMEOUT_IRQN: u16 = 3;
const RP2350_IO_BANK0_INTR0_OFFSET: usize = 0x230;
const RP2350_IO_QSPI_INTR_OFFSET: usize = 0x218;
const RP2350_IO_IRQ_WORD_STRIDE: usize = 0x4;
const RP2350_GPIO_BANK0_SUMMARY_WORDS: usize = 6;
const RP2350_GPIO_QSPI_SUMMARY_WORDS: usize = 1;
const RP2350_GPIO_EDGE_EVENT_MASK: u32 = 0xCCCC_CCCC;
const RP2350_TIMER_ALARM0_OFFSET: usize = 0x10;
const RP2350_TIMER_ARMED_OFFSET: usize = 0x20;
const RP2350_TIMER_TIMERAWL_OFFSET: usize = 0x28;
const RP2350_TIMER_INTR_OFFSET: usize = 0x3c;
const RP2350_TIMER_INTE_OFFSET: usize = 0x40;
const RP2350_TIMER_INTS_OFFSET: usize = 0x48;
const RP2350_DMA_INTS0_OFFSET: usize = 0x40c;
const RP2350_SPI_SSPMIS_OFFSET: usize = 0x1c;
const RP2350_SPI_SSPICR_OFFSET: usize = 0x20;
const RP2350_SPI_SSPICR_CLEARABLE_MASK: u32 = 0x3;
const RP2350_UARTMIS_OFFSET: usize = 0x40;
const RP2350_UARTICR_OFFSET: usize = 0x44;
const RP2350_UARTICR_CLEARABLE_BITS: u32 = 0x0000_07ff;
const RP2350_I2C_IC_INTR_STAT_OFFSET: usize = 0x2c;
const RP2350_I2C_IC_CLR_INTR_OFFSET: usize = 0x40;
const RP2350_PIO_IRQ_OFFSET: usize = 0x30;
const RP2350_PIO_IRQ0_INTS_OFFSET: usize = 0x178;
const RP2350_PIO_IRQ1_INTS_OFFSET: usize = 0x184;

unsafe extern "C" {
    static __sheap: u8;
    static _stack_end: u8;
}

const CLK_REF_MAIN_SOURCES: &[&str] = &[
    "rosc_clkr_ref",
    "clksrc_clk_ref_aux",
    "xosc_clkr_ref",
    "lposc_clkr_ref",
];
const CLK_REF_AUX_SOURCES: &[&str] = &["clksrc_gpin0", "clksrc_gpin1", "pll_usb_clkr_ref"];
const CLK_REF_CONSUMERS: &[&str] = &["otp", "powman", "ticks"];
const CLK_SYS_MAIN_SOURCES: &[&str] = &["clksrc_clk_sys_aux", "clk_ref"];
const CLK_SYS_AUX_SOURCES: &[&str] = &[
    "clksrc_pll_sys",
    "clksrc_gpin0",
    "clksrc_gpin1",
    "clksrc_pll_usb",
    "rosc_clkr_sys",
    "xosc_clkr_sys",
];
const CLK_SYS_CONSUMERS: &[&str] = &[
    "cores",
    "bootram",
    "busctrl",
    "bus fabric",
    "dma",
    "glitch detector",
    "pio0",
    "pio1",
    "pio2",
    "rom",
    "sha256",
    "sio",
    "sram",
    "timer0",
    "timer1",
    "trng",
];
const CLK_PERI_MAIN_SOURCES: &[&str] = &[];
const CLK_PERI_AUX_SOURCES: &[&str] = &[
    "clksrc_pll_sys",
    "clksrc_gpin0",
    "clksrc_gpin1",
    "clksrc_pll_usb",
    "rosc_clksrc_ph",
    "xosc_clksrc",
    "clk_sys",
];
const CLK_PERI_CONSUMERS: &[&str] = &["uart0", "uart1", "spi0", "spi1", "i2c0", "i2c1"];
const CLK_HSTX_MAIN_SOURCES: &[&str] = &[];
const CLK_HSTX_AUX_SOURCES: &[&str] = &["clksrc_pll_sys", "clksrc_pll_usb", "clk_sys"];
const CLK_HSTX_CONSUMERS: &[&str] = &["hstx"];
const CLK_USB_MAIN_SOURCES: &[&str] = &[];
const CLK_USB_AUX_SOURCES: &[&str] = &[
    "clksrc_pll_sys",
    "clksrc_gpin0",
    "clksrc_gpin1",
    "clksrc_pll_usb",
    "rosc_clksrc_ph",
    "xosc_clksrc",
];
const CLK_USB_CONSUMERS: &[&str] = &["usbctrl"];
const CLK_ADC_MAIN_SOURCES: &[&str] = &[];
const CLK_ADC_AUX_SOURCES: &[&str] = &[
    "clksrc_pll_sys",
    "clksrc_gpin0",
    "clksrc_gpin1",
    "clksrc_pll_usb",
    "rosc_clksrc_ph",
    "xosc_clksrc",
];
const CLK_ADC_CONSUMERS: &[&str] = &["adc"];
const RP2350_SLEEP_WAKE_SOURCES: &[&str] = &["irq", "sev", "timer", "gpio"];
const RP2350_SLEEP_GATED_DOMAINS: &[&str] = &["core pipeline"];
const RP2350_DEEP_SLEEP_WAKE_SOURCES: &[&str] = &["irq", "gpio", "timer"];
const RP2350_DEEP_SLEEP_GATED_DOMAINS: &[&str] = &["clk_sys", "clk_peri", "core pipeline"];
const RP2350_FLASH_BYTES: usize = 4 * 1024 * 1024;
const RP2350_FLASH_ERASE_BLOCK_BYTES: usize = 4 * 1024;
const RP2350_FLASH_PROGRAM_GRANULE_BYTES: usize = 256;

const MEMORY_MAP: [CortexMMemoryRegionDescriptor; 8] = [
    CortexMMemoryRegionDescriptor {
        name: "rom",
        kind: CortexMMemoryRegionKind::Rom,
        base: 0x0000_0000,
        len: ROM_BYTES,
        protect: Protect::READ.union(Protect::EXEC),
        attrs: RegionAttrs::STATIC_REGION.union(RegionAttrs::EXECUTABLE),
        cache: CachePolicy::Default,
        backing: MemResourceBackingKind::StaticRegion,
        allocatable: false,
    },
    CortexMMemoryRegionDescriptor {
        name: "xip-cached",
        kind: CortexMMemoryRegionKind::Xip,
        base: 0x1000_0000,
        len: XIP_WINDOW_BYTES,
        protect: Protect::READ.union(Protect::EXEC),
        attrs: RegionAttrs::STATIC_REGION
            .union(RegionAttrs::CACHEABLE)
            .union(RegionAttrs::EXECUTABLE),
        cache: CachePolicy::Default,
        backing: MemResourceBackingKind::StaticRegion,
        allocatable: false,
    },
    CortexMMemoryRegionDescriptor {
        name: "xip-nocache-noalloc",
        kind: CortexMMemoryRegionKind::Xip,
        base: 0x1400_0000,
        len: XIP_WINDOW_BYTES,
        protect: Protect::READ.union(Protect::EXEC),
        attrs: RegionAttrs::STATIC_REGION.union(RegionAttrs::EXECUTABLE),
        cache: CachePolicy::Uncached,
        backing: MemResourceBackingKind::StaticRegion,
        allocatable: false,
    },
    CortexMMemoryRegionDescriptor {
        name: "sram",
        kind: CortexMMemoryRegionKind::Sram,
        base: 0x2000_0000,
        len: SRAM_BYTES,
        protect: Protect::READ.union(Protect::WRITE).union(Protect::EXEC),
        attrs: RegionAttrs::STATIC_REGION
            .union(RegionAttrs::DMA_VISIBLE)
            .union(RegionAttrs::CACHEABLE)
            .union(RegionAttrs::COHERENT)
            .union(RegionAttrs::EXECUTABLE),
        cache: CachePolicy::Default,
        backing: MemResourceBackingKind::StaticRegion,
        allocatable: false,
    },
    CortexMMemoryRegionDescriptor {
        name: "apb-peripherals",
        kind: CortexMMemoryRegionKind::Mmio,
        base: 0x4000_0000,
        len: APB_SEGMENT_BYTES,
        protect: Protect::READ.union(Protect::WRITE),
        attrs: RegionAttrs::STATIC_REGION,
        cache: CachePolicy::Uncached,
        backing: MemResourceBackingKind::Mmio,
        allocatable: false,
    },
    CortexMMemoryRegionDescriptor {
        name: "ahb-peripherals",
        kind: CortexMMemoryRegionKind::Mmio,
        base: 0x5000_0000,
        len: AHB_SEGMENT_BYTES,
        protect: Protect::READ.union(Protect::WRITE),
        attrs: RegionAttrs::STATIC_REGION,
        cache: CachePolicy::Uncached,
        backing: MemResourceBackingKind::Mmio,
        allocatable: false,
    },
    CortexMMemoryRegionDescriptor {
        name: "sio",
        kind: CortexMMemoryRegionKind::Mmio,
        base: 0xd000_0000,
        len: SIO_SEGMENT_BYTES,
        protect: Protect::READ.union(Protect::WRITE),
        attrs: RegionAttrs::STATIC_REGION,
        cache: CachePolicy::Uncached,
        backing: MemResourceBackingKind::Mmio,
        allocatable: false,
    },
    CortexMMemoryRegionDescriptor {
        name: "ppb",
        kind: CortexMMemoryRegionKind::Mmio,
        base: 0xe000_0000,
        len: PPB_SEGMENT_BYTES,
        protect: Protect::READ.union(Protect::WRITE),
        attrs: RegionAttrs::STATIC_REGION,
        cache: CachePolicy::Uncached,
        backing: MemResourceBackingKind::Mmio,
        allocatable: false,
    },
];

const PERIPHERALS: [CortexMPeripheralDescriptor; 45] = [
    CortexMPeripheralDescriptor {
        name: "sysinfo",
        bus: CortexMPeripheralBus::Apb,
        base: 0x4000_0000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "syscfg",
        bus: CortexMPeripheralBus::Apb,
        base: 0x4000_8000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "clocks",
        bus: CortexMPeripheralBus::Apb,
        base: 0x4001_0000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "psm",
        bus: CortexMPeripheralBus::Apb,
        base: 0x4001_8000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "resets",
        bus: CortexMPeripheralBus::Apb,
        base: 0x4002_0000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "io_bank0",
        bus: CortexMPeripheralBus::Apb,
        base: 0x4002_8000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "pads_bank0",
        bus: CortexMPeripheralBus::Apb,
        base: 0x4003_8000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "xosc",
        bus: CortexMPeripheralBus::Apb,
        base: 0x4004_8000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "pll_sys",
        bus: CortexMPeripheralBus::Apb,
        base: 0x4005_0000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "pll_usb",
        bus: CortexMPeripheralBus::Apb,
        base: 0x4005_8000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "accessctrl",
        bus: CortexMPeripheralBus::Apb,
        base: 0x4006_0000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "busctrl",
        bus: CortexMPeripheralBus::Apb,
        base: 0x4006_8000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "uart0",
        bus: CortexMPeripheralBus::Apb,
        base: 0x4007_0000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "uart1",
        bus: CortexMPeripheralBus::Apb,
        base: 0x4007_8000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "spi0",
        bus: CortexMPeripheralBus::Apb,
        base: 0x4008_0000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "spi1",
        bus: CortexMPeripheralBus::Apb,
        base: 0x4008_8000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "i2c0",
        bus: CortexMPeripheralBus::Apb,
        base: 0x4009_0000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "i2c1",
        bus: CortexMPeripheralBus::Apb,
        base: 0x4009_8000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "adc",
        bus: CortexMPeripheralBus::Apb,
        base: 0x400a_0000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "pwm",
        bus: CortexMPeripheralBus::Apb,
        base: 0x400a_8000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "timer0",
        bus: CortexMPeripheralBus::Apb,
        base: 0x400b_0000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "timer1",
        bus: CortexMPeripheralBus::Apb,
        base: 0x400b_8000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "hstx_ctrl",
        bus: CortexMPeripheralBus::Apb,
        base: 0x400c_0000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "xip_ctrl",
        bus: CortexMPeripheralBus::Apb,
        base: 0x400c_8000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "xip_qmi",
        bus: CortexMPeripheralBus::Apb,
        base: 0x400d_0000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "watchdog",
        bus: CortexMPeripheralBus::Apb,
        base: 0x400d_8000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "bootram",
        bus: CortexMPeripheralBus::Apb,
        base: 0x400e_0000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "rosc",
        bus: CortexMPeripheralBus::Apb,
        base: 0x400e_8000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "trng",
        bus: CortexMPeripheralBus::Apb,
        base: 0x400f_0000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "sha256",
        bus: CortexMPeripheralBus::Apb,
        base: 0x400f_8000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "powman",
        bus: CortexMPeripheralBus::Apb,
        base: 0x4010_0000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "ticks",
        bus: CortexMPeripheralBus::Apb,
        base: 0x4010_8000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "otp",
        bus: CortexMPeripheralBus::Apb,
        base: 0x4012_0000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "otp_data",
        bus: CortexMPeripheralBus::Apb,
        base: 0x4013_0000,
        len: APB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "dma",
        bus: CortexMPeripheralBus::Ahb,
        base: 0x5000_0000,
        len: AHB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "usbctrl_dpram",
        bus: CortexMPeripheralBus::Ahb,
        base: 0x5010_0000,
        len: 0x0001_0000,
    },
    CortexMPeripheralDescriptor {
        name: "usbctrl_regs",
        bus: CortexMPeripheralBus::Ahb,
        base: 0x5011_0000,
        len: 0x0001_0000,
    },
    CortexMPeripheralDescriptor {
        name: "pio0",
        bus: CortexMPeripheralBus::Ahb,
        base: 0x5020_0000,
        len: AHB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "pio1",
        bus: CortexMPeripheralBus::Ahb,
        base: 0x5030_0000,
        len: AHB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "pio2",
        bus: CortexMPeripheralBus::Ahb,
        base: 0x5040_0000,
        len: AHB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "xip_aux",
        bus: CortexMPeripheralBus::Ahb,
        base: 0x5050_0000,
        len: AHB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "hstx_fifo",
        bus: CortexMPeripheralBus::Ahb,
        base: 0x5060_0000,
        len: AHB_SLOT_BYTES,
    },
    CortexMPeripheralDescriptor {
        name: "sio",
        bus: CortexMPeripheralBus::Sio,
        base: 0xd000_0000,
        len: 0x0002_0000,
    },
    CortexMPeripheralDescriptor {
        name: "sio_nonsec",
        bus: CortexMPeripheralBus::Sio,
        base: 0xd002_0000,
        len: 0x0002_0000,
    },
    CortexMPeripheralDescriptor {
        name: "ppb",
        bus: CortexMPeripheralBus::Ppb,
        base: 0xe000_0000,
        len: 0x0002_0000,
    },
];

const fn irq(
    name: &'static str,
    irqn: u16,
    peripheral: Option<&'static str>,
    class: CortexMIrqClass,
    endpoint: Option<&'static str>,
    nonsecure: bool,
) -> CortexMIrqDescriptor {
    CortexMIrqDescriptor {
        name,
        irqn,
        peripheral,
        class,
        endpoint,
        nonsecure,
    }
}

const IRQS: [CortexMIrqDescriptor; 52] = [
    irq(
        "timer0-irq0",
        0,
        Some("timer0"),
        CortexMIrqClass::Timer,
        Some("irq0"),
        false,
    ),
    irq(
        "timer0-irq1",
        1,
        Some("timer0"),
        CortexMIrqClass::Timer,
        Some("irq1"),
        false,
    ),
    irq(
        "timer0-irq2",
        2,
        Some("timer0"),
        CortexMIrqClass::Timer,
        Some("irq2"),
        false,
    ),
    irq(
        "timer0-irq3",
        3,
        Some("timer0"),
        CortexMIrqClass::Timer,
        Some("irq3"),
        false,
    ),
    irq(
        "timer1-irq0",
        4,
        Some("timer1"),
        CortexMIrqClass::Timer,
        Some("irq0"),
        false,
    ),
    irq(
        "timer1-irq1",
        5,
        Some("timer1"),
        CortexMIrqClass::Timer,
        Some("irq1"),
        false,
    ),
    irq(
        "timer1-irq2",
        6,
        Some("timer1"),
        CortexMIrqClass::Timer,
        Some("irq2"),
        false,
    ),
    irq(
        "timer1-irq3",
        7,
        Some("timer1"),
        CortexMIrqClass::Timer,
        Some("irq3"),
        false,
    ),
    irq(
        "pwm-wrap0",
        8,
        Some("pwm"),
        CortexMIrqClass::Pwm,
        Some("wrap0"),
        false,
    ),
    irq(
        "pwm-wrap1",
        9,
        Some("pwm"),
        CortexMIrqClass::Pwm,
        Some("wrap1"),
        false,
    ),
    irq(
        "dma-irq0",
        10,
        Some("dma"),
        CortexMIrqClass::Dma,
        Some("irq0"),
        false,
    ),
    irq(
        "dma-irq1",
        11,
        Some("dma"),
        CortexMIrqClass::Dma,
        Some("irq1"),
        false,
    ),
    irq(
        "dma-irq2",
        12,
        Some("dma"),
        CortexMIrqClass::Dma,
        Some("irq2"),
        false,
    ),
    irq(
        "dma-irq3",
        13,
        Some("dma"),
        CortexMIrqClass::Dma,
        Some("irq3"),
        false,
    ),
    irq(
        "usbctrl",
        14,
        Some("usbctrl"),
        CortexMIrqClass::Usb,
        None,
        false,
    ),
    irq(
        "pio0-irq0",
        15,
        Some("pio0"),
        CortexMIrqClass::Pio,
        Some("irq0"),
        false,
    ),
    irq(
        "pio0-irq1",
        16,
        Some("pio0"),
        CortexMIrqClass::Pio,
        Some("irq1"),
        false,
    ),
    irq(
        "pio1-irq0",
        17,
        Some("pio1"),
        CortexMIrqClass::Pio,
        Some("irq0"),
        false,
    ),
    irq(
        "pio1-irq1",
        18,
        Some("pio1"),
        CortexMIrqClass::Pio,
        Some("irq1"),
        false,
    ),
    irq(
        "pio2-irq0",
        19,
        Some("pio2"),
        CortexMIrqClass::Pio,
        Some("irq0"),
        false,
    ),
    irq(
        "pio2-irq1",
        20,
        Some("pio2"),
        CortexMIrqClass::Pio,
        Some("irq1"),
        false,
    ),
    irq(
        "io-bank0",
        21,
        Some("io_bank0"),
        CortexMIrqClass::Gpio,
        None,
        false,
    ),
    irq(
        "io-bank0-ns",
        22,
        Some("io_bank0"),
        CortexMIrqClass::Gpio,
        None,
        true,
    ),
    irq(
        "io-qspi",
        23,
        Some("io_qspi"),
        CortexMIrqClass::Gpio,
        None,
        false,
    ),
    irq(
        "io-qspi-ns",
        24,
        Some("io_qspi"),
        CortexMIrqClass::Gpio,
        None,
        true,
    ),
    irq(
        "sio-fifo",
        25,
        Some("sio"),
        CortexMIrqClass::Sio,
        Some("fifo"),
        false,
    ),
    irq(
        "sio-bell",
        26,
        Some("sio"),
        CortexMIrqClass::Sio,
        Some("bell"),
        false,
    ),
    irq(
        "sio-fifo-ns",
        27,
        Some("sio"),
        CortexMIrqClass::Sio,
        Some("fifo"),
        true,
    ),
    irq(
        "sio-bell-ns",
        28,
        Some("sio"),
        CortexMIrqClass::Sio,
        Some("bell"),
        true,
    ),
    irq(
        "sio-mtimecmp",
        29,
        Some("sio"),
        CortexMIrqClass::Sio,
        Some("mtimecmp"),
        false,
    ),
    irq(
        "clocks",
        30,
        Some("clocks"),
        CortexMIrqClass::Clock,
        None,
        false,
    ),
    irq("spi0", 31, Some("spi0"), CortexMIrqClass::Spi, None, false),
    irq("spi1", 32, Some("spi1"), CortexMIrqClass::Spi, None, false),
    irq(
        "uart0",
        33,
        Some("uart0"),
        CortexMIrqClass::Uart,
        None,
        false,
    ),
    irq(
        "uart1",
        34,
        Some("uart1"),
        CortexMIrqClass::Uart,
        None,
        false,
    ),
    irq(
        "adc-fifo",
        35,
        Some("adc"),
        CortexMIrqClass::Adc,
        Some("fifo"),
        false,
    ),
    irq("i2c0", 36, Some("i2c0"), CortexMIrqClass::I2c, None, false),
    irq("i2c1", 37, Some("i2c1"), CortexMIrqClass::I2c, None, false),
    irq("otp", 38, Some("otp"), CortexMIrqClass::Otp, None, false),
    irq("trng", 39, Some("trng"), CortexMIrqClass::Trng, None, false),
    irq(
        "proc0-cti",
        40,
        Some("proc0"),
        CortexMIrqClass::CoreTrace,
        Some("cti"),
        false,
    ),
    irq(
        "proc1-cti",
        41,
        Some("proc1"),
        CortexMIrqClass::CoreTrace,
        Some("cti"),
        false,
    ),
    irq(
        "pll-sys",
        42,
        Some("pll_sys"),
        CortexMIrqClass::Pll,
        None,
        false,
    ),
    irq(
        "pll-usb",
        43,
        Some("pll_usb"),
        CortexMIrqClass::Pll,
        None,
        false,
    ),
    irq(
        "powman-pow",
        44,
        Some("powman"),
        CortexMIrqClass::Power,
        Some("pow"),
        false,
    ),
    irq(
        "powman-timer",
        45,
        Some("powman"),
        CortexMIrqClass::Power,
        Some("timer"),
        false,
    ),
    irq("spare0", 46, None, CortexMIrqClass::Spare, Some("0"), false),
    irq("spare1", 47, None, CortexMIrqClass::Spare, Some("1"), false),
    irq("spare2", 48, None, CortexMIrqClass::Spare, Some("2"), false),
    irq("spare3", 49, None, CortexMIrqClass::Spare, Some("3"), false),
    irq("spare4", 50, None, CortexMIrqClass::Spare, Some("4"), false),
    irq("spare5", 51, None, CortexMIrqClass::Spare, Some("5"), false),
];

const DMA_CONTROLLERS: [CortexMDmaControllerDescriptor; 1] = [CortexMDmaControllerDescriptor {
    name: "dma",
    base: 0x5000_0000,
    channel_count: 16,
    transfer_caps: CortexMDmaTransferCaps::MEMORY_TO_MEMORY
        .union(CortexMDmaTransferCaps::MEMORY_TO_PERIPHERAL)
        .union(CortexMDmaTransferCaps::PERIPHERAL_TO_MEMORY)
        .union(CortexMDmaTransferCaps::CHANNEL_CHAINING),
}];

const DMA_PERIPHERAL_TX_CAPS: CortexMDmaTransferCaps = CortexMDmaTransferCaps::MEMORY_TO_PERIPHERAL;
const DMA_PERIPHERAL_RX_CAPS: CortexMDmaTransferCaps = CortexMDmaTransferCaps::PERIPHERAL_TO_MEMORY;
const DMA_PACER_CAPS: CortexMDmaTransferCaps = CortexMDmaTransferCaps::MEMORY_TO_MEMORY
    .union(CortexMDmaTransferCaps::MEMORY_TO_PERIPHERAL)
    .union(CortexMDmaTransferCaps::PERIPHERAL_TO_MEMORY);

const fn dma_request(
    name: &'static str,
    request_line: u16,
    peripheral: Option<&'static str>,
    class: CortexMDmaRequestClass,
    endpoint: Option<&'static str>,
    transfer_caps: CortexMDmaTransferCaps,
) -> CortexMDmaRequestDescriptor {
    CortexMDmaRequestDescriptor {
        name,
        request_line,
        peripheral,
        class,
        endpoint,
        transfer_caps,
    }
}

const DMA_REQUESTS: [CortexMDmaRequestDescriptor; 60] = [
    dma_request(
        "pio0-tx0",
        0,
        Some("pio0"),
        CortexMDmaRequestClass::PeripheralTx,
        Some("tx0"),
        DMA_PERIPHERAL_TX_CAPS,
    ),
    dma_request(
        "pio0-tx1",
        1,
        Some("pio0"),
        CortexMDmaRequestClass::PeripheralTx,
        Some("tx1"),
        DMA_PERIPHERAL_TX_CAPS,
    ),
    dma_request(
        "pio0-tx2",
        2,
        Some("pio0"),
        CortexMDmaRequestClass::PeripheralTx,
        Some("tx2"),
        DMA_PERIPHERAL_TX_CAPS,
    ),
    dma_request(
        "pio0-tx3",
        3,
        Some("pio0"),
        CortexMDmaRequestClass::PeripheralTx,
        Some("tx3"),
        DMA_PERIPHERAL_TX_CAPS,
    ),
    dma_request(
        "pio0-rx0",
        4,
        Some("pio0"),
        CortexMDmaRequestClass::PeripheralRx,
        Some("rx0"),
        DMA_PERIPHERAL_RX_CAPS,
    ),
    dma_request(
        "pio0-rx1",
        5,
        Some("pio0"),
        CortexMDmaRequestClass::PeripheralRx,
        Some("rx1"),
        DMA_PERIPHERAL_RX_CAPS,
    ),
    dma_request(
        "pio0-rx2",
        6,
        Some("pio0"),
        CortexMDmaRequestClass::PeripheralRx,
        Some("rx2"),
        DMA_PERIPHERAL_RX_CAPS,
    ),
    dma_request(
        "pio0-rx3",
        7,
        Some("pio0"),
        CortexMDmaRequestClass::PeripheralRx,
        Some("rx3"),
        DMA_PERIPHERAL_RX_CAPS,
    ),
    dma_request(
        "pio1-tx0",
        8,
        Some("pio1"),
        CortexMDmaRequestClass::PeripheralTx,
        Some("tx0"),
        DMA_PERIPHERAL_TX_CAPS,
    ),
    dma_request(
        "pio1-tx1",
        9,
        Some("pio1"),
        CortexMDmaRequestClass::PeripheralTx,
        Some("tx1"),
        DMA_PERIPHERAL_TX_CAPS,
    ),
    dma_request(
        "pio1-tx2",
        10,
        Some("pio1"),
        CortexMDmaRequestClass::PeripheralTx,
        Some("tx2"),
        DMA_PERIPHERAL_TX_CAPS,
    ),
    dma_request(
        "pio1-tx3",
        11,
        Some("pio1"),
        CortexMDmaRequestClass::PeripheralTx,
        Some("tx3"),
        DMA_PERIPHERAL_TX_CAPS,
    ),
    dma_request(
        "pio1-rx0",
        12,
        Some("pio1"),
        CortexMDmaRequestClass::PeripheralRx,
        Some("rx0"),
        DMA_PERIPHERAL_RX_CAPS,
    ),
    dma_request(
        "pio1-rx1",
        13,
        Some("pio1"),
        CortexMDmaRequestClass::PeripheralRx,
        Some("rx1"),
        DMA_PERIPHERAL_RX_CAPS,
    ),
    dma_request(
        "pio1-rx2",
        14,
        Some("pio1"),
        CortexMDmaRequestClass::PeripheralRx,
        Some("rx2"),
        DMA_PERIPHERAL_RX_CAPS,
    ),
    dma_request(
        "pio1-rx3",
        15,
        Some("pio1"),
        CortexMDmaRequestClass::PeripheralRx,
        Some("rx3"),
        DMA_PERIPHERAL_RX_CAPS,
    ),
    dma_request(
        "pio2-tx0",
        16,
        Some("pio2"),
        CortexMDmaRequestClass::PeripheralTx,
        Some("tx0"),
        DMA_PERIPHERAL_TX_CAPS,
    ),
    dma_request(
        "pio2-tx1",
        17,
        Some("pio2"),
        CortexMDmaRequestClass::PeripheralTx,
        Some("tx1"),
        DMA_PERIPHERAL_TX_CAPS,
    ),
    dma_request(
        "pio2-tx2",
        18,
        Some("pio2"),
        CortexMDmaRequestClass::PeripheralTx,
        Some("tx2"),
        DMA_PERIPHERAL_TX_CAPS,
    ),
    dma_request(
        "pio2-tx3",
        19,
        Some("pio2"),
        CortexMDmaRequestClass::PeripheralTx,
        Some("tx3"),
        DMA_PERIPHERAL_TX_CAPS,
    ),
    dma_request(
        "pio2-rx0",
        20,
        Some("pio2"),
        CortexMDmaRequestClass::PeripheralRx,
        Some("rx0"),
        DMA_PERIPHERAL_RX_CAPS,
    ),
    dma_request(
        "pio2-rx1",
        21,
        Some("pio2"),
        CortexMDmaRequestClass::PeripheralRx,
        Some("rx1"),
        DMA_PERIPHERAL_RX_CAPS,
    ),
    dma_request(
        "pio2-rx2",
        22,
        Some("pio2"),
        CortexMDmaRequestClass::PeripheralRx,
        Some("rx2"),
        DMA_PERIPHERAL_RX_CAPS,
    ),
    dma_request(
        "pio2-rx3",
        23,
        Some("pio2"),
        CortexMDmaRequestClass::PeripheralRx,
        Some("rx3"),
        DMA_PERIPHERAL_RX_CAPS,
    ),
    dma_request(
        "spi0-tx",
        24,
        Some("spi0"),
        CortexMDmaRequestClass::PeripheralTx,
        Some("tx"),
        DMA_PERIPHERAL_TX_CAPS,
    ),
    dma_request(
        "spi0-rx",
        25,
        Some("spi0"),
        CortexMDmaRequestClass::PeripheralRx,
        Some("rx"),
        DMA_PERIPHERAL_RX_CAPS,
    ),
    dma_request(
        "spi1-tx",
        26,
        Some("spi1"),
        CortexMDmaRequestClass::PeripheralTx,
        Some("tx"),
        DMA_PERIPHERAL_TX_CAPS,
    ),
    dma_request(
        "spi1-rx",
        27,
        Some("spi1"),
        CortexMDmaRequestClass::PeripheralRx,
        Some("rx"),
        DMA_PERIPHERAL_RX_CAPS,
    ),
    dma_request(
        "uart0-tx",
        28,
        Some("uart0"),
        CortexMDmaRequestClass::PeripheralTx,
        Some("tx"),
        DMA_PERIPHERAL_TX_CAPS,
    ),
    dma_request(
        "uart0-rx",
        29,
        Some("uart0"),
        CortexMDmaRequestClass::PeripheralRx,
        Some("rx"),
        DMA_PERIPHERAL_RX_CAPS,
    ),
    dma_request(
        "uart1-tx",
        30,
        Some("uart1"),
        CortexMDmaRequestClass::PeripheralTx,
        Some("tx"),
        DMA_PERIPHERAL_TX_CAPS,
    ),
    dma_request(
        "uart1-rx",
        31,
        Some("uart1"),
        CortexMDmaRequestClass::PeripheralRx,
        Some("rx"),
        DMA_PERIPHERAL_RX_CAPS,
    ),
    dma_request(
        "pwm-wrap0",
        32,
        Some("pwm"),
        CortexMDmaRequestClass::PeripheralPacer,
        Some("wrap0"),
        DMA_PACER_CAPS,
    ),
    dma_request(
        "pwm-wrap1",
        33,
        Some("pwm"),
        CortexMDmaRequestClass::PeripheralPacer,
        Some("wrap1"),
        DMA_PACER_CAPS,
    ),
    dma_request(
        "pwm-wrap2",
        34,
        Some("pwm"),
        CortexMDmaRequestClass::PeripheralPacer,
        Some("wrap2"),
        DMA_PACER_CAPS,
    ),
    dma_request(
        "pwm-wrap3",
        35,
        Some("pwm"),
        CortexMDmaRequestClass::PeripheralPacer,
        Some("wrap3"),
        DMA_PACER_CAPS,
    ),
    dma_request(
        "pwm-wrap4",
        36,
        Some("pwm"),
        CortexMDmaRequestClass::PeripheralPacer,
        Some("wrap4"),
        DMA_PACER_CAPS,
    ),
    dma_request(
        "pwm-wrap5",
        37,
        Some("pwm"),
        CortexMDmaRequestClass::PeripheralPacer,
        Some("wrap5"),
        DMA_PACER_CAPS,
    ),
    dma_request(
        "pwm-wrap6",
        38,
        Some("pwm"),
        CortexMDmaRequestClass::PeripheralPacer,
        Some("wrap6"),
        DMA_PACER_CAPS,
    ),
    dma_request(
        "pwm-wrap7",
        39,
        Some("pwm"),
        CortexMDmaRequestClass::PeripheralPacer,
        Some("wrap7"),
        DMA_PACER_CAPS,
    ),
    dma_request(
        "pwm-wrap8",
        40,
        Some("pwm"),
        CortexMDmaRequestClass::PeripheralPacer,
        Some("wrap8"),
        DMA_PACER_CAPS,
    ),
    dma_request(
        "pwm-wrap9",
        41,
        Some("pwm"),
        CortexMDmaRequestClass::PeripheralPacer,
        Some("wrap9"),
        DMA_PACER_CAPS,
    ),
    dma_request(
        "pwm-wrap10",
        42,
        Some("pwm"),
        CortexMDmaRequestClass::PeripheralPacer,
        Some("wrap10"),
        DMA_PACER_CAPS,
    ),
    dma_request(
        "pwm-wrap11",
        43,
        Some("pwm"),
        CortexMDmaRequestClass::PeripheralPacer,
        Some("wrap11"),
        DMA_PACER_CAPS,
    ),
    dma_request(
        "i2c0-tx",
        44,
        Some("i2c0"),
        CortexMDmaRequestClass::PeripheralTx,
        Some("tx"),
        DMA_PERIPHERAL_TX_CAPS,
    ),
    dma_request(
        "i2c0-rx",
        45,
        Some("i2c0"),
        CortexMDmaRequestClass::PeripheralRx,
        Some("rx"),
        DMA_PERIPHERAL_RX_CAPS,
    ),
    dma_request(
        "i2c1-tx",
        46,
        Some("i2c1"),
        CortexMDmaRequestClass::PeripheralTx,
        Some("tx"),
        DMA_PERIPHERAL_TX_CAPS,
    ),
    dma_request(
        "i2c1-rx",
        47,
        Some("i2c1"),
        CortexMDmaRequestClass::PeripheralRx,
        Some("rx"),
        DMA_PERIPHERAL_RX_CAPS,
    ),
    dma_request(
        "adc",
        48,
        Some("adc"),
        CortexMDmaRequestClass::PeripheralRx,
        Some("fifo"),
        DMA_PERIPHERAL_RX_CAPS,
    ),
    dma_request(
        "xip-stream",
        49,
        Some("xip_aux"),
        CortexMDmaRequestClass::PeripheralRx,
        Some("stream"),
        DMA_PERIPHERAL_RX_CAPS,
    ),
    dma_request(
        "xip-qmitx",
        50,
        Some("xip_qmi"),
        CortexMDmaRequestClass::PeripheralTx,
        Some("tx"),
        DMA_PERIPHERAL_TX_CAPS,
    ),
    dma_request(
        "xip-qmirx",
        51,
        Some("xip_qmi"),
        CortexMDmaRequestClass::PeripheralRx,
        Some("rx"),
        DMA_PERIPHERAL_RX_CAPS,
    ),
    dma_request(
        "hstx",
        52,
        Some("hstx"),
        CortexMDmaRequestClass::PeripheralTx,
        Some("fifo"),
        DMA_PERIPHERAL_TX_CAPS,
    ),
    dma_request(
        "coresight-trace",
        53,
        Some("coresight"),
        CortexMDmaRequestClass::PeripheralRx,
        Some("trace"),
        DMA_PERIPHERAL_RX_CAPS,
    ),
    dma_request(
        "sha256",
        54,
        Some("sha256"),
        CortexMDmaRequestClass::PeripheralTx,
        Some("fifo"),
        DMA_PERIPHERAL_TX_CAPS,
    ),
    dma_request(
        "dma-timer0",
        59,
        None,
        CortexMDmaRequestClass::TimerPacer,
        Some("timer0"),
        DMA_PACER_CAPS,
    ),
    dma_request(
        "dma-timer1",
        60,
        None,
        CortexMDmaRequestClass::TimerPacer,
        Some("timer1"),
        DMA_PACER_CAPS,
    ),
    dma_request(
        "dma-timer2",
        61,
        None,
        CortexMDmaRequestClass::TimerPacer,
        Some("timer2"),
        DMA_PACER_CAPS,
    ),
    dma_request(
        "dma-timer3",
        62,
        None,
        CortexMDmaRequestClass::TimerPacer,
        Some("timer3"),
        DMA_PACER_CAPS,
    ),
    dma_request(
        "force",
        63,
        None,
        CortexMDmaRequestClass::Force,
        None,
        DMA_PACER_CAPS,
    ),
];

const CLOCK_TREE: [CortexMClockDescriptor; 6] = [
    CortexMClockDescriptor {
        name: "clk_ref",
        main_sources: CLK_REF_MAIN_SOURCES,
        aux_sources: CLK_REF_AUX_SOURCES,
        consumers: CLK_REF_CONSUMERS,
    },
    CortexMClockDescriptor {
        name: "clk_sys",
        main_sources: CLK_SYS_MAIN_SOURCES,
        aux_sources: CLK_SYS_AUX_SOURCES,
        consumers: CLK_SYS_CONSUMERS,
    },
    CortexMClockDescriptor {
        name: "clk_peri",
        main_sources: CLK_PERI_MAIN_SOURCES,
        aux_sources: CLK_PERI_AUX_SOURCES,
        consumers: CLK_PERI_CONSUMERS,
    },
    CortexMClockDescriptor {
        name: "clk_hstx",
        main_sources: CLK_HSTX_MAIN_SOURCES,
        aux_sources: CLK_HSTX_AUX_SOURCES,
        consumers: CLK_HSTX_CONSUMERS,
    },
    CortexMClockDescriptor {
        name: "clk_usb",
        main_sources: CLK_USB_MAIN_SOURCES,
        aux_sources: CLK_USB_AUX_SOURCES,
        consumers: CLK_USB_CONSUMERS,
    },
    CortexMClockDescriptor {
        name: "clk_adc",
        main_sources: CLK_ADC_MAIN_SOURCES,
        aux_sources: CLK_ADC_AUX_SOURCES,
        consumers: CLK_ADC_CONSUMERS,
    },
];

const POWER_MODES: [CortexMPowerModeDescriptor; 2] = [
    CortexMPowerModeDescriptor {
        name: "sleep-wfi",
        uses_wfi: true,
        uses_wfe: false,
        deep_sleep: false,
        wake_sources: RP2350_SLEEP_WAKE_SOURCES,
        gated_domains: RP2350_SLEEP_GATED_DOMAINS,
    },
    CortexMPowerModeDescriptor {
        name: "deep-sleep-wfi",
        uses_wfi: true,
        uses_wfe: false,
        deep_sleep: true,
        wake_sources: RP2350_DEEP_SLEEP_WAKE_SOURCES,
        gated_domains: RP2350_DEEP_SLEEP_GATED_DOMAINS,
    },
];

const PAL_POWER_MODES: [PowerModeDescriptor; 2] = [
    PowerModeDescriptor {
        name: "sleep-wfi",
        depth: PowerModeDepth::Sleep,
        wake_sources: RP2350_SLEEP_WAKE_SOURCES,
        gated_domains: RP2350_SLEEP_GATED_DOMAINS,
    },
    PowerModeDescriptor {
        name: "deep-sleep-wfi",
        depth: PowerModeDepth::DeepSleep,
        wake_sources: RP2350_DEEP_SLEEP_WAKE_SOURCES,
        gated_domains: RP2350_DEEP_SLEEP_GATED_DOMAINS,
    },
];

// NOTE: the current selected RP2350 board contract follows the open Pico 2 W schematic and the
// local PicoTarget linker layout, both of which assume a 32 Mbit / 4 MiB external flash
// population. Raspberry Pi's Pico 2 W prose datasheet currently disagrees and mentions a
// W25Q16JV instead, so this must split into a truly board-specific module if that ambiguity ever
// becomes more than documentation slop.
const FLASH_REGIONS: [CortexMFlashRegionDescriptor; 1] = [CortexMFlashRegionDescriptor {
    name: "qspi-flash-xip",
    base: 0x1000_0000,
    len: RP2350_FLASH_BYTES,
    erase_block_bytes: RP2350_FLASH_ERASE_BLOCK_BYTES,
    program_granule_bytes: RP2350_FLASH_PROGRAM_GRANULE_BYTES,
    xip: true,
    writable: true,
    requires_xip_quiesce: true,
}];

const fn rp2350_owned_sram_region_from_bounds(
    heap_start: usize,
    stack_end: usize,
) -> Option<CortexMMemoryRegionDescriptor> {
    if stack_end <= heap_start {
        return None;
    }

    Some(CortexMMemoryRegionDescriptor {
        name: "board-sram-free",
        kind: CortexMMemoryRegionKind::Sram,
        base: heap_start,
        len: stack_end - heap_start,
        protect: Protect::READ.union(Protect::WRITE),
        attrs: RegionAttrs::STATIC_REGION
            .union(RegionAttrs::DMA_VISIBLE)
            .union(RegionAttrs::CACHEABLE)
            .union(RegionAttrs::COHERENT),
        cache: CachePolicy::Default,
        backing: MemResourceBackingKind::StaticRegion,
        allocatable: true,
    })
}

fn rp2350_owned_sram_region() -> Option<CortexMMemoryRegionDescriptor> {
    let heap_start = (&raw const __sheap) as usize;
    let stack_end = (&raw const _stack_end) as usize;
    rp2350_owned_sram_region_from_bounds(heap_start, stack_end)
}

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

/// TODO: clears both ICER and ICPR - disabling clears pending. Reasonable default,
/// TODO: but can't 'disable but remember pending'.
fn rp2350_irq_disable_line(irqn: u16) -> Result<(), HardwareError> {
    if !rp2350_irq_is_known(irqn) {
        return Err(HardwareError::invalid());
    }

    rp2350_nvic_write(CORTEX_M_NVIC_ICER, irqn);
    rp2350_nvic_write(CORTEX_M_NVIC_ICPR, irqn);
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
    let timerawl = (RP2350_EVENT_TIMEOUT_TIMER_BASE + RP2350_TIMER_TIMERAWL_OFFSET) as *const u32;
    // SAFETY: TIMERAWL is a side-effect-free raw read of the RP2350 timer low word.
    let now = unsafe { ptr::read_volatile(timerawl) };
    now.wrapping_add(delta.max(1))
}

fn rp2350_arm_event_timeout(timeout: Duration) -> Result<(), HardwareError> {
    let deadline = rp2350_event_timeout_deadline(timeout);
    let alarm_bit = 1_u32 << u32::from(RP2350_EVENT_TIMEOUT_ALARM_INDEX);
    let timer_base = RP2350_EVENT_TIMEOUT_TIMER_BASE;
    let interrupt_clear = (timer_base + RP2350_TIMER_INTR_OFFSET) as *mut u32;
    let interrupt_enable = (timer_base + RP2350_TIMER_INTE_OFFSET) as *mut u32;
    let alarm = (timer_base
        + RP2350_TIMER_ALARM0_OFFSET
        + (usize::from(RP2350_EVENT_TIMEOUT_ALARM_INDEX) * 4)) as *mut u32;

    rp2350_irq_enable_line(RP2350_EVENT_TIMEOUT_IRQN)?;

    // SAFETY: these are the RP2350 timer interrupt-clear, interrupt-enable, and alarm registers
    // for the reserved backend timeout alarm.
    unsafe {
        ptr::write_volatile(interrupt_clear, alarm_bit);
        let current_enable = ptr::read_volatile(interrupt_enable);
        ptr::write_volatile(interrupt_enable, current_enable | alarm_bit);
        ptr::write_volatile(alarm, deadline);
    }
    rp2350_nvic_write(CORTEX_M_NVIC_ICPR, RP2350_EVENT_TIMEOUT_IRQN);
    Ok(())
}

fn rp2350_cancel_event_timeout_alarm() -> Result<(), HardwareError> {
    let alarm_bit = 1_u32 << u32::from(RP2350_EVENT_TIMEOUT_ALARM_INDEX);
    let timer_base = RP2350_EVENT_TIMEOUT_TIMER_BASE;
    let armed = (timer_base + RP2350_TIMER_ARMED_OFFSET) as *mut u32;
    let interrupt_clear = (timer_base + RP2350_TIMER_INTR_OFFSET) as *mut u32;
    let interrupt_enable = (timer_base + RP2350_TIMER_INTE_OFFSET) as *mut u32;

    // SAFETY: these are the RP2350 timer armed, interrupt-clear, and interrupt-enable registers
    // for the reserved backend timeout alarm.
    unsafe {
        let current_enable = ptr::read_volatile(interrupt_enable);
        ptr::write_volatile(interrupt_enable, current_enable & !alarm_bit);
        ptr::write_volatile(armed, alarm_bit);
        ptr::write_volatile(interrupt_clear, alarm_bit);
    }
    rp2350_irq_disable_line(RP2350_EVENT_TIMEOUT_IRQN)?;
    Ok(())
}

fn rp2350_event_timeout_fired_now() -> bool {
    let alarm_bit = 1_u32 << u32::from(RP2350_EVENT_TIMEOUT_ALARM_INDEX);
    let ints = (RP2350_EVENT_TIMEOUT_TIMER_BASE + RP2350_TIMER_INTS_OFFSET) as *const u32;
    // SAFETY: TIMERx_INTS is a side-effect-free masked status register for the reserved backend
    // timeout alarm.
    let masked_status = unsafe { ptr::read_volatile(ints) };
    (masked_status & alarm_bit) != 0
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
                cluster: None,
                package: None,
                numa_node: None,
                core_class: None,
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

    fn event_timeout_supported(&self) -> bool {
        true
    }

    fn event_timeout_irq(&self) -> Option<u16> {
        Some(RP2350_EVENT_TIMEOUT_IRQN)
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
mod tests {
    use super::{
        CLK_REF_AUX_SOURCES,
        CLK_REF_MAIN_SOURCES,
        CLOCK_TREE,
        CortexMDmaRequestClass,
        CortexMIrqClass,
        CortexMSocDeviceIdSupport,
        DEVICE_ID_SUPPORT,
        DMA_CONTROLLERS,
        DMA_REQUESTS,
        FLASH_REGIONS,
        IRQS,
        MEMORY_MAP,
        POWER_MODES,
        Rp2350PowerModeAction,
        rp2350_chip_manufacturer,
        rp2350_chip_part,
        rp2350_chip_revision,
        rp2350_gpio_irq_bank,
        rp2350_owned_sram_region_from_bounds,
        rp2350_pio_base_and_irq_index,
        rp2350_power_mode_action,
        rp2350_public_device_id_from_words,
        rp2350_spi_base,
    };

    #[test]
    fn decodes_sysinfo_chip_id_fields() {
        let raw_chip_id = (0xau32 << 28) | (0x2350u32 << 12) | (0x12au32 << 1) | 1u32;

        assert_eq!(rp2350_chip_revision(raw_chip_id), 0x0a);
        assert_eq!(rp2350_chip_part(raw_chip_id), 0x2350);
        assert_eq!(rp2350_chip_manufacturer(raw_chip_id), 0x012a);
    }

    #[test]
    fn clk_ref_clock_descriptor_surfaces_mux_shape() {
        let clk_ref = CLOCK_TREE
            .iter()
            .find(|descriptor| descriptor.name == "clk_ref")
            .expect("clk_ref descriptor should exist");

        assert_eq!(clk_ref.main_sources, CLK_REF_MAIN_SOURCES);
        assert_eq!(clk_ref.aux_sources, CLK_REF_AUX_SOURCES);
    }

    #[test]
    fn sram_region_is_not_marked_allocatable_without_a_board_carveout() {
        let sram = MEMORY_MAP
            .iter()
            .find(|descriptor| descriptor.name == "sram")
            .expect("sram descriptor should exist");

        assert!(!sram.allocatable);
    }

    #[test]
    fn dma_and_power_surfaces_are_exposed_for_rp2350() {
        assert_eq!(DMA_CONTROLLERS[0].channel_count, 16);
        assert_eq!(DMA_REQUESTS[0].name, "pio0-tx0");
        assert_eq!(DMA_REQUESTS[55].name, "dma-timer0");
        assert_eq!(POWER_MODES[0].name, "sleep-wfi");
    }

    #[test]
    fn dma_request_metadata_tracks_endpoint_shape() {
        assert_eq!(DMA_REQUESTS[0].class, CortexMDmaRequestClass::PeripheralTx);
        assert_eq!(DMA_REQUESTS[0].endpoint, Some("tx0"));
        assert_eq!(DMA_REQUESTS[48].class, CortexMDmaRequestClass::PeripheralRx);
        assert_eq!(DMA_REQUESTS[48].endpoint, Some("fifo"));
        assert_eq!(DMA_REQUESTS[55].class, CortexMDmaRequestClass::TimerPacer);
        assert_eq!(DMA_REQUESTS[55].endpoint, Some("timer0"));
        assert_eq!(DMA_REQUESTS[59].class, CortexMDmaRequestClass::Force);
        assert_eq!(DMA_REQUESTS[59].endpoint, None);
    }

    #[test]
    fn irq_surface_covers_rp2350_nvic_lines() {
        assert_eq!(IRQS.len(), 52);
        assert_eq!(IRQS[10].name, "dma-irq0");
        assert_eq!(IRQS[10].irqn, 10);
        assert_eq!(IRQS[10].class, CortexMIrqClass::Dma);
        assert_eq!(IRQS[22].name, "io-bank0-ns");
        assert!(IRQS[22].nonsecure);
        assert_eq!(IRQS[44].name, "powman-pow");
        assert_eq!(IRQS[44].class, CortexMIrqClass::Power);
    }

    #[test]
    fn generic_irq_acknowledge_support_tracks_only_honest_shared_clear_paths() {
        assert!(super::irq_acknowledge_supported(0));
        assert!(super::irq_acknowledge_supported(10));
        assert!(super::irq_acknowledge_supported(33));
        assert!(super::irq_acknowledge_supported(36));
        assert!(!super::irq_acknowledge_supported(15));
        assert!(!super::irq_acknowledge_supported(21));
        assert!(!super::irq_acknowledge_supported(31));
    }

    #[test]
    fn gpio_shared_summary_helpers_keep_bank_shape_honest() {
        assert_eq!(
            rp2350_gpio_irq_bank(21),
            Some((
                RP2350_IO_BANK0_BASE,
                RP2350_IO_BANK0_INTR0_OFFSET,
                RP2350_GPIO_BANK0_SUMMARY_WORDS,
            ))
        );
        assert_eq!(
            rp2350_gpio_irq_bank(23),
            Some((
                RP2350_IO_QSPI_BASE,
                RP2350_IO_QSPI_INTR_OFFSET,
                RP2350_GPIO_QSPI_SUMMARY_WORDS,
            ))
        );
        assert_eq!(rp2350_gpio_irq_bank(31), None);

        let summary = Rp2350GpioIrqSummary {
            word_count: 1,
            words: [0x0000_00c9, 0, 0, 0, 0, 0],
        };
        assert_eq!(summary.word_count(), 1);
        assert_eq!(summary.word(0), Some(0x0000_00c9));
        assert_eq!(summary.word(1), None);
        assert_eq!(summary.line_events(0), Some(0x9));
        assert_eq!(summary.line_events(1), Some(0xc));
        assert_eq!(summary.line_events(8), None);
    }

    #[test]
    fn pio_shared_summary_helpers_split_internal_and_fifo_causes() {
        assert_eq!(
            rp2350_pio_base_and_irq_index(15),
            Some((RP2350_PIO0_BASE, 0))
        );
        assert_eq!(
            rp2350_pio_base_and_irq_index(20),
            Some((RP2350_PIO2_BASE, 1))
        );
        assert_eq!(rp2350_pio_base_and_irq_index(31), None);

        let summary = Rp2350PioIrqSummary { raw: 0xa53c };
        assert_eq!(summary.raw(), 0xa53c);
        assert_eq!(summary.internal_irq_flags(), 0xa5);
        assert_eq!(summary.tx_not_full_mask(), 0x3);
        assert_eq!(summary.rx_not_empty_mask(), 0xc);
    }

    #[test]
    fn spi_shared_summary_helpers_only_claim_clearable_rt_ror_bits() {
        assert_eq!(rp2350_spi_base(31), Some(RP2350_SPI0_BASE));
        assert_eq!(rp2350_spi_base(32), Some(RP2350_SPI1_BASE));
        assert_eq!(rp2350_spi_base(33), None);

        let summary = Rp2350SpiIrqSummary { raw: 0x0f };
        assert!(summary.tx());
        assert!(summary.rx());
        assert!(summary.receive_timeout());
        assert!(summary.receive_overrun());
        assert_eq!(summary.clearable_mask(), 0x03);
    }

    #[test]
    fn public_device_id_words_pack_little_endian() {
        let identity = rp2350_public_device_id_from_words([0x0123, 0x4567, 0x89ab, 0xcdef]);
        assert_eq!(identity, 0xcdef_89ab_4567_0123);
    }

    #[test]
    fn device_identity_surface_is_marked_otp_readable() {
        assert_eq!(DEVICE_ID_SUPPORT, CortexMSocDeviceIdSupport::OtpReadable);
    }

    #[test]
    fn rp2350_power_mode_names_map_to_actions() {
        assert_eq!(
            rp2350_power_mode_action("sleep-wfi"),
            Some(Rp2350PowerModeAction::SleepWfi)
        );
        assert_eq!(
            rp2350_power_mode_action("deep-sleep-wfi"),
            Some(Rp2350PowerModeAction::DeepSleepWfi)
        );
        assert_eq!(rp2350_power_mode_action("nonsense"), None);
    }

    #[test]
    fn flash_surface_tracks_selected_board_geometry() {
        assert_eq!(FLASH_REGIONS[0].name, "qspi-flash-xip");
        assert_eq!(FLASH_REGIONS[0].base, 0x1000_0000);
        assert_eq!(FLASH_REGIONS[0].len, 4 * 1024 * 1024);
        assert_eq!(FLASH_REGIONS[0].erase_block_bytes, 4 * 1024);
        assert_eq!(FLASH_REGIONS[0].program_granule_bytes, 256);
        assert!(FLASH_REGIONS[0].xip);
        assert!(FLASH_REGIONS[0].writable);
        assert!(FLASH_REGIONS[0].requires_xip_quiesce);
    }

    #[test]
    fn owned_sram_region_requires_a_real_gap_between_heap_and_stack() {
        assert!(rp2350_owned_sram_region_from_bounds(0x2000_1000, 0x2000_1000).is_none());
        assert!(rp2350_owned_sram_region_from_bounds(0x2000_1004, 0x2000_1000).is_none());

        let region = rp2350_owned_sram_region_from_bounds(0x2000_1000, 0x2000_9000)
            .expect("owned region should be surfaced when the board reserves a stack gap");
        assert_eq!(region.name, "board-sram-free");
        assert_eq!(region.base, 0x2000_1000);
        assert_eq!(region.len, 0x8000);
        assert!(region.allocatable);
    }
}
