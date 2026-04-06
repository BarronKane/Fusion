//! Internal CYW43439 bootstrap helpers.

use core::sync::atomic::{
    AtomicU32,
    Ordering,
};

use crate::core::{
    Cyw43439ChipState,
    Cyw43439Chipset,
};
use crate::interface::contract::{
    Cyw43439ControllerCaps,
    Cyw43439Error,
    Cyw43439HardwareContract,
    Cyw43439Radio,
};
use crate::transport::wlan::{
    CYW43439_GSPI_BACKPLANE_READ_PAD_LEN_BYTES,
    CYW43439_GSPI_AI_IOCTRL_OFFSET,
    CYW43439_GSPI_AI_RESETCTRL_OFFSET,
    CYW43439_GSPI_AIRC_RESET,
    CYW43439_GSPI_BUS_MAX_BLOCK_SIZE,
    CYW43439_GSPI_SDIOD_CCCR_BRCM_CARDCAP,
    CYW43439_GSPI_SDIOD_CCCR_BRCM_CARDCAP_CMD_NODEC,
    CYW43439_GSPI_I_HMB_FC_CHANGE,
    CYW43439_GSPI_I_HMB_SW_MASK,
    CYW43439_GSPI_POST_POWER_ON_POLL_WINDOW_MS,
    CYW43439_RAM_SIZE_BYTES,
    CYW43439_GSPI_SBSDIO_HT_AVAIL,
    CYW43439_GSPI_SDIO_CHIP_CLOCK_CSR,
    CYW43439_GSPI_SDIO_FUNCTION2_WATERMARK,
    CYW43439_GSPI_SDIO_INT_HOST_MASK,
    CYW43439_GSPI_SDIO_PULL_UP,
    CYW43439_GSPI_SDIO_SLEEP_CSR,
    CYW43439_GSPI_SDIO_WAKEUP_CTRL,
    CYW43439_GSPI_SICF_CLOCK_EN,
    CYW43439_GSPI_SICF_CPUHALT,
    CYW43439_GSPI_SICF_FGC,
    CYW43439_GSPI_SBSDIO_FORCE_HT,
    CYW43439_GSPI_SBSDIO_SLPCSR_KEEP_SDIO_ON,
    CYW43439_GSPI_SBSDIO_WCTRL_WAKE_TILL_HT_AVAIL,
    CYW43439_GSPI_SOCSRAM_BANKX_INDEX,
    CYW43439_GSPI_SOCSRAM_BANKX_PDA,
    CYW43439_GSPI_SPI_F2_WATERMARK,
    CYW43439_GSPI_TEST_PATTERN,
    CYW43439_GSPI_WLAN_ARMCM3_BASE_ADDRESS,
    CYW43439_GSPI_SOCSRAM_BASE_ADDRESS,
    CYW43439_GSPI_WRAPPER_REGISTER_OFFSET,
    Cyw43439GspiBusControlFlags,
    Cyw43439GspiBusStatusControlFlags,
    Cyw43439GspiF0Register,
    Cyw43439GspiInterruptStatusFlags,
    Cyw43439GspiStatusFlags,
    Cyw43439WlanTransport,
    Cyw43439WlanTransportLease,
};

const CYW43439_BOOT_POWER_DOWN_MS: u32 = 20;
const CYW43439_BOOT_POWER_UP_SETTLE_MS: u32 = 250;
const CYW43439_BOOT_HT_POLL_WINDOW_MS: u32 = 1000;
const CYW43439_BOOT_F2_READY_POLL_WINDOW_MS: u32 = 1000;
const CYW43439_BOOT_HT_POLL_INTERVAL_MS: u32 = 10;
const CYW43439_BOOT_F2_READY_POLL_INTERVAL_MS: u32 = 10;
const CYW43439_BOOT_BT_WATERMARK_PROBE: u8 = 0x10;
const CYW43439_CORE_WLAN_ARM: u8 = 1;
const CYW43439_CORE_SOCRAM: u8 = 2;
const CYW43439_BOOT_INTERRUPT_CLEAR_MASK: Cyw43439GspiInterruptStatusFlags =
    Cyw43439GspiInterruptStatusFlags::DATA_NOT_AVAILABLE
        .union(Cyw43439GspiInterruptStatusFlags::COMMAND_ERROR)
        .union(Cyw43439GspiInterruptStatusFlags::DATA_ERROR)
        .union(Cyw43439GspiInterruptStatusFlags::F1_OVERFLOW);
