use super::{
    BLUETOOTH_CONTROLLERS,
    CLK_REF_AUX_SOURCES,
    CLK_REF_MAIN_SOURCES,
    CLOCK_TREE,
    CortexMBluetoothTransportBinding,
    CortexMDmaRequestClass,
    CortexMEventTimeoutImplementation,
    CortexMIrqClass,
    CortexMSocDeviceIdSupport,
    CortexMSocMonotonicTimeImpact,
    DEVICE_ID_SUPPORT,
    DMA_CONTROLLERS,
    DMA_REQUESTS,
    FLASH_REGIONS,
    IRQS,
    MEMORY_MAP,
    POWER_MODES,
    RP2350_EVENT_TIMEOUT_COUNTER_BITS,
    RP2350_EVENT_TIMEOUT_IRQN,
    RP2350_EVENT_TIMEOUT_MAX_RELATIVE_TIMEOUT,
    RP2350_EVENT_TIMEOUT_TICK_HZ,
    RP2350_OVERCLOCK_PROFILES,
    RP2350_PICO2W_RESERVED_GPIO_PINS,
    Rp2350PowerModeAction,
    event_timeout_support,
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
fn overclock_profiles_surface_known_targets_and_time_impacts() {
    assert!(
        RP2350_OVERCLOCK_PROFILES
            .iter()
            .any(|profile| profile.name == "oc-200mhz")
    );
    assert!(RP2350_OVERCLOCK_PROFILES.iter().all(|profile| matches!(
        profile.monotonic_time_impact,
        CortexMSocMonotonicTimeImpact::Unknown
    )));
}

#[test]
fn event_timeout_support_surfaces_reserved_alarm_shape() {
    let timeout = event_timeout_support().expect("rp2350 timeout support should exist");

    assert_eq!(
        timeout.implementation,
        CortexMEventTimeoutImplementation::ReservedOneShotAlarm
    );
    assert_eq!(timeout.irqn, Some(RP2350_EVENT_TIMEOUT_IRQN));
    assert_eq!(
        timeout.counter_bits,
        Some(RP2350_EVENT_TIMEOUT_COUNTER_BITS)
    );
    assert_eq!(timeout.tick_hz, Some(RP2350_EVENT_TIMEOUT_TICK_HZ));
    assert_eq!(
        timeout.max_relative_timeout,
        Some(RP2350_EVENT_TIMEOUT_MAX_RELATIVE_TIMEOUT)
    );
}

#[test]
fn dma_and_power_surfaces_are_exposed_for_rp2350() {
    assert_eq!(DMA_CONTROLLERS[0].channel_count, 16);
    assert_eq!(DMA_REQUESTS[0].name, "pio0-tx0");
    assert_eq!(DMA_REQUESTS[55].name, "dma-timer0");
    assert_eq!(POWER_MODES[0].name, "sleep-wfi");
}

#[test]
fn bluetooth_binding_tracks_pico2w_wiring_truth() {
    assert_eq!(BLUETOOTH_CONTROLLERS.len(), 1);
    let controller = BLUETOOTH_CONTROLLERS[0];
    assert_eq!(controller.vendor, "infineon");
    assert_eq!(controller.chip, "CYW43439");
    assert_eq!(controller.power_gpio, Some(23));
    assert_eq!(controller.reset_gpio, None);
    assert_eq!(controller.wake_gpio, None);
    assert_eq!(RP2350_PICO2W_RESERVED_GPIO_PINS, [23, 24, 25, 29]);

    assert!(matches!(
        controller.transport,
        CortexMBluetoothTransportBinding::Spi3WireSharedDataIrq {
            clock_gpio: 29,
            chip_select_gpio: 25,
            data_irq_gpio: 24,
        }
    ));
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
fn pio_surface_tracks_rp2350_engine_and_lane_shape() {
    assert_eq!(pcu_support().engine_count, 3);
    assert!(
        pcu_support()
            .caps
            .contains(PcuCaps::SHARED_INSTRUCTION_MEMORY)
    );
    assert_eq!(pcu_engines().len(), 3);
    assert_eq!(pcu_engines()[0].instruction_memory.word_count, 32);
    assert_eq!(pcu_engines()[1].tx_dreq_base, Some(8));
    assert_eq!(pcu_engines()[2].rx_dreq_base, Some(20));
    assert_eq!(pcu_lanes(PcuEngineId(0)).len(), 4);
    assert_eq!(pcu_lanes(PcuEngineId(2))[3].name, "pio2-sm3");
    assert!(
        pcu_lanes(PcuEngineId(1))[0]
            .pin_mapping
            .contains(PcuPinMappingCaps::SIDESET_BASE)
    );
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
fn inline_current_exception_stack_budget_uses_conservative_reserve() {
    assert!(rp2350_inline_current_exception_stack_allows_from_bounds(
        0x2000_0000,
        0x2000_0200,
        0
    ));
    assert!(rp2350_inline_current_exception_stack_allows_from_bounds(
        0x2000_0000,
        0x2000_0200,
        0x40
    ));
    assert!(!rp2350_inline_current_exception_stack_allows_from_bounds(
        0x2000_0000,
        0x2000_0080,
        0x10
    ));
    assert!(!rp2350_inline_current_exception_stack_allows_from_bounds(
        0x2000_0100,
        0x2000_0100,
        0x10
    ));
    assert!(!rp2350_inline_current_exception_stack_allows_from_bounds(
        0x2000_1000,
        0x2000_1010,
        0x10
    ));
}

#[test]
fn exception_stack_observation_reports_window_usage_and_overflow() {
    let observation =
        rp2350_exception_stack_observation_from_bounds(0x2000_0000, 0x2000_1000, 0x2000_0f00);
    assert_eq!(observation.configured_bytes, 0x1000);
    assert_eq!(observation.current_used_bytes, 0x100);
    assert_eq!(observation.current_headroom_bytes, 0x0f00);
    assert!(!observation.overflow_detected);

    let overflow =
        rp2350_exception_stack_observation_from_bounds(0x2000_0000, 0x2000_1000, 0x1fff_ffc0);
    assert_eq!(overflow.current_headroom_bytes, 0);
    assert!(overflow.current_used_bytes > overflow.configured_bytes);
    assert!(overflow.overflow_detected);
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
