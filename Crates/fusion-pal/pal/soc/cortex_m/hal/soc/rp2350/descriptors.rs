use super::*;
use fusion_hal::contract::drivers::bus::gpio::GpioSignalSource;

/// Compile-time descriptor for the RP2350 SoC family.
pub const DESCRIPTOR: CortexMSocDescriptor = CortexMSocDescriptor {
    name: "rp2350",
    topology_summary: Some(HardwareTopologySummary {
        logical_cpu_count: Some(2),
        core_count: Some(2),
        cluster_count: Some(1),
        package_count: Some(1),
        numa_node_count: None,
        core_class_count: Some(1),
    }),
    topology_authorities: HardwareAuthoritySet::TOPOLOGY,
    chip_id_support: CortexMSocChipIdSupport::RegisterReadable,
};

/// Whether local interrupt masking is sufficient to serialize local synchronization on RP2350.
pub const LOCAL_CRITICAL_SECTION_SYNC_SAFE: bool = false;
/// Runtime per-device identity support class for RP2350 boards.
pub const DEVICE_ID_SUPPORT: CortexMSocDeviceIdSupport = CortexMSocDeviceIdSupport::OtpReadable;

pub(crate) const APB_SLOT_BYTES: usize = 0x0000_8000;
pub(crate) const AHB_SLOT_BYTES: usize = 0x0010_0000;
pub(crate) const ROM_BYTES: usize = 32 * 1024;
pub(crate) const XIP_WINDOW_BYTES: usize = 32 * 1024 * 1024;
pub(crate) const SRAM_BYTES: usize = 0x0008_2000;
pub(crate) const APB_SEGMENT_BYTES: usize = 0x0016_8000;
pub(crate) const AHB_SEGMENT_BYTES: usize = 0x0080_0000;
pub(crate) const SIO_SEGMENT_BYTES: usize = 0x0004_0000;
pub(crate) const PPB_SEGMENT_BYTES: usize = 0x000A_0000;