const CYW43439_BOOT_INTERRUPT_ENABLE_MASK: Cyw43439GspiInterruptStatusFlags =
    Cyw43439GspiInterruptStatusFlags::F2_F3_UNDERFLOW
        .union(Cyw43439GspiInterruptStatusFlags::F2_F3_OVERFLOW)
        .union(Cyw43439GspiInterruptStatusFlags::COMMAND_ERROR)
        .union(Cyw43439GspiInterruptStatusFlags::DATA_ERROR)
        .union(Cyw43439GspiInterruptStatusFlags::F2_PACKET_AVAILABLE)
        .union(Cyw43439GspiInterruptStatusFlags::F1_OVERFLOW);

#[unsafe(no_mangle)]
pub static CYW43439_BOOT_PHASE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_BOOT_LAST_PATTERN: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_BOOT_LAST_ERROR: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_BOOT_LAST_BACKPLANE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_BOOT_LAST_TRANSFER_ADDRESS: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_BOOT_LAST_TRANSFER_OFFSET: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_BOOT_LAST_STATUS: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_BOOT_LAST_CORE_IOCTRL: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_BOOT_LAST_CORE_RESETCTRL: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_BOOT_LAST_VERIFY: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_BOOT_LAST_SLEEP_CSR: AtomicU32 = AtomicU32::new(0);

pub(crate) struct Cyw43439Bootstrap;

impl Cyw43439Bootstrap {
    pub(crate) fn ensure_wlan_bus_clocked<H>(
        chipset: &mut Cyw43439Chipset<H>,
    ) -> Result<(), Cyw43439Error>
    where
        H: Cyw43439HardwareContract,
    {
        CYW43439_BOOT_PHASE.store(1, Ordering::Release);
        CYW43439_BOOT_LAST_ERROR.store(0, Ordering::Release);
        CYW43439_BOOT_LAST_BACKPLANE.store(0, Ordering::Release);
        CYW43439_BOOT_LAST_TRANSFER_ADDRESS.store(0, Ordering::Release);
        CYW43439_BOOT_LAST_TRANSFER_OFFSET.store(0, Ordering::Release);
        CYW43439_BOOT_LAST_STATUS.store(0, Ordering::Release);
        CYW43439_BOOT_LAST_CORE_IOCTRL.store(0, Ordering::Release);
        CYW43439_BOOT_LAST_CORE_RESETCTRL.store(0, Ordering::Release);
        CYW43439_BOOT_LAST_VERIFY.store(0, Ordering::Release);
        CYW43439_BOOT_LAST_SLEEP_CSR.store(0, Ordering::Release);
        if matches!(
            chipset.state(),
            Cyw43439ChipState::Clocked
                | Cyw43439ChipState::FirmwareLoaded
                | Cyw43439ChipState::Ready
                | Cyw43439ChipState::LowPower
        ) {
            CYW43439_BOOT_PHASE.store(2, Ordering::Release);
            return Ok(());
        }

        if !matches!(
            chipset.transport_profile()?.wifi,
            Some(Cyw43439WlanTransport::BoardSharedSpi)
        ) {
            CYW43439_BOOT_LAST_ERROR.store(1, Ordering::Release);
            return Err(Cyw43439Error::unsupported());
        }

        CYW43439_BOOT_PHASE.store(3, Ordering::Release);
        Self::power_cycle_if_supported(chipset).inspect_err(|_| {
            CYW43439_BOOT_LAST_ERROR.store(2, Ordering::Release);
        })?;
        CYW43439_BOOT_PHASE.store(4, Ordering::Release);
        Self::bootstrap_shared_spi_bus(chipset)?;
        chipset.mark_clocked();
        chipset.sync_driver_activity_indicator();
        CYW43439_BOOT_PHASE.store(14, Ordering::Release);
        Ok(())
    }

    pub(crate) fn ensure_wlan_runtime_ready<H>(
        chipset: &mut Cyw43439Chipset<H>,
    ) -> Result<(), Cyw43439Error>
    where
        H: Cyw43439HardwareContract,
    {
        Self::ensure_wlan_bus_clocked(chipset)?;
        match chipset.state() {
            Cyw43439ChipState::Ready | Cyw43439ChipState::LowPower => return Ok(()),
            Cyw43439ChipState::FirmwareLoaded => {}
            _ => Self::download_wlan_runtime(chipset)?,
        }
        Self::wait_for_wlan_runtime_ready(chipset)
    }

