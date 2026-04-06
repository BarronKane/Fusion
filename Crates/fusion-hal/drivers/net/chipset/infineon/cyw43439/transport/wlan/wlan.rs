//! CYW43439 WLAN host-transport descriptors.

use bitflags::bitflags;

use crate::interface::contract::{
    Cyw43439Error,
    Cyw43439HardwareContract,
    Cyw43439Radio,
};

/// Documented WLAN-facing host transport for CYW43439.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cyw43439WlanTransport {
    /// Generic SPI transport described by the vendor datasheet.
    Gspi,
    /// Standard SDIO transport described by the vendor datasheet.
    Sdio,
    /// Board-selected shared SPI host path where WLAN traffic shares the same CYW43 transport
    /// lane used by the Bluetooth side.
    BoardSharedSpi,
}

/// Host-side transport clock plan for one CYW43439 WLAN lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cyw43439WlanTransportClockProfile {
    /// gSPI-backed WLAN transport.
    Gspi {
        /// Intended SPI transport clock rate.
        target_clock_hz: Option<u32>,
        /// Current host-side source clock feeding the transport block when one is known.
        host_source_clock_hz: Option<u64>,
    },
    /// SDIO-backed WLAN transport.
    Sdio {
        /// Intended SDIO clock rate.
        target_clock_hz: Option<u32>,
        /// Current host-side source clock feeding the transport block when one is known.
        host_source_clock_hz: Option<u64>,
    },
    /// Board-shared SPI path tunneling WLAN traffic.
    BoardSharedSpi {
        /// Intended SPI transport clock rate.
        target_clock_hz: Option<u32>,
        /// Current host-side source clock feeding the transport block when one is known.
        host_source_clock_hz: Option<u64>,
    },
}

/// F0 register lanes used during gSPI bring-up and framed data exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum Cyw43439GspiF0Register {
    BusControl = 0x0000,
    BusResponseDelay = 0x0001,
    BusStatusControl = 0x0002,
    BackplaneReset = 0x0003,
    InterruptStatus = 0x0004,
    InterruptEnable = 0x0006,
    Status = 0x0008,
    F1Info = 0x000C,
    F2Info = 0x000E,
    TestRead = 0x0014,
    TestReadWrite = 0x0018,
    ResponseDelayF0 = 0x001C,
    ResponseDelayF1 = 0x001D,
    ResponseDelayF2 = 0x001E,
    ResponseDelayF3 = 0x001F,
}

impl Cyw43439GspiF0Register {
    #[must_use]
    pub const fn address(self) -> u32 {
        self as u32
    }
}

bitflags! {
    /// Bus-control flags for F0 register `0x0000`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Cyw43439GspiBusControlFlags: u16 {
        const WORD_LENGTH_32             = 1 << 0;
        const BIG_ENDIAN                 = 1 << 1;
        const HIGH_SPEED                 = 1 << 4;
        const INTERRUPT_POLARITY_HIGH    = 1 << 5;
        const WAKE_WLAN                  = 1 << 7;
    }
}

bitflags! {
    /// Status-control flags for F0 register `0x0002`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Cyw43439GspiBusStatusControlFlags: u16 {
        const STATUS_ENABLE              = 1 << 0;
        const INTERRUPT_WITH_STATUS      = 1 << 1;
    }
}

bitflags! {
    /// Interrupt/status bits surfaced through F0 `0x0004`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cyw43439GspiInterruptStatusFlags: u16 {
        const DATA_NOT_AVAILABLE         = 1 << 0;
        const F2_F3_UNDERFLOW            = 1 << 1;
        const F2_F3_OVERFLOW             = 1 << 2;
        const COMMAND_ERROR              = 1 << 3;
        const DATA_ERROR                 = 1 << 4;
        const F2_PACKET_AVAILABLE        = 1 << 5;
        const F3_PACKET_AVAILABLE        = 1 << 6;
        const F1_OVERFLOW                = 1 << 7;
        const GSPI_PACKET_AVAILABLE      = 1 << 8;
        const MISC_INTERRUPT1            = 1 << 9;
        const MISC_INTERRUPT2            = 1 << 10;
        const MISC_INTERRUPT3            = 1 << 11;
        const MISC_INTERRUPT4            = 1 << 12;
        const F1_INTERRUPT               = 1 << 13;
        const F2_INTERRUPT               = 1 << 14;
        const F3_INTERRUPT               = 1 << 15;
    }
}

bitflags! {
    /// Interrupt-line bits surfaced through F0 `0x0005` / enable mask `0x0006`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Cyw43439GspiInterruptLineFlags: u16 {
        const F1_INTERRUPT               = 1 << 5;
        const F2_INTERRUPT               = 1 << 6;
        const F3_INTERRUPT               = 1 << 7;
    }
}

/// gSPI function selector surfaced by the CYW43439 WLAN host interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Cyw43439GspiFunction {
    F0 = 0b00,
    F1 = 0b01,
    F2 = 0b10,
    F3 = 0b11,
}

impl Cyw43439GspiFunction {
    #[must_use]
    pub const fn from_bits(bits: u8) -> Self {
        match bits & 0b11 {
            0b00 => Self::F0,
            0b01 => Self::F1,
            0b10 => Self::F2,
            _ => Self::F3,
        }
    }
}

/// Parsed 32-bit gSPI command word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cyw43439GspiCommand {
    pub write: bool,
    pub incrementing: bool,
    pub function: Cyw43439GspiFunction,
    pub address: u32,
    pub packet_length: u16,
}

impl Cyw43439GspiCommand {
    pub const MAX_ADDRESS: u32 = (1 << 17) - 1;
    pub const MAX_PACKET_LENGTH: u16 = 2048;
    pub const F1_MAX_PACKET_LENGTH: u16 = 64;