pub(crate) const RP2350_SYSINFO_CHIP_ID: *const u32 = 0x4000_0000 as *const u32;
pub(crate) const RP2350_SYSINFO_PLATFORM: *const u32 = 0x4000_0008 as *const u32;
pub(crate) const RP2350_SYSINFO_GITREF_RP2350: *const u32 = 0x4000_0014 as *const u32;
pub(crate) const RP2350_SYSINFO_CHIP_INFO: *const u32 = 0x4000_0018 as *const u32;
pub(crate) const RP2350_OTP_DATA: *const u32 = 0x4013_0000 as *const u32;
pub(crate) const RP2350_SIO_CPUID: *const u32 = 0xd000_0000 as *const u32;
pub(crate) const RP2350_SIO_BASE: usize = 0xd000_0000;
pub(crate) const CORTEX_M_SCB_SCR: *mut u32 = 0xE000_ED10 as *mut u32;
pub(crate) const CORTEX_M_SCB_SCR_SLEEPDEEP: u32 = 1 << 2;
pub(crate) const CORTEX_M_NVIC_ISER: *mut u32 = 0xE000_E100 as *mut u32;
pub(crate) const CORTEX_M_NVIC_ICER: *mut u32 = 0xE000_E180 as *mut u32;
pub(crate) const CORTEX_M_NVIC_ISPR: *mut u32 = 0xE000_E200 as *mut u32;
pub(crate) const CORTEX_M_NVIC_ICPR: *mut u32 = 0xE000_E280 as *mut u32;
pub(crate) const CORTEX_M_NVIC_IPR: *mut u8 = 0xE000_E400 as *mut u8;
pub(crate) const RP2350_TICKS_BASE: usize = 0x4010_8000;
pub(crate) const RP2350_TIMER0_BASE: usize = 0x400b_0000;
pub(crate) const RP2350_TIMER1_BASE: usize = 0x400b_8000;
pub(crate) const RP2350_CLOCKS_BASE: usize = 0x4001_0000;
pub(crate) const RP2350_RESETS_BASE: usize = 0x4002_0000;
pub(crate) const RP2350_IO_BANK0_BASE: usize = 0x4002_8000;
pub(crate) const RP2350_IO_QSPI_BASE: usize = 0x4003_0000;
pub(crate) const RP2350_PADS_BANK0_BASE: usize = 0x4003_8000;
pub(crate) const RP2350_XOSC_BASE: usize = 0x4004_8000;
pub(crate) const RP2350_PLL_SYS_BASE: usize = 0x4005_0000;
pub(crate) const RP2350_PLL_USB_BASE: usize = 0x4005_8000;
pub(crate) const RP2350_DMA_BASE: usize = 0x5000_0000;
pub(crate) const RP2350_SPI0_BASE: usize = 0x4008_0000;
pub(crate) const RP2350_SPI1_BASE: usize = 0x4008_8000;
pub(crate) const RP2350_UART0_BASE: usize = 0x4007_0000;
pub(crate) const RP2350_UART1_BASE: usize = 0x4007_8000;
pub(crate) const RP2350_I2C0_BASE: usize = 0x4009_0000;
pub(crate) const RP2350_I2C1_BASE: usize = 0x4009_8000;
pub(crate) const RP2350_PIO0_BASE: usize = 0x5020_0000;
pub(crate) const RP2350_PIO1_BASE: usize = 0x5030_0000;
pub(crate) const RP2350_PIO2_BASE: usize = 0x5040_0000;
pub(crate) const RP2350_PIO_ENGINE_COUNT: usize = 3;
pub(crate) const RP2350_PIO_LANES_PER_ENGINE: usize = 4;
pub(crate) const RP2350_PIO_FIFO_DEPTH_WORDS: u8 = 4;
pub(crate) const RP2350_PIO_INSTRUCTION_WORDS: u16 = 32;
pub(crate) const RP2350_REG_ALIAS_SET_OFFSET: usize = 0x2000;
pub(crate) const RP2350_REG_ALIAS_CLR_OFFSET: usize = 0x3000;
pub(crate) const RP2350_PIO_CTRL_OFFSET: usize = 0x00;
pub(crate) const RP2350_PIO_FSTAT_OFFSET: usize = 0x04;
pub(crate) const RP2350_PIO_FDEBUG_OFFSET: usize = 0x08;
pub(crate) const RP2350_PIO_TXF0_OFFSET: usize = 0x10;
pub(crate) const RP2350_PIO_RXF0_OFFSET: usize = 0x20;
pub(crate) const RP2350_PIO_INSTR_MEM0_OFFSET: usize = 0x48;
pub(crate) const RP2350_PIO_SM_STRIDE: usize = 0x18;
pub(crate) const RP2350_PIO_SM0_CLKDIV_OFFSET: usize = 0xc8;
pub(crate) const RP2350_PIO_SM0_EXECCTRL_OFFSET: usize = 0xcc;
pub(crate) const RP2350_PIO_SM0_SHIFTCTRL_OFFSET: usize = 0xd0;
pub(crate) const RP2350_PIO_SM0_INSTR_OFFSET: usize = 0xd8;
pub(crate) const RP2350_PIO_SM0_PINCTRL_OFFSET: usize = 0xdc;
pub(crate) const RP2350_PIO_CTRL_SM_ENABLE_MASK: u32 = 0x0000_000f;
pub(crate) const RP2350_PIO_CTRL_SM_RESTART_SHIFT: u32 = 4;
pub(crate) const RP2350_PIO_CTRL_CLKDIV_RESTART_SHIFT: u32 = 8;
pub(crate) const RP2350_PIO_FSTAT_TXFULL_SHIFT: u32 = 16;
pub(crate) const RP2350_PIO_FSTAT_RXEMPTY_SHIFT: u32 = 8;
pub(crate) const RP2350_PIO_FDEBUG_TXSTALL_SHIFT: u32 = 24;
pub(crate) const RP2350_PIO_FDEBUG_TXOVER_SHIFT: u32 = 16;
pub(crate) const RP2350_PIO_FDEBUG_RXUNDER_SHIFT: u32 = 8;
pub(crate) const RP2350_PIO_FDEBUG_RXSTALL_SHIFT: u32 = 0;
pub(crate) const RP2350_PIO_SM_SHIFTCTRL_FJOIN_RX_BIT: u32 = 1 << 31;
pub(crate) const RP2350_PIO_VALID_LANE_MASK: u8 = 0x0f;
pub(crate) const RP2350_RESETS_RESET_OFFSET: usize = 0x00;
pub(crate) const RP2350_RESETS_RESET_DONE_OFFSET: usize = 0x08;
pub(crate) const RP2350_RESETS_PIO0_BIT: u32 = 0x0000_0800;
pub(crate) const RP2350_RESETS_PIO1_BIT: u32 = 0x0000_1000;
pub(crate) const RP2350_RESETS_PIO2_BIT: u32 = 0x0000_2000;
pub(crate) const RP2350_EVENT_TIMEOUT_TIMER_BASE: usize = RP2350_TIMER0_BASE;
pub(crate) const RP2350_EVENT_TIMEOUT_ALARM_INDEX: u16 = 3;
pub(crate) const RP2350_EVENT_TIMEOUT_IRQN: u16 = 3;
pub(crate) const RP2350_EVENT_TIMEOUT_TICK_HZ: u64 = 1_000_000;
pub(crate) const RP2350_EVENT_TIMEOUT_COUNTER_BITS: u32 = 32;
pub(crate) const RP2350_EVENT_TIMEOUT_MAX_RELATIVE_TIMEOUT: Duration =
    Duration::from_micros(u32::MAX as u64);