    fn power_cycle_if_supported<H>(chipset: &mut Cyw43439Chipset<H>) -> Result<(), Cyw43439Error>
    where
        H: Cyw43439HardwareContract,
    {
        let caps = chipset.hardware.controller_caps(Cyw43439Radio::Wifi);
        if caps.contains(Cyw43439ControllerCaps::POWER_CONTROL) {
            let _ = chipset.hardware.set_controller_powered(false);
            Self::delay_with_progress(chipset, CYW43439_BOOT_POWER_DOWN_MS);
            chipset.hardware.set_controller_powered(true)?;
            Self::delay_with_progress(chipset, CYW43439_BOOT_POWER_UP_SETTLE_MS);
            return Ok(());
        }

        if caps.contains(Cyw43439ControllerCaps::RESET_CONTROL) {
            chipset.hardware.set_controller_reset(true)?;
            Self::delay_with_progress(chipset, CYW43439_BOOT_POWER_DOWN_MS);
            chipset.hardware.set_controller_reset(false)?;
            Self::delay_with_progress(chipset, CYW43439_BOOT_POWER_UP_SETTLE_MS);
            return Ok(());
        }

        if chipset.hardware.controller_powered().unwrap_or(false) {
            Self::delay_with_progress(chipset, CYW43439_BOOT_POWER_UP_SETTLE_MS);
            return Ok(());
        }

        Err(Cyw43439Error::unsupported())
    }