    /// Builds one fixed-address F0 register read.
    #[must_use]
    pub const fn read_f0(register: Cyw43439GspiF0Register) -> Self {
        Self {
            write: false,
            incrementing: true,
            function: Cyw43439GspiFunction::F0,
            address: register.address(),
            packet_length: 4,
        }
    }

    /// Builds one fixed-address F0 register write.
    #[must_use]
    pub const fn write_f0(register: Cyw43439GspiF0Register, payload_length: u16) -> Self {
        Self {
            write: true,
            incrementing: true,
            function: Cyw43439GspiFunction::F0,
            address: register.address(),
            packet_length: payload_length,
        }
    }

    /// Encodes one gSPI command if the address and payload length are valid.
    #[must_use]
    pub const fn encode(self) -> Option<u32> {
        if self.address > Self::MAX_ADDRESS {
            return None;
        }
        if self.packet_length == 0 || self.packet_length > Self::MAX_PACKET_LENGTH {
            return None;
        }
        if matches!(self.function, Cyw43439GspiFunction::F1)
            && self.packet_length > Self::F1_MAX_PACKET_LENGTH
        {
            return None;
        }

        let encoded_length = if self.packet_length == Self::MAX_PACKET_LENGTH {
            0
        } else {
            self.packet_length
        };
        let write = if self.write { 1_u32 } else { 0 };
        let incrementing = if self.incrementing { 1_u32 } else { 0 };

        Some(
            (write << 31)
                | (incrementing << 30)
                | ((self.function as u32) << 28)
                | ((self.address & Self::MAX_ADDRESS) << 11)
                | ((encoded_length & 0x07ff) as u32),
        )
    }

    /// Decodes one raw 32-bit gSPI command word.
    #[must_use]
    pub const fn decode(raw: u32) -> Self {
        let encoded_length = (raw & 0x07ff) as u16;
        Self {
            write: ((raw >> 31) & 1) != 0,
            incrementing: ((raw >> 30) & 1) != 0,
            function: Cyw43439GspiFunction::from_bits(((raw >> 28) & 0b11) as u8),
            address: (raw >> 11) & Self::MAX_ADDRESS,
            packet_length: if encoded_length == 0 {
                Self::MAX_PACKET_LENGTH
            } else {
                encoded_length
            },
        }
    }
}

/// Parsed F1/F2 info register shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cyw43439GspiFunctionInfo {
    pub enabled: bool,
    pub ready: bool,
    pub max_packet_size: u16,
}

impl Cyw43439GspiFunctionInfo {
    #[must_use]
    pub const fn decode(raw: u16) -> Self {
        Self {
            enabled: (raw & 0x0001) != 0,
            ready: (raw & 0x0002) != 0,
            max_packet_size: ((raw >> 2) & 0x3fff) as u16,
        }
    }
}

bitflags! {
    /// gSPI trailing status flags surfaced by the WLAN transport.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Cyw43439GspiStatusFlags: u32 {
        const UNDERFLOW         = 1 << 1;
        const OVERFLOW          = 1 << 2;
        const F2_INTERRUPT      = 1 << 3;
        const F2_RX_READY       = 1 << 5;
        const F2_PACKET_READY   = 1 << 8;
    }
}

/// Parsed gSPI trailing status word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cyw43439GspiStatus {
    pub flags: Cyw43439GspiStatusFlags,
    pub f2_packet_length: u16,
    pub raw: u32,
}

impl Cyw43439GspiStatus {
    /// Decodes one raw gSPI trailing status word.
    #[must_use]
    pub const fn decode(raw: u32) -> Self {
        Self {
            flags: Cyw43439GspiStatusFlags::from_bits_retain(raw),
            f2_packet_length: ((raw >> 9) & 0x07ff) as u16,
            raw,
        }
    }
}

