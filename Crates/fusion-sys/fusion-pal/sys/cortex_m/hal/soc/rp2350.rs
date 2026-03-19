#![allow(clippy::doc_markdown)]

//! RP2350 Cortex-M SoC descriptor.
//!
//! This module is where verified RP2350 memory-map, peripheral, and clock-tree facts belong.
//! The current implementation wires the architected topology, the major static memory regions,
//! the major peripheral blocks, and the board-visible clock domains from the RP2350 datasheet
//! and Pico SDK clock model.

use core::ptr;

use crate::pal::hal::{
    HardwareAuthoritySet, HardwareError, HardwareTopologySummary, HardwareWriteSummary,
};
use crate::pal::mem::{CachePolicy, MemResourceBackingKind, Protect, RegionAttrs};
use crate::pal::thread::{
    ThreadAuthoritySet, ThreadCoreId, ThreadError, ThreadExecutionLocation, ThreadId,
    ThreadLogicalCpuId, ThreadProcessorGroupId,
};

use super::board_contract::{self, CortexMSocBoard};

pub use super::board_contract::{
    CortexMClockDescriptor, CortexMMemoryRegionDescriptor, CortexMMemoryRegionKind,
    CortexMPeripheralBus, CortexMPeripheralDescriptor, CortexMSocBoard as CortexMSoc,
    CortexMSocChipIdSupport, CortexMSocChipIdentity, CortexMSocDescriptor,
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
const RP2350_SIO_CPUID: *const u32 = 0xd000_0000 as *const u32;

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
        allocatable: true,
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

impl CortexMSocBoard for Rp2350Soc {
    fn descriptor(&self) -> CortexMSocDescriptor {
        DESCRIPTOR
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

    fn memory_map(&self) -> &'static [CortexMMemoryRegionDescriptor] {
        &MEMORY_MAP
    }

    fn peripherals(&self) -> &'static [CortexMPeripheralDescriptor] {
        &PERIPHERALS
    }

    fn clock_tree(&self) -> &'static [CortexMClockDescriptor] {
        &CLOCK_TREE
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

/// Returns the selected RP2350 memory map.
#[must_use]
pub fn memory_map() -> &'static [CortexMMemoryRegionDescriptor] {
    board_contract::memory_map(system_soc())
}

/// Returns the selected RP2350 peripheral descriptors.
#[must_use]
pub fn peripherals() -> &'static [CortexMPeripheralDescriptor] {
    board_contract::peripherals(system_soc())
}

/// Returns the selected RP2350 clock-tree descriptors.
#[must_use]
pub fn clock_tree() -> &'static [CortexMClockDescriptor] {
    board_contract::clock_tree(system_soc())
}

#[cfg(test)]
mod tests {
    use super::{
        CLK_REF_AUX_SOURCES, CLK_REF_MAIN_SOURCES, CLOCK_TREE, rp2350_chip_manufacturer,
        rp2350_chip_part, rp2350_chip_revision,
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
}