    fn bootstrap_shared_spi_bus<H>(chipset: &mut Cyw43439Chipset<H>) -> Result<(), Cyw43439Error>
    where
        H: Cyw43439HardwareContract,
    {
        chipset
            .hardware
            .bootstrap_write_raw_bytes(&[0])
            .inspect_err(|_| {
                CYW43439_BOOT_LAST_ERROR.store(9, Ordering::Release);
            })?;
        CYW43439_BOOT_PHASE.store(5, Ordering::Release);
        let mut test_pattern_seen = false;
        for _ in 0..CYW43439_GSPI_POST_POWER_ON_POLL_WINDOW_MS {
            let pattern = chipset
                .hardware
                .bootstrap_read_wlan_register_swapped_u32(Cyw43439GspiF0Register::TestRead)?;
            CYW43439_BOOT_LAST_PATTERN.store(pattern, Ordering::Release);
            if pattern == CYW43439_GSPI_TEST_PATTERN {
                test_pattern_seen = true;
                break;
            }
            chipset.hardware.progress_host_runtime();
            chipset.hardware.delay_ms(1);
        }
        if !test_pattern_seen {
            CYW43439_BOOT_LAST_ERROR.store(3, Ordering::Release);
            return Err(Cyw43439Error::busy());
        }

        let mut bus_control = Cyw43439GspiBusControlFlags::WORD_LENGTH_32
            | Cyw43439GspiBusControlFlags::BIG_ENDIAN
            | Cyw43439GspiBusControlFlags::HIGH_SPEED
            | Cyw43439GspiBusControlFlags::WAKE_WLAN;
        let bluetooth_firmware_present = chipset
            .hardware
            .controller_caps(Cyw43439Radio::Bluetooth)
            .contains(Cyw43439ControllerCaps::FIRMWARE_IMAGE);
        if chipset
            .hardware
            .controller_caps(Cyw43439Radio::Wifi)
            .contains(Cyw43439ControllerCaps::IRQ_WAIT)
        {
            bus_control |= Cyw43439GspiBusControlFlags::INTERRUPT_POLARITY_HIGH;
        }

        let bootstrap_word = (bus_control.bits() as u32)
            | (4_u32 << 8)
            | ((Cyw43439GspiBusStatusControlFlags::INTERRUPT_WITH_STATUS.bits() as u32) << 16);
        CYW43439_BOOT_PHASE.store(6, Ordering::Release);
        chipset
            .hardware
            .bootstrap_write_wlan_register_swapped_u32(
                Cyw43439GspiF0Register::BusControl,
                bootstrap_word,
            )
            .inspect_err(|_| {
                CYW43439_BOOT_LAST_ERROR.store(4, Ordering::Release);
            })?;

        let mut enable_mask = CYW43439_BOOT_INTERRUPT_ENABLE_MASK;
        if bluetooth_firmware_present {
            enable_mask |= Cyw43439GspiInterruptStatusFlags::F1_INTERRUPT;
        }

        CYW43439_BOOT_PHASE.store(7, Ordering::Release);
        let mut transport = Cyw43439WlanTransportLease::acquire(&mut chipset.hardware)
            .inspect_err(|_| {
                CYW43439_BOOT_LAST_ERROR.store(5, Ordering::Release);
            })?;
        let _ = transport
            .read_f0_u32(Cyw43439GspiF0Register::BusControl)
            .inspect_err(|_| {
                CYW43439_BOOT_LAST_ERROR.store(6, Ordering::Release);
            })?;
        CYW43439_BOOT_PHASE.store(8, Ordering::Release);
        transport
            .write_f0_u8(
                Cyw43439GspiF0Register::ResponseDelayF1,
                CYW43439_GSPI_BACKPLANE_READ_PAD_LEN_BYTES as u8,
            )
            .inspect_err(|_| {
                CYW43439_BOOT_LAST_ERROR.store(7, Ordering::Release);
            })?;
        CYW43439_BOOT_PHASE.store(9, Ordering::Release);
        if bluetooth_firmware_present {
            transport
                .write_f1_u8(
                    CYW43439_GSPI_SDIO_FUNCTION2_WATERMARK,
                    CYW43439_BOOT_BT_WATERMARK_PROBE,
                )
                .inspect_err(|_| {
                    CYW43439_BOOT_LAST_ERROR.store(11, Ordering::Release);
                })?;
            let watermark = transport
                .read_f1_u8(CYW43439_GSPI_SDIO_FUNCTION2_WATERMARK)
                .inspect_err(|_| {
                    CYW43439_BOOT_LAST_ERROR.store(12, Ordering::Release);
                })?;
            CYW43439_BOOT_LAST_STATUS.store(watermark as u32, Ordering::Release);
        }
        CYW43439_BOOT_PHASE.store(10, Ordering::Release);
        let backplane_clock_csr = transport
            .read_f1_u8(CYW43439_GSPI_SDIO_CHIP_CLOCK_CSR)
            .inspect_err(|_| {
                CYW43439_BOOT_LAST_ERROR.store(8, Ordering::Release);
            })?;
        CYW43439_BOOT_LAST_BACKPLANE.store(backplane_clock_csr as u32, Ordering::Release);
        CYW43439_BOOT_PHASE.store(11, Ordering::Release);
        transport
            .write_f0_u16(
                Cyw43439GspiF0Register::InterruptStatus,
                CYW43439_BOOT_INTERRUPT_CLEAR_MASK.bits(),
            )
            .inspect_err(|_| {
                CYW43439_BOOT_LAST_ERROR.store(9, Ordering::Release);
            })?;
        CYW43439_BOOT_PHASE.store(12, Ordering::Release);
        transport
            .write_f0_u16(Cyw43439GspiF0Register::InterruptEnable, enable_mask.bits())
            .inspect_err(|_| {
                CYW43439_BOOT_LAST_ERROR.store(10, Ordering::Release);
            })?;
        CYW43439_BOOT_PHASE.store(13, Ordering::Release);
        Ok(())
    }