/// Predefined F0 test pattern documented by the vendor gSPI bring-up sequence.
pub const CYW43439_GSPI_TEST_PATTERN: u32 = 0xFEED_BEAD;
/// Maximum documented host wait window before F0 test reads should respond after power-on.
pub const CYW43439_GSPI_POST_POWER_ON_POLL_WINDOW_MS: u32 = 50;
/// Maximum documented wait window after asserting the WLAN wake bit.
pub const CYW43439_GSPI_WAKE_WAIT_MS: u32 = 15;
/// Upstream Pico/CYW43 SPI path uses 64-byte backplane write blocks.
pub const CYW43439_GSPI_BUS_MAX_BLOCK_SIZE: usize = 64;
/// Upstream Pico/CYW43 SPI path uses a 16-byte response-delay pad for backplane (F1) reads.
pub const CYW43439_GSPI_BACKPLANE_READ_PAD_LEN_BYTES: usize = 16;
pub const CYW43439_GSPI_BACKPLANE_ADDRESS_LOW: u32 = 0x1000A;
pub const CYW43439_GSPI_BACKPLANE_ADDRESS_MID: u32 = 0x1000B;
pub const CYW43439_GSPI_BACKPLANE_ADDRESS_HIGH: u32 = 0x1000C;
pub const CYW43439_GSPI_SDIO_CHIP_CLOCK_CSR: u32 = 0x1000E;
pub const CYW43439_GSPI_SDIO_PULL_UP: u32 = 0x1000F;
pub const CYW43439_GSPI_CHIPCOMMON_BASE_ADDRESS: u32 = 0x1800_0000;
pub const CYW43439_GSPI_CHIPCOMMON_GPIOOUT_OFFSET: u32 = 0x64;
pub const CYW43439_GSPI_CHIPCOMMON_GPIOOUTEN_OFFSET: u32 = 0x68;
pub const CYW43439_GSPI_CHIPCOMMON_GPIOCONTROL_OFFSET: u32 = 0x6c;
pub const CYW43439_GSPI_BACKPLANE_ADDR_MASK: u32 = 0x7FFF;
pub const CYW43439_GSPI_BACKPLANE_ACCESS_2_4B_FLAG: u32 = 0x08000;
pub const CYW43439_GSPI_SDIO_FUNCTION2_WATERMARK: u32 = 0x10008;
pub const CYW43439_GSPI_SDIO_INT_HOST_MASK: u32 = 0x1800_2024;
pub const CYW43439_GSPI_SDIO_WAKEUP_CTRL: u32 = 0x1001e;
pub const CYW43439_GSPI_SDIO_SLEEP_CSR: u32 = 0x1001f;
pub const CYW43439_GSPI_SDIOD_CCCR_BRCM_CARDCAP: u32 = 0x00F0;
pub const CYW43439_GSPI_WLAN_ARMCM3_BASE_ADDRESS: u32 = 0x1800_3000;
pub const CYW43439_GSPI_SOCSRAM_BASE_ADDRESS: u32 = 0x1800_4000;
pub const CYW43439_GSPI_WRAPPER_REGISTER_OFFSET: u32 = 0x100000;
pub const CYW43439_GSPI_SOCSRAM_BANKX_INDEX: u32 = CYW43439_GSPI_SOCSRAM_BASE_ADDRESS + 0x10;
pub const CYW43439_GSPI_SOCSRAM_BANKX_PDA: u32 = CYW43439_GSPI_SOCSRAM_BASE_ADDRESS + 0x44;
pub const CYW43439_GSPI_AI_IOCTRL_OFFSET: u32 = 0x408;
pub const CYW43439_GSPI_AI_RESETCTRL_OFFSET: u32 = 0x800;
pub const CYW43439_GSPI_SICF_CLOCK_EN: u8 = 0x01;
pub const CYW43439_GSPI_SICF_FGC: u8 = 0x02;
pub const CYW43439_GSPI_SICF_CPUHALT: u8 = 0x20;
pub const CYW43439_GSPI_AIRC_RESET: u8 = 0x01;
pub const CYW43439_GSPI_SBSDIO_ALP_AVAIL_REQ: u8 = 0x08;
pub const CYW43439_GSPI_SBSDIO_FORCE_HT: u8 = 0x02;
pub const CYW43439_GSPI_SBSDIO_HT_AVAIL_REQ: u8 = 0x10;
pub const CYW43439_GSPI_SBSDIO_ALP_AVAIL: u8 = 0x40;
pub const CYW43439_GSPI_SBSDIO_HT_AVAIL: u8 = 0x80;
pub const CYW43439_GSPI_SBSDIO_WCTRL_WAKE_TILL_HT_AVAIL: u8 = 1 << 1;
pub const CYW43439_GSPI_SBSDIO_SLPCSR_KEEP_SDIO_ON: u8 = 1 << 0;
pub const CYW43439_GSPI_SBSDIO_SLPCSR_DEVICE_ON: u8 = 1 << 1;
pub const CYW43439_GSPI_SDIOD_CCCR_BRCM_CARDCAP_CMD_NODEC: u8 = 0x08;
pub const CYW43439_GSPI_I_HMB_SW_MASK: u32 = 0x0000_00f0;
pub const CYW43439_GSPI_I_HMB_FC_CHANGE: u32 = 1 << 5;
pub const CYW43439_GSPI_SPI_F2_WATERMARK: u8 = 32;
pub const CYW43439_RAM_SIZE_BYTES: u32 = 512 * 1024;

/// One acquired WLAN transport lease over a CYW43439 hardware substrate.
#[derive(Debug)]
pub struct Cyw43439WlanTransportLease<'a, H: Cyw43439HardwareContract> {
    hardware: &'a mut H,
    // Firmware download advances linearly through RAM, so reprogramming the
    // 32 KiB backplane window on every 64-byte chunk is pure transport tax.
    current_backplane_window: Option<u32>,
}