pub(crate) const RP2350_CLK_REF_HZ: u32 = 12_000_000;
pub(crate) const RP2350_TIMER_TICK_CYCLES: u32 =
    RP2350_CLK_REF_HZ / RP2350_EVENT_TIMEOUT_TICK_HZ as u32;
pub(crate) const RP2350_TICKS_TIMER0_CTRL_OFFSET: usize = 0x18;
pub(crate) const RP2350_TICKS_TIMER0_CYCLES_OFFSET: usize = 0x1c;
pub(crate) const RP2350_TICKS_CTRL_ENABLE: u32 = 1 << 0;
pub(crate) const RP2350_TICKS_CTRL_RUNNING: u32 = 1 << 1;

pub(crate) const RP2350_EVENT_TIMEOUT_SUPPORT: CortexMEventTimeoutSupport =
    CortexMEventTimeoutSupport {
        implementation: CortexMEventTimeoutImplementation::ReservedOneShotAlarm,
        irqn: Some(RP2350_EVENT_TIMEOUT_IRQN),
        counter_bits: Some(RP2350_EVENT_TIMEOUT_COUNTER_BITS),
        tick_hz: Some(RP2350_EVENT_TIMEOUT_TICK_HZ),
        max_relative_timeout: Some(RP2350_EVENT_TIMEOUT_MAX_RELATIVE_TIMEOUT),
    };
pub(crate) const RP2350_INLINE_EXCEPTION_STACK_RESERVE_BYTES: usize = 128;
pub(crate) const RP2350_IO_BANK0_INTR0_OFFSET: usize = 0x230;
pub(crate) const RP2350_IO_QSPI_INTR_OFFSET: usize = 0x218;
pub(crate) const RP2350_IO_IRQ_WORD_STRIDE: usize = 0x4;
pub(crate) const RP2350_GPIO_BANK0_SUMMARY_WORDS: usize = 6;
pub(crate) const RP2350_GPIO_QSPI_SUMMARY_WORDS: usize = 1;
pub(crate) const RP2350_GPIO_EDGE_EVENT_MASK: u32 = 0xCCCC_CCCC;
pub(crate) const RP2350_TIMER_ALARM0_OFFSET: usize = 0x10;
pub(crate) const RP2350_TIMER_ARMED_OFFSET: usize = 0x20;
pub(crate) const RP2350_TIMER_TIMERAWH_OFFSET: usize = 0x24;
pub(crate) const RP2350_TIMER_TIMERAWL_OFFSET: usize = 0x28;
pub(crate) const RP2350_TIMER_INTR_OFFSET: usize = 0x3c;
pub(crate) const RP2350_TIMER_INTE_OFFSET: usize = 0x40;
pub(crate) const RP2350_TIMER_INTS_OFFSET: usize = 0x48;
pub(crate) const RP2350_DMA_INTS0_OFFSET: usize = 0x40c;
pub(crate) const RP2350_SPI_SSPMIS_OFFSET: usize = 0x1c;
pub(crate) const RP2350_SPI_SSPICR_OFFSET: usize = 0x20;
pub(crate) const RP2350_SPI_SSPICR_CLEARABLE_MASK: u32 = 0x3;
pub(crate) const RP2350_UARTMIS_OFFSET: usize = 0x40;
pub(crate) const RP2350_UARTICR_OFFSET: usize = 0x44;
pub(crate) const RP2350_UARTICR_CLEARABLE_BITS: u32 = 0x0000_07ff;
pub(crate) const RP2350_I2C_IC_INTR_STAT_OFFSET: usize = 0x2c;
pub(crate) const RP2350_I2C_IC_CLR_INTR_OFFSET: usize = 0x40;
pub(crate) const RP2350_PIO_IRQ_OFFSET: usize = 0x30;
pub(crate) const RP2350_PIO_IRQ0_INTS_OFFSET: usize = 0x178;
pub(crate) const RP2350_PIO_IRQ1_INTS_OFFSET: usize = 0x184;
pub(crate) const RP2350_BOOT_CLOCK_STATE_UNINITIALIZED: u8 = 0;
pub(crate) const RP2350_BOOT_CLOCK_STATE_INITIALIZING: u8 = 1;
pub(crate) const RP2350_BOOT_CLOCK_STATE_READY: u8 = 2;
pub(crate) const RP2350_TIMER0_TICK_STATE_UNINITIALIZED: u8 = 0;
pub(crate) const RP2350_TIMER0_TICK_STATE_INITIALIZING: u8 = 1;
pub(crate) const RP2350_TIMER0_TICK_STATE_READY: u8 = 2;
pub(crate) const RP2350_XOSC_HZ: u32 = 12_000_000;
pub(crate) const RP2350_DEFAULT_SYS_CLOCK_HZ: u32 = 150_000_000;