    fn download_wlan_runtime<H>(chipset: &mut Cyw43439Chipset<H>) -> Result<(), Cyw43439Error>
    where
        H: Cyw43439HardwareContract,
    {
        let assets = chipset.wifi_firmware_assets()?;
        let firmware = assets
            .firmware_image
            .ok_or_else(Cyw43439Error::unsupported)?;
        let nvram = assets.nvram_image.ok_or_else(Cyw43439Error::unsupported)?;

        CYW43439_BOOT_PHASE.store(20, Ordering::Release);
        let mut transport = Cyw43439WlanTransportLease::acquire(&mut chipset.hardware)
            .inspect_err(|_| {
                CYW43439_BOOT_LAST_ERROR.store(20, Ordering::Release);
            })?;

        CYW43439_BOOT_PHASE.store(21, Ordering::Release);
        Self::disable_device_core(&mut transport, CYW43439_CORE_WLAN_ARM, false).inspect_err(
            |_| {
                CYW43439_BOOT_LAST_ERROR.store(21, Ordering::Release);
            },
        )?;
        CYW43439_BOOT_PHASE.store(22, Ordering::Release);
        Self::disable_device_core(&mut transport, CYW43439_CORE_SOCRAM, false).inspect_err(
            |_| {
                CYW43439_BOOT_LAST_ERROR.store(22, Ordering::Release);
            },
        )?;
        CYW43439_BOOT_PHASE.store(23, Ordering::Release);
        Self::reset_device_core(&mut transport, CYW43439_CORE_SOCRAM, false).inspect_err(|_| {
            CYW43439_BOOT_LAST_ERROR.store(23, Ordering::Release);
        })?;
        drop(transport);
        chipset.sync_driver_activity_indicator();
        let mut transport = Cyw43439WlanTransportLease::acquire(&mut chipset.hardware)
            .inspect_err(|_| {
                CYW43439_BOOT_LAST_ERROR.store(48, Ordering::Release);
            })?;

        CYW43439_BOOT_PHASE.store(24, Ordering::Release);
        transport
            .write_backplane_u32(CYW43439_GSPI_SOCSRAM_BANKX_INDEX, 0x3)
            .inspect_err(|_| {
                CYW43439_BOOT_LAST_ERROR.store(24, Ordering::Release);
            })?;
        transport
            .write_backplane_u32(CYW43439_GSPI_SOCSRAM_BANKX_PDA, 0)
            .inspect_err(|_| {
                CYW43439_BOOT_LAST_ERROR.store(25, Ordering::Release);
            })?;

        CYW43439_BOOT_PHASE.store(26, Ordering::Release);
        let firmware_len_aligned = align_up_4(firmware.len());
        Self::download_backplane_resource(&mut transport, 0, firmware, firmware_len_aligned)
            .inspect_err(|_| {
                CYW43439_BOOT_LAST_ERROR.store(26, Ordering::Release);
            })?;
        Self::verify_backplane_word(&mut transport, 0, &firmware[..firmware.len().min(4)])
            .inspect_err(|_| {
                CYW43439_BOOT_LAST_ERROR.store(44, Ordering::Release);
            })?;
        if firmware.len() > 4 {
            let start = firmware.len() - 4;
            Self::verify_backplane_word(&mut transport, start as u32, &firmware[start..])
                .inspect_err(|_| {
                    CYW43439_BOOT_LAST_ERROR.store(45, Ordering::Release);
                })?;
        }

        CYW43439_BOOT_PHASE.store(27, Ordering::Release);
        let nvram_len_aligned = align_up_4(nvram.len());
        let nvram_addr = CYW43439_RAM_SIZE_BYTES
            .checked_sub(4 + nvram_len_aligned as u32)
            .ok_or_else(Cyw43439Error::invalid)
            .inspect_err(|_| {
                CYW43439_BOOT_LAST_ERROR.store(27, Ordering::Release);
            })?;
        Self::download_backplane_resource(&mut transport, nvram_addr, nvram, nvram_len_aligned)
            .inspect_err(|_| {
                CYW43439_BOOT_LAST_ERROR.store(28, Ordering::Release);
            })?;
        Self::verify_backplane_word(&mut transport, nvram_addr, &nvram[..nvram.len().min(4)])
            .inspect_err(|_| {
                CYW43439_BOOT_LAST_ERROR.store(46, Ordering::Release);
            })?;
        let nvram_words = (nvram_len_aligned / 4) as u32;
        let nvram_size_info = ((!(nvram_words) & 0xffff) << 16) | nvram_words;
        transport
            .write_backplane_u32(CYW43439_RAM_SIZE_BYTES - 4, nvram_size_info)
            .inspect_err(|_| {
                CYW43439_BOOT_LAST_ERROR.store(29, Ordering::Release);
            })?;
        CYW43439_BOOT_LAST_VERIFY.store(nvram_size_info, Ordering::Release);

        CYW43439_BOOT_PHASE.store(28, Ordering::Release);
        Self::reset_device_core(&mut transport, CYW43439_CORE_WLAN_ARM, false).inspect_err(
            |_| {
                CYW43439_BOOT_LAST_ERROR.store(30, Ordering::Release);
            },
        )?;
        Self::device_core_is_up(&mut transport, CYW43439_CORE_WLAN_ARM).inspect_err(|_| {
            CYW43439_BOOT_LAST_ERROR.store(31, Ordering::Release);
        })?;
        drop(transport);
        chipset.mark_firmware_loaded();
        chipset.sync_driver_activity_indicator();
        CYW43439_BOOT_PHASE.store(29, Ordering::Release);
        Ok(())
    }