impl<'a, H> Cyw43439WlanTransportLease<'a, H>
where
    H: Cyw43439HardwareContract,
{
    fn write_function_bytes(
        &mut self,
        function: Cyw43439GspiFunction,
        address: u32,
        payload: &[u8],
    ) -> Result<(), Cyw43439Error> {
        if payload.is_empty() || payload.len() > CYW43439_GSPI_BUS_MAX_BLOCK_SIZE {
            return Err(Cyw43439Error::invalid());
        }
        let packet_length = u16::try_from(payload.len()).map_err(|_| Cyw43439Error::invalid())?;
        let command = Cyw43439GspiCommand {
            write: true,
            incrementing: true,
            function,
            address,
            packet_length,
        }
        .encode()
        .ok_or_else(Cyw43439Error::invalid)?;
        let aligned_len = (payload.len() + 3) & !3;
        let mut scratch = [0_u8; 4 + CYW43439_GSPI_BUS_MAX_BLOCK_SIZE];
        scratch[..4].copy_from_slice(&command.to_le_bytes());
        scratch[4..4 + payload.len()].copy_from_slice(payload);
        self.hardware
            .write_controller_transport(Cyw43439Radio::Wifi, &scratch[..4 + aligned_len])
    }

    fn set_backplane_window(&mut self, address: u32) -> Result<(), Cyw43439Error> {
        let window = address & !CYW43439_GSPI_BACKPLANE_ADDR_MASK;
        if self.current_backplane_window == Some(window) {
            return Ok(());
        }
        self.write_register_word(
            Cyw43439GspiFunction::F1,
            CYW43439_GSPI_BACKPLANE_ADDRESS_HIGH,
            1,
            (window >> 24) & 0xff,
        )?;
        self.write_register_word(
            Cyw43439GspiFunction::F1,
            CYW43439_GSPI_BACKPLANE_ADDRESS_MID,
            1,
            (window >> 16) & 0xff,
        )?;
        self.write_register_word(
            Cyw43439GspiFunction::F1,
            CYW43439_GSPI_BACKPLANE_ADDRESS_LOW,
            1,
            (window >> 8) & 0xff,
        )?;
        self.current_backplane_window = Some(window);
        Ok(())
    }

    fn read_register_bytes(
        &mut self,
        function: Cyw43439GspiFunction,
        address: u32,
        len: usize,
        out: &mut [u8],
    ) -> Result<(), Cyw43439Error> {
        if len == 0 || len > out.len() || len > 4 {
            return Err(Cyw43439Error::invalid());
        }
        let packet_length = u16::try_from(len).map_err(|_| Cyw43439Error::invalid())?;
        let command = Cyw43439GspiCommand {
            write: false,
            incrementing: true,
            function,
            address,
            packet_length,
        }
        .encode()
        .ok_or_else(Cyw43439Error::invalid)?;
        self.hardware
            .write_controller_transport(Cyw43439Radio::Wifi, &command.to_le_bytes())?;

        let padding = if matches!(function, Cyw43439GspiFunction::F1) {
            CYW43439_GSPI_BACKPLANE_READ_PAD_LEN_BYTES
        } else {
            0
        };
        let aligned_len = (len + 3) & !3;
        let mut scratch = [0_u8; CYW43439_GSPI_BACKPLANE_READ_PAD_LEN_BYTES + 4];
        let transfer_len = padding + aligned_len;
        let read = self
            .hardware
            .read_controller_transport(Cyw43439Radio::Wifi, &mut scratch[..transfer_len])?;
        if read != transfer_len {
            return Err(Cyw43439Error::invalid());
        }
        out[..len].copy_from_slice(&scratch[padding..padding + len]);
        Ok(())
    }

    fn write_register_word(
        &mut self,
        function: Cyw43439GspiFunction,
        address: u32,
        packet_length: u16,
        value: u32,
    ) -> Result<(), Cyw43439Error> {
        let command = Cyw43439GspiCommand {
            write: true,
            incrementing: true,
            function,
            address,
            packet_length,
        }
        .encode()
        .ok_or_else(Cyw43439Error::invalid)?;
        let mut payload = [0_u8; 8];
        payload[..4].copy_from_slice(&command.to_le_bytes());
        payload[4..].copy_from_slice(&value.to_le_bytes());
        self.hardware
            .write_controller_transport(Cyw43439Radio::Wifi, &payload)
    }

    /// Acquires one WLAN transport lease from the underlying hardware substrate.
    pub fn acquire(hardware: &'a mut H) -> Result<Self, Cyw43439Error> {
        hardware.acquire_transport(Cyw43439Radio::Wifi)?;
        Ok(Self {
            hardware,
            current_backplane_window: None,
        })
    }

    /// Issues one substrate-backed millisecond delay while this transport lease is held.
    pub fn delay_ms(&self, milliseconds: u32) {
        self.hardware.delay_ms(milliseconds);
    }

    /// Gives the host one best-effort chance to progress local cooperative runtime work.
    pub fn progress_host_runtime(&self) {
        self.hardware.progress_host_runtime();
    }

    /// Reads one 32-bit F0 register value.
    pub fn read_f0_u32(&mut self, register: Cyw43439GspiF0Register) -> Result<u32, Cyw43439Error> {
        let mut out = [0_u8; 4];
        self.read_register_bytes(
            Cyw43439GspiFunction::F0,
            register.address(),
            out.len(),
            &mut out,
        )?;
        Ok(u32::from_le_bytes(out))
    }

    /// Reads one 8-bit F0 register value.
    pub fn read_f0_u8(&mut self, register: Cyw43439GspiF0Register) -> Result<u8, Cyw43439Error> {
        let mut out = [0_u8; 1];
        self.read_register_bytes(Cyw43439GspiFunction::F0, register.address(), 1, &mut out)?;
        Ok(out[0])
    }

    /// Reads one 16-bit F0 register value.
    pub fn read_f0_u16(&mut self, register: Cyw43439GspiF0Register) -> Result<u16, Cyw43439Error> {
        let mut out = [0_u8; 2];
        self.read_register_bytes(Cyw43439GspiFunction::F0, register.address(), 2, &mut out)?;
        Ok(u16::from_le_bytes(out))
    }

    /// Writes one 8-bit F0 register value.
    pub fn write_f0_u8(
        &mut self,
        register: Cyw43439GspiF0Register,
        value: u8,
    ) -> Result<(), Cyw43439Error> {
        self.write_register_word(
            Cyw43439GspiFunction::F0,
            register.address(),
            1,
            value as u32,
        )
    }

    /// Writes one 16-bit F0 register value.
    pub fn write_f0_u16(
        &mut self,
        register: Cyw43439GspiF0Register,
        value: u16,
    ) -> Result<(), Cyw43439Error> {
        self.write_register_word(
            Cyw43439GspiFunction::F0,
            register.address(),
            2,
            value as u32,
        )
    }

    /// Writes one 32-bit F0 register value.
    pub fn write_f0_u32(
        &mut self,
        register: Cyw43439GspiF0Register,
        value: u32,
    ) -> Result<(), Cyw43439Error> {
        self.write_register_word(Cyw43439GspiFunction::F0, register.address(), 4, value)
    }

    /// Reads one raw function-0 bus register value outside the fixed F0 enum.
    pub fn read_bus_u8(&mut self, address: u32) -> Result<u8, Cyw43439Error> {
        let mut out = [0_u8; 1];
        self.read_register_bytes(Cyw43439GspiFunction::F0, address, 1, &mut out)?;
        Ok(out[0])
    }

    /// Writes one raw function-0 bus register value outside the fixed F0 enum.
    pub fn write_bus_u8(&mut self, address: u32, value: u8) -> Result<(), Cyw43439Error> {
        self.write_register_word(Cyw43439GspiFunction::F0, address, 1, value as u32)
    }

    /// Reads one direct function-1 register without backplane window translation.
    pub fn read_f1_u8(&mut self, address: u32) -> Result<u8, Cyw43439Error> {
        let mut out = [0_u8; 1];
        self.read_register_bytes(Cyw43439GspiFunction::F1, address, 1, &mut out)?;
        Ok(out[0])
    }

    /// Writes one direct function-1 register without backplane window translation.
    pub fn write_f1_u8(&mut self, address: u32, value: u8) -> Result<(), Cyw43439Error> {
        self.write_register_word(Cyw43439GspiFunction::F1, address, 1, value as u32)
    }

    /// Reads one 8-bit backplane register value via F1.
    pub fn read_backplane_u8(&mut self, address: u32) -> Result<u8, Cyw43439Error> {
        let mut out = [0_u8; 1];
        self.set_backplane_window(address)?;
        let register = (address & CYW43439_GSPI_BACKPLANE_ADDR_MASK)
            | CYW43439_GSPI_BACKPLANE_ACCESS_2_4B_FLAG;
        self.read_register_bytes(Cyw43439GspiFunction::F1, register, 1, &mut out)?;
        Ok(out[0])
    }

    /// Reads one 32-bit backplane register value via F1.
    pub fn read_backplane_u32(&mut self, address: u32) -> Result<u32, Cyw43439Error> {
        let mut out = [0_u8; 4];
        self.set_backplane_window(address)?;
        let register = (address & CYW43439_GSPI_BACKPLANE_ADDR_MASK)
            | CYW43439_GSPI_BACKPLANE_ACCESS_2_4B_FLAG;
        self.read_register_bytes(Cyw43439GspiFunction::F1, register, 4, &mut out)?;
        Ok(u32::from_le_bytes(out))
    }

    /// Reads an arbitrary backplane byte slice via F1 in SPI-sized chunks.
    pub fn read_backplane_bytes(
        &mut self,
        mut address: u32,
        mut out: &mut [u8],
    ) -> Result<(), Cyw43439Error> {
        while !out.is_empty() {
            let offset_in_window = (address & CYW43439_GSPI_BACKPLANE_ADDR_MASK) as usize;
            let remaining_in_window =
                (CYW43439_GSPI_BACKPLANE_ADDR_MASK as usize + 1).saturating_sub(offset_in_window);
            let chunk_len = out
                .len()
                .min(CYW43439_GSPI_BUS_MAX_BLOCK_SIZE)
                .min(remaining_in_window);
            self.set_backplane_window(address)?;
            let register = (address & CYW43439_GSPI_BACKPLANE_ADDR_MASK)
                | CYW43439_GSPI_BACKPLANE_ACCESS_2_4B_FLAG;
            self.read_register_bytes(
                Cyw43439GspiFunction::F1,
                register,
                chunk_len,
                &mut out[..chunk_len],
            )?;
            address += chunk_len as u32;
            out = &mut out[chunk_len..];
        }
        Ok(())
    }

    /// Writes one 8-bit backplane register value via F1.
    pub fn write_backplane_u8(&mut self, address: u32, value: u8) -> Result<(), Cyw43439Error> {
        self.set_backplane_window(address)?;
        let register = (address & CYW43439_GSPI_BACKPLANE_ADDR_MASK)
            | CYW43439_GSPI_BACKPLANE_ACCESS_2_4B_FLAG;
        self.write_register_word(Cyw43439GspiFunction::F1, register, 1, value as u32)
    }

    /// Writes one 32-bit backplane register value via F1.
    pub fn write_backplane_u32(&mut self, address: u32, value: u32) -> Result<(), Cyw43439Error> {
        self.set_backplane_window(address)?;
        let register = (address & CYW43439_GSPI_BACKPLANE_ADDR_MASK)
            | CYW43439_GSPI_BACKPLANE_ACCESS_2_4B_FLAG;
        self.write_register_word(Cyw43439GspiFunction::F1, register, 4, value)
    }

    /// Writes an arbitrary backplane byte slice via F1 in SPI-sized chunks.
    pub fn write_backplane_bytes(
        &mut self,
        mut address: u32,
        mut payload: &[u8],
    ) -> Result<(), Cyw43439Error> {
        while !payload.is_empty() {
            let offset_in_window = (address & CYW43439_GSPI_BACKPLANE_ADDR_MASK) as usize;
            let remaining_in_window =
                (CYW43439_GSPI_BACKPLANE_ADDR_MASK as usize + 1).saturating_sub(offset_in_window);
            let chunk_len = payload
                .len()
                .min(CYW43439_GSPI_BUS_MAX_BLOCK_SIZE)
                .min(remaining_in_window);
            self.set_backplane_window(address)?;
            let register = (address & CYW43439_GSPI_BACKPLANE_ADDR_MASK)
                | CYW43439_GSPI_BACKPLANE_ACCESS_2_4B_FLAG;
            self.write_function_bytes(Cyw43439GspiFunction::F1, register, &payload[..chunk_len])?;
            address += chunk_len as u32;
            payload = &payload[chunk_len..];
        }
        Ok(())
    }

    /// Reads the gSPI bus-control register.
    pub fn read_bus_control(&mut self) -> Result<Cyw43439GspiBusControlFlags, Cyw43439Error> {
        Ok(Cyw43439GspiBusControlFlags::from_bits_retain(
            (self.read_f0_u32(Cyw43439GspiF0Register::BusControl)? & 0xffff) as u16,
        ))
    }

    /// Writes the gSPI bus-control register.
    pub fn write_bus_control(
        &mut self,
        flags: Cyw43439GspiBusControlFlags,
    ) -> Result<(), Cyw43439Error> {
        self.write_f0_u16(Cyw43439GspiF0Register::BusControl, flags.bits())
    }

    /// Reads the gSPI bus-status control register.
    pub fn read_bus_status_control(
        &mut self,
    ) -> Result<Cyw43439GspiBusStatusControlFlags, Cyw43439Error> {
        Ok(Cyw43439GspiBusStatusControlFlags::from_bits_retain(
            self.read_f0_u16(Cyw43439GspiF0Register::BusStatusControl)?,
        ))
    }

    /// Reads the F1 or F2 function info register.
    pub fn read_function_info(
        &mut self,
        function: Cyw43439GspiFunction,
    ) -> Result<Cyw43439GspiFunctionInfo, Cyw43439Error> {
        let register = match function {
            Cyw43439GspiFunction::F1 => Cyw43439GspiF0Register::F1Info,
            Cyw43439GspiFunction::F2 => Cyw43439GspiF0Register::F2Info,
            _ => return Err(Cyw43439Error::invalid()),
        };
        Ok(Cyw43439GspiFunctionInfo::decode(
            self.read_f0_u16(register)?,
        ))
    }

    /// Reads the documented gSPI status register.
    pub fn read_status_register(&mut self) -> Result<Cyw43439GspiStatus, Cyw43439Error> {
        Ok(Cyw43439GspiStatus::decode(
            self.read_f0_u32(Cyw43439GspiF0Register::Status)?,
        ))
    }

    /// Polls the documented F0 test register until the bring-up pattern appears or attempts are
    /// exhausted.
    pub fn poll_test_pattern(&mut self, attempts: u32) -> Result<bool, Cyw43439Error> {
        for _ in 0..attempts {
            if self.read_f0_u32(Cyw43439GspiF0Register::TestRead)? == CYW43439_GSPI_TEST_PATTERN {
                return Ok(true);
            }
        }

        Ok(false)
    }
}