unsafe extern "C" {
    static __sheap: u8;
    static _stack_end: u8;
    static _stack_start: u8;
}

pub(crate) const CLK_REF_MAIN_SOURCES: &[&str] = &[
    "rosc_clkr_ref",
    "clksrc_clk_ref_aux",
    "xosc_clkr_ref",
    "lposc_clkr_ref",
];
pub(crate) const CLK_REF_AUX_SOURCES: &[&str] =
    &["clksrc_gpin0", "clksrc_gpin1", "pll_usb_clkr_ref"];
pub(crate) const CLK_REF_CONSUMERS: &[&str] = &["otp", "powman", "ticks"];
pub(crate) const CLK_SYS_MAIN_SOURCES: &[&str] = &["clksrc_clk_sys_aux", "clk_ref"];
pub(crate) const CLK_SYS_AUX_SOURCES: &[&str] = &[
    "clksrc_pll_sys",
    "clksrc_gpin0",
    "clksrc_gpin1",
    "clksrc_pll_usb",
    "rosc_clkr_sys",
    "xosc_clkr_sys",
];
pub(crate) const CLK_SYS_CONSUMERS: &[&str] = &[
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
pub(crate) const CLK_PERI_MAIN_SOURCES: &[&str] = &[];
pub(crate) const CLK_PERI_AUX_SOURCES: &[&str] = &[
    "clksrc_pll_sys",
    "clksrc_gpin0",
    "clksrc_gpin1",
    "clksrc_pll_usb",
    "rosc_clksrc_ph",
    "xosc_clksrc",
    "clk_sys",
];
pub(crate) const CLK_PERI_CONSUMERS: &[&str] = &["uart0", "uart1", "spi0", "spi1", "i2c0", "i2c1"];
pub(crate) const CLK_HSTX_MAIN_SOURCES: &[&str] = &[];
pub(crate) const CLK_HSTX_AUX_SOURCES: &[&str] = &["clksrc_pll_sys", "clksrc_pll_usb", "clk_sys"];
pub(crate) const CLK_HSTX_CONSUMERS: &[&str] = &["hstx"];
pub(crate) const CLK_USB_MAIN_SOURCES: &[&str] = &[];
pub(crate) const CLK_USB_AUX_SOURCES: &[&str] = &[
    "clksrc_pll_sys",
    "clksrc_gpin0",
    "clksrc_gpin1",
    "clksrc_pll_usb",
    "rosc_clksrc_ph",
    "xosc_clksrc",
];
pub(crate) const CLK_USB_CONSUMERS: &[&str] = &["usbctrl"];
pub(crate) const CLK_ADC_MAIN_SOURCES: &[&str] = &[];
pub(crate) const CLK_ADC_AUX_SOURCES: &[&str] = &[
    "clksrc_pll_sys",
    "clksrc_gpin0",
    "clksrc_gpin1",
    "clksrc_pll_usb",
    "rosc_clksrc_ph",
    "xosc_clksrc",
];

pub(crate) const RP2350_OVERCLOCK_PROFILES: &[CortexMSocOverclockProfile] = &[
    CortexMSocOverclockProfile {
        name: "stock-150mhz",
        sys_clock_hz: 150_000_000,
        monotonic_time_impact: CortexMSocMonotonicTimeImpact::Unknown,
    },
    CortexMSocOverclockProfile {
        name: "oc-200mhz",
        sys_clock_hz: 200_000_000,
        monotonic_time_impact: CortexMSocMonotonicTimeImpact::Unknown,
    },
    CortexMSocOverclockProfile {
        name: "oc-250mhz",
        sys_clock_hz: 250_000_000,
        monotonic_time_impact: CortexMSocMonotonicTimeImpact::Unknown,
    },
    CortexMSocOverclockProfile {
        name: "oc-300mhz",
        sys_clock_hz: 300_000_000,
        monotonic_time_impact: CortexMSocMonotonicTimeImpact::Unknown,
    },
];
pub(crate) const CLK_ADC_CONSUMERS: &[&str] = &["adc"];
pub(crate) const RP2350_SLEEP_WAKE_SOURCES: &[&str] = &["irq", "sev", "timer", "gpio"];
pub(crate) const RP2350_SLEEP_GATED_DOMAINS: &[&str] = &["core pipeline"];
pub(crate) const RP2350_DEEP_SLEEP_WAKE_SOURCES: &[&str] = &["irq", "gpio", "timer"];
pub(crate) const RP2350_DEEP_SLEEP_GATED_DOMAINS: &[&str] =
    &["clk_sys", "clk_peri", "core pipeline"];
pub(crate) const RP2350_FLASH_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const RP2350_FLASH_ERASE_BLOCK_BYTES: usize = 4 * 1024;
pub(crate) const RP2350_FLASH_PROGRAM_GRANULE_BYTES: usize = 256;
pub(crate) const RP2350_PIO_PIN_MAPPING: PcuPinMappingCaps = PcuPinMappingCaps::INPUT_BASE
    .union(PcuPinMappingCaps::OUTPUT_BASE)
    .union(PcuPinMappingCaps::SET_BASE)
    .union(PcuPinMappingCaps::SIDESET_BASE)
    .union(PcuPinMappingCaps::JMP_PIN);
pub(crate) const RP2350_PIO_ENGINE_CAPS: PcuCaps = PcuCaps::SHARED_INSTRUCTION_MEMORY
    .union(PcuCaps::PER_LANE_PROGRAM_COUNTER)
    .union(PcuCaps::LANE_SIDESET)
    .union(PcuCaps::WAIT_ON_PIN)
    .union(PcuCaps::IRQ_SIGNAL)
    .union(PcuCaps::BIDIRECTIONAL_SHIFT)
    .union(PcuCaps::AUTOPULL)
    .union(PcuCaps::AUTOPUSH)
    .union(PcuCaps::DMA_FEED)
    .union(PcuCaps::PROGRAM_SWAP_REQUIRES_STOP)
    .union(PcuCaps::MULTI_LANE_COOPERATIVE_START)
    .union(PcuCaps::PIN_MAPPING_FLEXIBLE);
pub(crate) const RP2350_PIO_SYSTEM_CAPS: PcuCaps = PcuCaps::ENUMERATE
    .union(PcuCaps::CLAIM_ENGINE)
    .union(PcuCaps::CLAIM_LANES)
    .union(PcuCaps::LOAD_PROGRAM)
    .union(PcuCaps::CONTROL)
    .union(PcuCaps::FIFO_IO)
    .union(RP2350_PIO_ENGINE_CAPS);
pub(crate) const RP2350_PIO_SUPPORT: PcuSupport = PcuSupport {
    caps: RP2350_PIO_SYSTEM_CAPS,
    implementation: PcuImplementationKind::Native,
    engine_count: RP2350_PIO_ENGINE_COUNT as u8,
};

pub(crate) static RP2350_PIO_ENGINE_CLAIMS: [AtomicBool; RP2350_PIO_ENGINE_COUNT] =
    [const { AtomicBool::new(false) }; RP2350_PIO_ENGINE_COUNT];
pub(crate) static RP2350_PIO_LANE_CLAIMS: [AtomicU8; RP2350_PIO_ENGINE_COUNT] =
    [const { AtomicU8::new(0) }; RP2350_PIO_ENGINE_COUNT];
pub(crate) static RP2350_BOOT_CLOCK_STATE: AtomicU8 =
    AtomicU8::new(RP2350_BOOT_CLOCK_STATE_UNINITIALIZED);
pub(crate) static RP2350_ACTIVE_SYS_CLOCK_HZ: AtomicU32 = AtomicU32::new(RP2350_XOSC_HZ);
pub(crate) static RP2350_TIMER0_TICK_STATE: AtomicU8 =
    AtomicU8::new(RP2350_TIMER0_TICK_STATE_UNINITIALIZED);
pub(crate) static RP2350_EVENT_TIMEOUT_FIRED: AtomicBool = AtomicBool::new(false);

pub(crate) const RP2350_PIO0_IRQ_LINES: [u16; 2] = [15, 16];
pub(crate) const RP2350_PIO1_IRQ_LINES: [u16; 2] = [17, 18];
pub(crate) const RP2350_PIO2_IRQ_LINES: [u16; 2] = [19, 20];

pub(crate) const fn rp2350_pio_fifo(
    lane: PcuLaneId,
    direction: PcuFifoDirection,
) -> PcuFifoDescriptor {
    PcuFifoDescriptor {
        id: PcuFifoId { lane, direction },
        depth_words: RP2350_PIO_FIFO_DEPTH_WORDS,
        word_bits: 32,
    }
}

pub(crate) const fn rp2350_pio_lane(
    engine: u8,
    index: u8,
    name: &'static str,
) -> PcuLaneDescriptor {
    let lane = PcuLaneId {
        engine: PcuEngineId(engine),
        index,
    };
    PcuLaneDescriptor {
        id: lane,
        name,
        tx_fifo: rp2350_pio_fifo(lane, PcuFifoDirection::Tx),
        rx_fifo: rp2350_pio_fifo(lane, PcuFifoDirection::Rx),
        pin_mapping: RP2350_PIO_PIN_MAPPING,
    }
}

pub(crate) const fn rp2350_pio_engine(
    engine: u8,
    name: &'static str,
    irq_lines: &'static [u16],
    tx_dreq_base: u16,
    rx_dreq_base: u16,
) -> PcuEngineDescriptor {
    PcuEngineDescriptor {
        id: PcuEngineId(engine),
        name,
        lane_count: RP2350_PIO_LANES_PER_ENGINE as u8,
        instruction_memory: PcuInstructionMemoryDescriptor {
            word_count: RP2350_PIO_INSTRUCTION_WORDS,
            word_bits: 16,
            shared_across_lanes: true,
        },
        clocking: PcuClockDescriptor {
            uses_system_clock: true,
            fractional_divider: true,
        },
        caps: RP2350_PIO_ENGINE_CAPS,
        irq_lines,
        tx_dreq_base: Some(tx_dreq_base),
        rx_dreq_base: Some(rx_dreq_base),
    }
}

pub(crate) const PIO_ENGINES: [PcuEngineDescriptor; RP2350_PIO_ENGINE_COUNT] = [
    rp2350_pio_engine(0, "pio0", &RP2350_PIO0_IRQ_LINES, 0, 4),
    rp2350_pio_engine(1, "pio1", &RP2350_PIO1_IRQ_LINES, 8, 12),
    rp2350_pio_engine(2, "pio2", &RP2350_PIO2_IRQ_LINES, 16, 20),
];

pub(crate) const PIO0_LANES: [PcuLaneDescriptor; RP2350_PIO_LANES_PER_ENGINE] = [
    rp2350_pio_lane(0, 0, "pio0-sm0"),
    rp2350_pio_lane(0, 1, "pio0-sm1"),
    rp2350_pio_lane(0, 2, "pio0-sm2"),
    rp2350_pio_lane(0, 3, "pio0-sm3"),
];
pub(crate) const PIO1_LANES: [PcuLaneDescriptor; RP2350_PIO_LANES_PER_ENGINE] = [
    rp2350_pio_lane(1, 0, "pio1-sm0"),
    rp2350_pio_lane(1, 1, "pio1-sm1"),
    rp2350_pio_lane(1, 2, "pio1-sm2"),
    rp2350_pio_lane(1, 3, "pio1-sm3"),
];
pub(crate) const PIO2_LANES: [PcuLaneDescriptor; RP2350_PIO_LANES_PER_ENGINE] = [
    rp2350_pio_lane(2, 0, "pio2-sm0"),
    rp2350_pio_lane(2, 1, "pio2-sm1"),
    rp2350_pio_lane(2, 2, "pio2-sm2"),
    rp2350_pio_lane(2, 3, "pio2-sm3"),
];

pub(crate) const MEMORY_MAP: [CortexMMemoryRegionDescriptor; 8] = [
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

pub(crate) const PERIPHERALS: [CortexMPeripheralDescriptor; 45] = [
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

pub(crate) const fn irq(
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

pub(crate) const IRQS: [CortexMIrqDescriptor; 52] = [
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

pub(crate) const DMA_CONTROLLERS: [CortexMDmaControllerDescriptor; 1] =
    [CortexMDmaControllerDescriptor {
        name: "dma",
        base: 0x5000_0000,
        channel_count: 16,
        transfer_caps: CortexMDmaTransferCaps::MEMORY_TO_MEMORY
            .union(CortexMDmaTransferCaps::MEMORY_TO_PERIPHERAL)
            .union(CortexMDmaTransferCaps::PERIPHERAL_TO_MEMORY)
            .union(CortexMDmaTransferCaps::CHANNEL_CHAINING),
    }];

pub(crate) const DMA_PERIPHERAL_TX_CAPS: CortexMDmaTransferCaps =
    CortexMDmaTransferCaps::MEMORY_TO_PERIPHERAL;
pub(crate) const DMA_PERIPHERAL_RX_CAPS: CortexMDmaTransferCaps =
    CortexMDmaTransferCaps::PERIPHERAL_TO_MEMORY;
pub(crate) const DMA_PACER_CAPS: CortexMDmaTransferCaps = CortexMDmaTransferCaps::MEMORY_TO_MEMORY
    .union(CortexMDmaTransferCaps::MEMORY_TO_PERIPHERAL)
    .union(CortexMDmaTransferCaps::PERIPHERAL_TO_MEMORY);

pub(crate) const fn dma_request(
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

pub(crate) const DMA_REQUESTS: [CortexMDmaRequestDescriptor; 60] = [
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

pub(crate) const CLOCK_TREE: [CortexMClockDescriptor; 6] = [
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

pub(crate) const POWER_MODES: [CortexMPowerModeDescriptor; 2] = [
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

pub(crate) const PAL_POWER_MODES: [PowerModeDescriptor; 2] = [
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

/// Pico 2 W board-reserved RP2350 GPIO pins consumed by the onboard CYW43439 wiring.
pub(crate) const RP2350_PICO2W_RESERVED_GPIO_PINS: [u8; 4] = [23, 24, 25, 29];
pub(crate) const RP2350_PICO2W_USB_DEVICE_VBUS_DETECT_SOURCE: CortexMUsbDeviceVbusDetectSource =
    CortexMUsbDeviceVbusDetectSource::GpioSignal(GpioSignalSource {
        controller_id: super::drivers::bus::gpio::CYW43439_WL_GPIO_CONTROLLER_ID,
        pin: 2,
    });
pub(crate) const RP2350_PICO2W_CYW43439_CLOCK: CortexMControllerClockProfile =
    CortexMControllerClockProfile {
        reference_clock_hz: Some(37_400_000),
        sleep_clock_hz: None,
    };
pub(crate) const RP2350_PICO2W_CYW43439_BLUETOOTH_ASSETS: CortexMBluetoothControllerAssets =
    CortexMBluetoothControllerAssets {
        patch: CortexMControllerAssetSource::EmbeddedByImplementation {
            name: "cyw43_btfw_43439.bin",
        },
    };
pub(crate) const RP2350_PICO2W_CYW43439_WIFI_ASSETS: CortexMWifiControllerAssets =
    CortexMWifiControllerAssets {
        firmware: CortexMControllerAssetSource::EmbeddedByImplementation {
            name: "w43439A0_7_95_49_00_combined.bin | wb43439A0_7_95_49_00_combined.bin",
        },
        nvram: CortexMControllerAssetSource::EmbeddedByImplementation {
            name: "wifi_nvram_43439.bin",
        },
        clm: CortexMControllerAssetSource::EmbeddedByImplementation {
            name: "combined CYW43439 Wi-Fi image tail",
        },
    };

/// Board-visible Bluetooth controller bindings for the current RP2350 / Pico 2 W contract.
pub(crate) const BLUETOOTH_CONTROLLERS: [CortexMBluetoothControllerBinding; 1] =
    [CortexMBluetoothControllerBinding {
        name: "pico2w-cyw43439",
        vendor: "infineon",
        chip: "CYW43439",
        transport: CortexMBluetoothTransportBinding::Spi3WireSharedDataIrq {
            clock_gpio: 29,
            chip_select_gpio: 25,
            data_irq_gpio: 24,
            target_clock_hz: Some(31_250_000),
        },
        power_gpio: Some(23),
        reset_gpio: None,
        wake_gpio: None,
        activity_gpio: Some(0),
        clock: RP2350_PICO2W_CYW43439_CLOCK,
        assets: RP2350_PICO2W_CYW43439_BLUETOOTH_ASSETS,
    }];

/// Board-visible Wi-Fi controller bindings for the current RP2350 / Pico 2 W contract.
pub(crate) const WIFI_CONTROLLERS: [CortexMWifiControllerBinding; 1] =
    [CortexMWifiControllerBinding {
        name: "pico2w-cyw43439",
        vendor: "infineon",
        chip: "CYW43439",
        transport: CortexMWifiTransportBinding::Spi3WireSharedDataIrq {
            clock_gpio: 29,
            chip_select_gpio: 25,
            data_irq_gpio: 24,
            target_clock_hz: Some(31_250_000),
        },
        power_gpio: Some(23),
        reset_gpio: None,
        wake_gpio: None,
        activity_gpio: Some(0),
        clock: RP2350_PICO2W_CYW43439_CLOCK,
        assets: RP2350_PICO2W_CYW43439_WIFI_ASSETS,
    }];

// NOTE: the current selected RP2350 board contract follows the open Pico 2 W schematic and the
// local RP2350 example linker layout, both of which assume a 32 Mbit / 4 MiB external flash
// population. Raspberry Pi's Pico 2 W prose datasheet currently disagrees and mentions a
// W25Q16JV instead, so this must split into a truly board-specific module if that ambiguity ever
// becomes more than documentation slop.
pub(crate) const FLASH_REGIONS: [CortexMFlashRegionDescriptor; 1] =
    [CortexMFlashRegionDescriptor {
        name: "qspi-flash-xip",
        base: 0x1000_0000,
        len: RP2350_FLASH_BYTES,
        erase_block_bytes: RP2350_FLASH_ERASE_BLOCK_BYTES,
        program_granule_bytes: RP2350_FLASH_PROGRAM_GRANULE_BYTES,
        xip: true,
        writable: true,
        requires_xip_quiesce: true,
    }];

pub(crate) const fn rp2350_owned_sram_region_from_bounds(
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

pub(crate) fn rp2350_owned_sram_region() -> Option<CortexMMemoryRegionDescriptor> {
    let heap_start = (&raw const __sheap) as usize;
    let stack_end = (&raw const _stack_end) as usize;
    rp2350_owned_sram_region_from_bounds(heap_start, stack_end)
}

pub(crate) fn rp2350_inline_current_exception_stack_allows(required_bytes: usize) -> bool {
    if required_bytes == 0 {
        return true;
    }

    let stack_floor = (&raw const _stack_end) as usize;
    let current_msp = rp2350_current_msp();
    rp2350_inline_current_exception_stack_allows_from_bounds(
        stack_floor,
        current_msp,
        required_bytes,
    )
}

pub(crate) const fn rp2350_inline_current_exception_stack_allows_from_bounds(
    stack_floor: usize,
    current_msp: usize,
    required_bytes: usize,
) -> bool {
    if required_bytes == 0 {
        return true;
    }
    if current_msp <= stack_floor {
        return false;
    }

    (current_msp - stack_floor)
        >= required_bytes.saturating_add(RP2350_INLINE_EXCEPTION_STACK_RESERVE_BYTES)
}

pub(crate) fn rp2350_exception_stack_observation() -> CortexMExceptionStackObservation {
    let lower_bound = (&raw const _stack_end) as usize;
    let upper_bound = (&raw const _stack_start) as usize;
    let current_sp = rp2350_current_msp();
    rp2350_exception_stack_observation_from_bounds(lower_bound, upper_bound, current_sp)
}

pub(crate) const fn rp2350_exception_stack_observation_from_bounds(
    lower_bound: usize,
    upper_bound: usize,
    current_sp: usize,
) -> CortexMExceptionStackObservation {
    CortexMExceptionStackObservation {
        lower_bound,
        upper_bound,
        configured_bytes: upper_bound.saturating_sub(lower_bound),
        current_sp,
        current_used_bytes: upper_bound.saturating_sub(current_sp),
        current_headroom_bytes: current_sp.saturating_sub(lower_bound),
        overflow_detected: current_sp < lower_bound,
    }
}

#[cfg(all(target_arch = "arm", target_os = "none"))]
pub(crate) fn rp2350_current_msp() -> usize {
    let msp: usize;
    unsafe {
        asm!(
            "mrs {msp}, MSP",
            msp = lateout(reg) msp,
            options(nomem, nostack, preserves_flags),
        );
    }
    msp
}

#[cfg(not(all(target_arch = "arm", target_os = "none")))]
pub(crate) const fn rp2350_current_msp() -> usize {
    0
}