    fn wait_for_wlan_runtime_ready<H>(chipset: &mut Cyw43439Chipset<H>) -> Result<(), Cyw43439Error>
    where
        H: Cyw43439HardwareContract,
    {
        let host_interrupt_mask = if chipset
            .hardware
            .controller_caps(Cyw43439Radio::Bluetooth)
            .contains(Cyw43439ControllerCaps::FIRMWARE_IMAGE)
        {
            CYW43439_GSPI_I_HMB_FC_CHANGE
        } else {
            CYW43439_GSPI_I_HMB_SW_MASK
        };
        let mut transport = Cyw43439WlanTransportLease::acquire(&mut chipset.hardware)
            .inspect_err(|_| {
                CYW43439_BOOT_LAST_ERROR.store(32, Ordering::Release);
            })?;

        CYW43439_BOOT_PHASE.store(30, Ordering::Release);
        let ht_poll_attempts =
            (CYW43439_BOOT_HT_POLL_WINDOW_MS / CYW43439_BOOT_HT_POLL_INTERVAL_MS).max(1);
        for _ in 0..ht_poll_attempts {
            let clock_csr = transport
                .read_f1_u8(CYW43439_GSPI_SDIO_CHIP_CLOCK_CSR)
                .inspect_err(|_| {
                    CYW43439_BOOT_LAST_ERROR.store(33, Ordering::Release);
                })?;
            CYW43439_BOOT_LAST_BACKPLANE.store(clock_csr as u32, Ordering::Release);
            if (clock_csr & CYW43439_GSPI_SBSDIO_HT_AVAIL) != 0 {
                break;
            }
            transport.progress_host_runtime();
            transport.delay_ms(CYW43439_BOOT_HT_POLL_INTERVAL_MS);
        }
        if (CYW43439_BOOT_LAST_BACKPLANE.load(Ordering::Acquire) as u8
            & CYW43439_GSPI_SBSDIO_HT_AVAIL)
            == 0
        {
            CYW43439_BOOT_LAST_ERROR.store(34, Ordering::Release);
            return Err(Cyw43439Error::busy());
        }
        CYW43439_BOOT_PHASE.store(31, Ordering::Release);
        transport
            .write_backplane_u32(CYW43439_GSPI_SDIO_INT_HOST_MASK, host_interrupt_mask)
            .inspect_err(|_| {
                CYW43439_BOOT_LAST_ERROR.store(35, Ordering::Release);
            })?;

        CYW43439_BOOT_PHASE.store(32, Ordering::Release);
        transport
            .write_f1_u8(
                CYW43439_GSPI_SDIO_FUNCTION2_WATERMARK,
                CYW43439_GSPI_SPI_F2_WATERMARK,
            )
            .inspect_err(|_| {
                CYW43439_BOOT_LAST_ERROR.store(36, Ordering::Release);
            })?;

        CYW43439_BOOT_PHASE.store(33, Ordering::Release);
        let mut f2_ready = false;
        let f2_poll_attempts = (CYW43439_BOOT_F2_READY_POLL_WINDOW_MS
            / CYW43439_BOOT_F2_READY_POLL_INTERVAL_MS)
            .max(1);
        for _ in 0..f2_poll_attempts {
            let status = transport.read_status_register().inspect_err(|_| {
                CYW43439_BOOT_LAST_ERROR.store(37, Ordering::Release);
            })?;
            CYW43439_BOOT_LAST_STATUS.store(status.raw, Ordering::Release);
            if status.flags.contains(Cyw43439GspiStatusFlags::F2_RX_READY) {
                f2_ready = true;
                break;
            }
            transport.progress_host_runtime();
            transport.delay_ms(CYW43439_BOOT_F2_READY_POLL_INTERVAL_MS);
        }
        if f2_ready {
            Self::enable_spi_runtime_keep_awake(&mut transport).inspect_err(|_| {
                CYW43439_BOOT_LAST_ERROR.store(47, Ordering::Release);
            })?;
        }
        drop(transport);
        if f2_ready {
            chipset.mark_ready();
            chipset.sync_driver_activity_indicator();
            CYW43439_BOOT_PHASE.store(34, Ordering::Release);
            Ok(())
        } else {
            CYW43439_BOOT_LAST_ERROR.store(38, Ordering::Release);
            Err(Cyw43439Error::busy())
        }
    }