impl<H> Drop for Cyw43439WlanTransportLease<'_, H>
where
    H: Cyw43439HardwareContract,
{
    fn drop(&mut self) {
        self.hardware.release_transport(Cyw43439Radio::Wifi);
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use std::collections::VecDeque;
    use std::vec::Vec;

    use fusion_hal::contract::drivers::net::bluetooth::{
        BluetoothAdapterDescriptor,
        BluetoothSupport,
    };
    use fusion_hal::contract::drivers::net::wifi::{
        WifiAdapterDescriptor,
        WifiSupport,
    };
    use crate::interface::contract::{
        Cyw43439ControllerCaps,
        Cyw43439Error,
        Cyw43439HardwareContract,
        Cyw43439Radio,
    };
    use crate::transport::{
        Cyw43439BluetoothTransport,
        Cyw43439BluetoothTransportClockProfile,
        Cyw43439TransportTopology,
    };

    use super::CYW43439_GSPI_TEST_PATTERN;
    use super::CYW43439_GSPI_BACKPLANE_READ_PAD_LEN_BYTES;
    use super::CYW43439_GSPI_SDIO_CHIP_CLOCK_CSR;
    use super::Cyw43439GspiBusControlFlags;
    use super::Cyw43439GspiCommand;
    use super::Cyw43439GspiF0Register;
    use super::Cyw43439GspiFunction;
    use super::Cyw43439GspiFunctionInfo;
    use super::Cyw43439GspiStatus;
    use super::Cyw43439GspiStatusFlags;
    use super::Cyw43439WlanTransport;
    use super::Cyw43439WlanTransportLease;

    #[derive(Debug, Default)]
    struct FakeHardware {
        transport_acquired: bool,
        writes: Vec<Vec<u8>>,
        reads: VecDeque<Vec<u8>>,
    }

    impl FakeHardware {
        fn with_reads(reads: impl IntoIterator<Item = Vec<u8>>) -> Self {
            Self {
                transport_acquired: false,
                writes: Vec::new(),
                reads: reads.into_iter().collect(),
            }
        }
    }

    impl Cyw43439HardwareContract for FakeHardware {
        fn bluetooth_support(&self) -> BluetoothSupport {
            BluetoothSupport::unsupported()
        }

        fn bluetooth_adapters(&self) -> &'static [BluetoothAdapterDescriptor] {
            &[]
        }

        fn bluetooth_transport(&self) -> Result<Cyw43439BluetoothTransport, Cyw43439Error> {
            Err(Cyw43439Error::unsupported())
        }

        fn bluetooth_transport_clock_profile(
            &self,
        ) -> Result<Cyw43439BluetoothTransportClockProfile, Cyw43439Error> {
            Err(Cyw43439Error::unsupported())
        }

        fn wifi_support(&self) -> WifiSupport {
            WifiSupport::unsupported()
        }

        fn wifi_adapters(&self) -> &'static [WifiAdapterDescriptor] {
            &[]
        }

        fn wifi_transport(&self) -> Result<Cyw43439WlanTransport, Cyw43439Error> {
            Ok(Cyw43439WlanTransport::Gspi)
        }

        fn wifi_transport_clock_profile(
            &self,
        ) -> Result<super::Cyw43439WlanTransportClockProfile, Cyw43439Error> {
            Ok(super::Cyw43439WlanTransportClockProfile::Gspi {
                target_clock_hz: Some(31_250_000),
                host_source_clock_hz: Some(150_000_000),
            })
        }

        fn transport_topology(&self) -> Result<Cyw43439TransportTopology, Cyw43439Error> {
            Ok(Cyw43439TransportTopology::SplitHostTransports)
        }

        fn controller_caps(&self, _radio: Cyw43439Radio) -> Cyw43439ControllerCaps {
            Cyw43439ControllerCaps::CLAIM_CONTROLLER
                | Cyw43439ControllerCaps::TRANSPORT_WRITE
                | Cyw43439ControllerCaps::TRANSPORT_READ
        }

        fn claim_controller(&mut self, _radio: Cyw43439Radio) -> Result<(), Cyw43439Error> {
            Ok(())
        }

        fn release_controller(&mut self, _radio: Cyw43439Radio) {}

        fn facet_enabled(&self, _radio: Cyw43439Radio) -> Result<bool, Cyw43439Error> {
            Ok(true)
        }

        fn set_facet_enabled(
            &mut self,
            _radio: Cyw43439Radio,
            _enabled: bool,
        ) -> Result<(), Cyw43439Error> {
            Ok(())
        }

        fn controller_powered(&self) -> Result<bool, Cyw43439Error> {
            Ok(true)
        }

        fn set_controller_powered(&mut self, _powered: bool) -> Result<(), Cyw43439Error> {
            Ok(())
        }

        fn set_controller_reset(&mut self, _asserted: bool) -> Result<(), Cyw43439Error> {
            Ok(())
        }

        fn set_controller_wake(&mut self, _awake: bool) -> Result<(), Cyw43439Error> {
            Ok(())
        }

        fn acquire_transport(&mut self, radio: Cyw43439Radio) -> Result<(), Cyw43439Error> {
            if radio != Cyw43439Radio::Wifi || self.transport_acquired {
                return Err(Cyw43439Error::busy());
            }
            self.transport_acquired = true;
            Ok(())
        }

        fn release_transport(&mut self, radio: Cyw43439Radio) {
            if radio == Cyw43439Radio::Wifi {
                self.transport_acquired = false;
            }
        }

        fn wait_for_controller_irq(
            &mut self,
            _radio: Cyw43439Radio,
            _timeout_ms: Option<u32>,
        ) -> Result<bool, Cyw43439Error> {
            Err(Cyw43439Error::unsupported())
        }

        fn acknowledge_controller_irq(
            &mut self,
            _radio: Cyw43439Radio,
        ) -> Result<(), Cyw43439Error> {
            Err(Cyw43439Error::unsupported())
        }

        fn write_controller_transport(
            &mut self,
            radio: Cyw43439Radio,
            payload: &[u8],
        ) -> Result<(), Cyw43439Error> {
            if radio != Cyw43439Radio::Wifi || !self.transport_acquired {
                return Err(Cyw43439Error::state_conflict());
            }
            self.writes.push(payload.to_vec());
            Ok(())
        }

        fn read_controller_transport(
            &mut self,
            radio: Cyw43439Radio,
            out: &mut [u8],
        ) -> Result<usize, Cyw43439Error> {
            if radio != Cyw43439Radio::Wifi || !self.transport_acquired {
                return Err(Cyw43439Error::state_conflict());
            }
            let payload = self
                .reads
                .pop_front()
                .ok_or_else(Cyw43439Error::resource_exhausted)?;
            let len = payload.len().min(out.len());
            out[..len].copy_from_slice(&payload[..len]);
            Ok(len)
        }

        fn firmware_image(
            &self,
            _radio: Cyw43439Radio,
        ) -> Result<Option<&'static [u8]>, Cyw43439Error> {
            Err(Cyw43439Error::unsupported())
        }

        fn nvram_image(
            &self,
            _radio: Cyw43439Radio,
        ) -> Result<Option<&'static [u8]>, Cyw43439Error> {
            Err(Cyw43439Error::unsupported())
        }

        fn clm_image(&self, _radio: Cyw43439Radio) -> Result<Option<&'static [u8]>, Cyw43439Error> {
            Err(Cyw43439Error::unsupported())
        }

        fn reference_clock_hz(&self) -> Result<Option<u32>, Cyw43439Error> {
            Ok(Some(37_400_000))
        }

        fn sleep_clock_hz(&self) -> Result<Option<u32>, Cyw43439Error> {
            Ok(None)
        }

        fn delay_ms(&self, _milliseconds: u32) {}
    }

    #[test]
    fn gspi_command_round_trips() {
        let command = Cyw43439GspiCommand {
            write: true,
            incrementing: true,
            function: Cyw43439GspiFunction::F2,
            address: 0x0014,
            packet_length: 2048,
        };

        let raw = command.encode().expect("valid gspi command");
        assert_eq!(Cyw43439GspiCommand::decode(raw), command);
    }

    #[test]
    fn gspi_command_rejects_invalid_bounds() {
        let invalid_address = Cyw43439GspiCommand {
            write: false,
            incrementing: false,
            function: Cyw43439GspiFunction::F0,
            address: Cyw43439GspiCommand::MAX_ADDRESS + 1,
            packet_length: 64,
        };
        assert_eq!(invalid_address.encode(), None);

        let invalid_length = Cyw43439GspiCommand {
            packet_length: 0,
            ..Cyw43439GspiCommand {
                write: false,
                incrementing: false,
                function: Cyw43439GspiFunction::F0,
                address: 0,
                packet_length: 64,
            }
        };
        assert_eq!(invalid_length.encode(), None);

        let invalid_f1_length = Cyw43439GspiCommand {
            write: true,
            incrementing: true,
            function: Cyw43439GspiFunction::F1,
            address: 0,
            packet_length: 65,
        };
        assert_eq!(invalid_f1_length.encode(), None);
    }

    #[test]
    fn gspi_status_extracts_flags_and_length() {
        let raw = (512_u32 << 9)
            | Cyw43439GspiStatusFlags::UNDERFLOW.bits()
            | Cyw43439GspiStatusFlags::F2_PACKET_READY.bits();
        let status = Cyw43439GspiStatus::decode(raw);

        assert_eq!(status.f2_packet_length, 512);
        assert!(status.flags.contains(Cyw43439GspiStatusFlags::UNDERFLOW));
        assert!(
            status
                .flags
                .contains(Cyw43439GspiStatusFlags::F2_PACKET_READY)
        );
        assert!(!status.flags.contains(Cyw43439GspiStatusFlags::OVERFLOW));
    }

    #[test]
    fn f0_helpers_build_expected_commands() {
        let read = Cyw43439GspiCommand::read_f0(Cyw43439GspiF0Register::TestRead);
        assert_eq!(read.function, Cyw43439GspiFunction::F0);
        assert!(!read.write);
        assert!(read.incrementing);
        assert_eq!(read.address, Cyw43439GspiF0Register::TestRead.address());
        assert_eq!(read.packet_length, 4);

        let write = Cyw43439GspiCommand::write_f0(Cyw43439GspiF0Register::BusControl, 2);
        assert!(write.write);
        assert!(write.incrementing);
        assert_eq!(write.address, Cyw43439GspiF0Register::BusControl.address());
        assert_eq!(write.packet_length, 2);
    }

    #[test]
    fn function_info_extracts_shape() {
        let info = Cyw43439GspiFunctionInfo::decode((0x0800 << 2) | 0b11);
        assert!(info.enabled);
        assert!(info.ready);
        assert_eq!(info.max_packet_size, 0x0800);
    }

    #[test]
    fn documented_test_pattern_is_kept() {
        assert_eq!(CYW43439_GSPI_TEST_PATTERN, 0xFEED_BEAD);
    }

    #[test]
    fn transport_lease_releases_on_drop() {
        let mut hardware = FakeHardware::default();
        {
            let _lease = Cyw43439WlanTransportLease::acquire(&mut hardware).unwrap();
        }
        assert!(!hardware.transport_acquired);
    }

    #[test]
    fn read_bus_control_issues_expected_register_read() {
        let mut hardware = FakeHardware::with_reads([0x0033_u32.to_le_bytes().to_vec()]);
        let flags = {
            let mut lease = Cyw43439WlanTransportLease::acquire(&mut hardware).unwrap();
            lease.read_bus_control().unwrap()
        };
        assert!(flags.contains(Cyw43439GspiBusControlFlags::WORD_LENGTH_32));
        assert!(flags.contains(Cyw43439GspiBusControlFlags::BIG_ENDIAN));

        let expected = Cyw43439GspiCommand::read_f0(Cyw43439GspiF0Register::BusControl)
            .encode()
            .unwrap()
            .to_le_bytes()
            .to_vec();
        assert_eq!(hardware.writes, [expected]);
    }

    #[test]
    fn write_bus_control_issues_expected_register_write() {
        let mut hardware = FakeHardware::default();
        let flags =
            Cyw43439GspiBusControlFlags::WORD_LENGTH_32 | Cyw43439GspiBusControlFlags::WAKE_WLAN;
        {
            let mut lease = Cyw43439WlanTransportLease::acquire(&mut hardware).unwrap();
            lease.write_bus_control(flags).unwrap();
        }

        let command = Cyw43439GspiCommand::write_f0(Cyw43439GspiF0Register::BusControl, 2)
            .encode()
            .unwrap()
            .to_le_bytes();
        let mut expected = Vec::from(command);
        expected.extend_from_slice(&(flags.bits() as u32).to_le_bytes());
        assert_eq!(hardware.writes, [expected]);
    }

    #[test]
    fn poll_test_pattern_eventually_succeeds() {
        let mut hardware = FakeHardware::with_reads([
            0_u32.to_le_bytes().to_vec(),
            0_u32.to_le_bytes().to_vec(),
            CYW43439_GSPI_TEST_PATTERN.to_le_bytes().to_vec(),
        ]);
        let success = {
            let mut lease = Cyw43439WlanTransportLease::acquire(&mut hardware).unwrap();
            lease.poll_test_pattern(3).unwrap()
        };

        assert!(success);
    }

    #[test]
    fn read_function_info_decodes_f2_packet_shape() {
        let raw = (((0x0800_u16) << 2) | 0b11).to_le_bytes();
        let mut payload = [0_u8; 4];
        payload[..2].copy_from_slice(&raw);
        let mut hardware = FakeHardware::with_reads([payload.to_vec()]);
        let info = {
            let mut lease = Cyw43439WlanTransportLease::acquire(&mut hardware).unwrap();
            lease.read_function_info(Cyw43439GspiFunction::F2).unwrap()
        };
        assert!(info.enabled);
        assert!(info.ready);
        assert_eq!(info.max_packet_size, 0x0800);
    }

    #[test]
    fn read_backplane_u8_discards_response_padding() {
        let mut payload = [0_u8; CYW43439_GSPI_BACKPLANE_READ_PAD_LEN_BYTES + 4];
        payload[CYW43439_GSPI_BACKPLANE_READ_PAD_LEN_BYTES] = 0x7f;
        let mut hardware = FakeHardware::with_reads([payload.to_vec()]);
        let value = {
            let mut lease = Cyw43439WlanTransportLease::acquire(&mut hardware).unwrap();
            lease
                .read_backplane_u8(CYW43439_GSPI_SDIO_CHIP_CLOCK_CSR)
                .unwrap()
        };

        assert_eq!(value, 0x7f);
        assert_eq!(hardware.writes.len(), 4);
    }

    #[test]
    fn read_f1_u8_uses_direct_function_register_access() {
        let mut payload = [0_u8; CYW43439_GSPI_BACKPLANE_READ_PAD_LEN_BYTES + 4];
        payload[CYW43439_GSPI_BACKPLANE_READ_PAD_LEN_BYTES] = 0x80;
        let mut hardware = FakeHardware::with_reads([payload.to_vec()]);
        let value = {
            let mut lease = Cyw43439WlanTransportLease::acquire(&mut hardware).unwrap();
            lease.read_f1_u8(CYW43439_GSPI_SDIO_CHIP_CLOCK_CSR).unwrap()
        };

        assert_eq!(value, 0x80);
        let expected = Cyw43439GspiCommand {
            write: false,
            incrementing: true,
            function: Cyw43439GspiFunction::F1,
            address: CYW43439_GSPI_SDIO_CHIP_CLOCK_CSR,
            packet_length: 1,
        }
        .encode()
        .unwrap()
        .to_le_bytes()
        .to_vec();
        assert_eq!(hardware.writes, [expected]);
    }
}