    fn get_core_address(core_id: u8) -> Result<u32, Cyw43439Error> {
        match core_id {
            CYW43439_CORE_WLAN_ARM => {
                Ok(CYW43439_GSPI_WLAN_ARMCM3_BASE_ADDRESS + CYW43439_GSPI_WRAPPER_REGISTER_OFFSET)
            }
            CYW43439_CORE_SOCRAM => {
                Ok(CYW43439_GSPI_SOCSRAM_BASE_ADDRESS + CYW43439_GSPI_WRAPPER_REGISTER_OFFSET)
            }
            _ => Err(Cyw43439Error::invalid()),
        }
    }

    fn disable_device_core<H>(
        transport: &mut Cyw43439WlanTransportLease<'_, H>,
        core_id: u8,
        _core_halt: bool,
    ) -> Result<(), Cyw43439Error>
    where
        H: Cyw43439HardwareContract,
    {
        let base = Self::get_core_address(core_id)?;
        let _ = transport.read_backplane_u8(base + CYW43439_GSPI_AI_RESETCTRL_OFFSET)?;
        let reg = transport.read_backplane_u8(base + CYW43439_GSPI_AI_RESETCTRL_OFFSET)?;
        if (reg & CYW43439_GSPI_AIRC_RESET) != 0 {
            return Ok(());
        }
        Err(Cyw43439Error::invalid())
    }

    fn reset_device_core<H>(
        transport: &mut Cyw43439WlanTransportLease<'_, H>,
        core_id: u8,
        core_halt: bool,
    ) -> Result<(), Cyw43439Error>
    where
        H: Cyw43439HardwareContract,
    {
        Self::disable_device_core(transport, core_id, core_halt)?;
        let base = Self::get_core_address(core_id)?;
        let ioctrl = base + CYW43439_GSPI_AI_IOCTRL_OFFSET;
        let resetctrl = base + CYW43439_GSPI_AI_RESETCTRL_OFFSET;
        let mut ioctrl_value = CYW43439_GSPI_SICF_FGC | CYW43439_GSPI_SICF_CLOCK_EN;
        if core_halt {
            ioctrl_value |= CYW43439_GSPI_SICF_CPUHALT;
        }
        transport.write_backplane_u8(ioctrl, ioctrl_value)?;
        let _ = transport.read_backplane_u8(ioctrl)?;
        transport.write_backplane_u8(resetctrl, 0)?;
        transport.delay_ms(1);
        ioctrl_value = CYW43439_GSPI_SICF_CLOCK_EN;
        if core_halt {
            ioctrl_value |= CYW43439_GSPI_SICF_CPUHALT;
        }
        transport.write_backplane_u8(ioctrl, ioctrl_value)?;
        let _ = transport.read_backplane_u8(ioctrl)?;
        transport.delay_ms(1);
        Ok(())
    }

    fn enable_spi_runtime_keep_awake<H>(
        transport: &mut Cyw43439WlanTransportLease<'_, H>,
    ) -> Result<(), Cyw43439Error>
    where
        H: Cyw43439HardwareContract,
    {
        let wakeup_ctrl = transport.read_f1_u8(CYW43439_GSPI_SDIO_WAKEUP_CTRL)?;
        transport.write_f1_u8(
            CYW43439_GSPI_SDIO_WAKEUP_CTRL,
            wakeup_ctrl | CYW43439_GSPI_SBSDIO_WCTRL_WAKE_TILL_HT_AVAIL,
        )?;
        transport.write_bus_u8(
            CYW43439_GSPI_SDIOD_CCCR_BRCM_CARDCAP,
            CYW43439_GSPI_SDIOD_CCCR_BRCM_CARDCAP_CMD_NODEC,
        )?;
        transport.write_f1_u8(
            CYW43439_GSPI_SDIO_CHIP_CLOCK_CSR,
            CYW43439_GSPI_SBSDIO_FORCE_HT,
        )?;

        let sleep_csr = transport.read_f1_u8(CYW43439_GSPI_SDIO_SLEEP_CSR)?;
        CYW43439_BOOT_LAST_SLEEP_CSR.store(sleep_csr as u32, Ordering::Release);
        if (sleep_csr & CYW43439_GSPI_SBSDIO_SLPCSR_KEEP_SDIO_ON) == 0 {
            transport.write_f1_u8(
                CYW43439_GSPI_SDIO_SLEEP_CSR,
                sleep_csr | CYW43439_GSPI_SBSDIO_SLPCSR_KEEP_SDIO_ON,
            )?;
            let read_back = transport.read_f1_u8(CYW43439_GSPI_SDIO_SLEEP_CSR)?;
            CYW43439_BOOT_LAST_SLEEP_CSR.store(read_back as u32, Ordering::Release);
        }
        transport.write_f1_u8(CYW43439_GSPI_SDIO_PULL_UP, 0x0f)?;
        Ok(())
    }

    fn device_core_is_up<H>(
        transport: &mut Cyw43439WlanTransportLease<'_, H>,
        core_id: u8,
    ) -> Result<(), Cyw43439Error>
    where
        H: Cyw43439HardwareContract,
    {
        let base = Self::get_core_address(core_id)?;
        let reg = transport.read_backplane_u8(base + CYW43439_GSPI_AI_IOCTRL_OFFSET)?;
        CYW43439_BOOT_LAST_CORE_IOCTRL.store(reg as u32, Ordering::Release);
        if (reg & (CYW43439_GSPI_SICF_FGC | CYW43439_GSPI_SICF_CLOCK_EN))
            != CYW43439_GSPI_SICF_CLOCK_EN
        {
            return Err(Cyw43439Error::invalid());
        }
        let reg = transport.read_backplane_u8(base + CYW43439_GSPI_AI_RESETCTRL_OFFSET)?;
        CYW43439_BOOT_LAST_CORE_RESETCTRL.store(reg as u32, Ordering::Release);
        if (reg & CYW43439_GSPI_AIRC_RESET) != 0 {
            return Err(Cyw43439Error::invalid());
        }
        Ok(())
    }

    fn verify_backplane_word<H>(
        transport: &mut Cyw43439WlanTransportLease<'_, H>,
        address: u32,
        expected: &[u8],
    ) -> Result<(), Cyw43439Error>
    where
        H: Cyw43439HardwareContract,
    {
        if expected.is_empty() || expected.len() > 4 {
            return Ok(());
        }
        let mut actual = [0_u8; 4];
        for (index, slot) in actual[..expected.len()].iter_mut().enumerate() {
            *slot = transport.read_backplane_u8(address + index as u32)?;
        }
        let actual_word = u32::from_le_bytes(actual);
        CYW43439_BOOT_LAST_VERIFY.store(actual_word, Ordering::Release);
        let mut expected_word = [0_u8; 4];
        expected_word[..expected.len()].copy_from_slice(expected);
        if actual_word != u32::from_le_bytes(expected_word) {
            return Err(Cyw43439Error::invalid());
        }
        Ok(())
    }

    fn download_backplane_resource<H>(
        transport: &mut Cyw43439WlanTransportLease<'_, H>,
        base_address: u32,
        data: &[u8],
        padded_len: usize,
    ) -> Result<(), Cyw43439Error>
    where
        H: Cyw43439HardwareContract,
    {
        let mut offset = 0usize;
        while offset < padded_len {
            let end = (offset + CYW43439_GSPI_BUS_MAX_BLOCK_SIZE).min(padded_len);
            let mut block = [0_u8; CYW43439_GSPI_BUS_MAX_BLOCK_SIZE];
            let copy_len = data.len().saturating_sub(offset).min(end - offset);
            if copy_len > 0 {
                block[..copy_len].copy_from_slice(&data[offset..offset + copy_len]);
            }
            let address = base_address + offset as u32;
            CYW43439_BOOT_LAST_TRANSFER_ADDRESS.store(address, Ordering::Release);
            CYW43439_BOOT_LAST_TRANSFER_OFFSET.store(offset as u32, Ordering::Release);
            transport.write_backplane_bytes(address, &block[..end - offset])?;
            transport.progress_host_runtime();
            offset = end;
        }
        Ok(())
    }

    fn delay_with_progress<H>(chipset: &mut Cyw43439Chipset<H>, milliseconds: u32)
    where
        H: Cyw43439HardwareContract,
    {
        for _ in 0..milliseconds {
            chipset.hardware.progress_host_runtime();
            chipset.hardware.delay_ms(1);
        }
    }
}

const fn align_up_4(value: usize) -> usize {
    (value + 3) & !3
}
