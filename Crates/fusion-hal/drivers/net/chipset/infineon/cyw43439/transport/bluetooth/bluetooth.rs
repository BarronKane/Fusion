//! CYW43439 Bluetooth host-transport descriptors.

use fusion_hal::contract::drivers::net::bluetooth::{
    BluetoothHciAclHeader,
    BluetoothHciCommandHeader,
    BluetoothHciEventHeader,
    BluetoothHciPacketType,
};
use crate::interface::contract::{
    Cyw43439Error,
    Cyw43439HardwareContract,
    Cyw43439Radio,
};

pub const CYW43439_BTSDIO_FWBUF_SIZE: u32 = 0x1000;
pub const CYW43439_BTFW_MEM_OFFSET: u32 = 0x1900_0000;
pub const CYW43439_BT2WLAN_PWRUP_ADDR: u32 = 0x0064_0894;
pub const CYW43439_BT2WLAN_PWRUP_WAKE: u32 = 0x03;
pub const CYW43439_BT_CTRL_REG_ADDR: u32 = 0x1800_0c7c;
pub const CYW43439_BT_HOST_CTRL_REG_ADDR: u32 = 0x1800_0d6c;
pub const CYW43439_BT_WLAN_RAM_BASE_REG_ADDR: u32 = 0x1800_0d68;
pub const CYW43439_BTSDIO_OFFSET_HOST_WRITE_BUF: u32 = 0x0000;
pub const CYW43439_BTSDIO_OFFSET_HOST_READ_BUF: u32 = CYW43439_BTSDIO_FWBUF_SIZE;
pub const CYW43439_BTSDIO_OFFSET_HOST2BT_IN: u32 = 0x2000;
pub const CYW43439_BTSDIO_OFFSET_HOST2BT_OUT: u32 = 0x2004;
pub const CYW43439_BTSDIO_OFFSET_BT2HOST_IN: u32 = 0x2008;
pub const CYW43439_BTSDIO_OFFSET_BT2HOST_OUT: u32 = 0x200c;
pub const CYW43439_BTSDIO_REG_DATA_VALID_BITMASK: u32 = 1 << 1;
pub const CYW43439_BTSDIO_REG_WAKE_BT_BITMASK: u32 = 1 << 17;
pub const CYW43439_BTSDIO_REG_SW_RDY_BITMASK: u32 = 1 << 24;
pub const CYW43439_BTSDIO_REG_BT_AWAKE_BITMASK: u32 = 1 << 8;
pub const CYW43439_BTSDIO_REG_FW_RDY_BITMASK: u32 = 1 << 24;
pub const CYW43439_BTSDIO_FW_READY_POLLING_INTERVAL_MS: u32 = 1;
pub const CYW43439_BTSDIO_FW_AWAKE_POLLING_INTERVAL_MS: u32 = 1;
pub const CYW43439_BTSDIO_FW_READY_POLLING_RETRY_COUNT: u32 = 300;
pub const CYW43439_BTSDIO_FW_AWAKE_POLLING_RETRY_COUNT: u32 = 300;
pub const CYW43439_BTFW_WAIT_TIME_MS: u32 = 150;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cyw43439BluetoothSharedBufferLayout {
    pub wlan_ram_base_addr: u32,
    pub host2bt_buf_addr: u32,
    pub host2bt_in_addr: u32,
    pub host2bt_out_addr: u32,
    pub bt2host_buf_addr: u32,
    pub bt2host_in_addr: u32,
    pub bt2host_out_addr: u32,
}

impl Cyw43439BluetoothSharedBufferLayout {
    #[must_use]
    pub const fn from_wlan_ram_base(wlan_ram_base_addr: u32) -> Self {
        Self {
            wlan_ram_base_addr,
            host2bt_buf_addr: wlan_ram_base_addr + CYW43439_BTSDIO_OFFSET_HOST_WRITE_BUF,
            host2bt_in_addr: wlan_ram_base_addr + CYW43439_BTSDIO_OFFSET_HOST2BT_IN,
            host2bt_out_addr: wlan_ram_base_addr + CYW43439_BTSDIO_OFFSET_HOST2BT_OUT,
            bt2host_buf_addr: wlan_ram_base_addr + CYW43439_BTSDIO_OFFSET_HOST_READ_BUF,
            bt2host_in_addr: wlan_ram_base_addr + CYW43439_BTSDIO_OFFSET_BT2HOST_IN,
            bt2host_out_addr: wlan_ram_base_addr + CYW43439_BTSDIO_OFFSET_BT2HOST_OUT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cyw43439BluetoothSharedBufferIndex {
    pub host2bt_in_val: u32,
    pub host2bt_out_val: u32,
    pub bt2host_in_val: u32,
    pub bt2host_out_val: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Cyw43439BluetoothPatchAddrMode {
    Unknown,
    Extended,
    Segment,
    Linear32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cyw43439BluetoothPatchState {
    addr_mode: Cyw43439BluetoothPatchAddrMode,
    hi_addr: u16,
    abs_base_addr32: u32,
}

impl Cyw43439BluetoothPatchState {
    #[must_use]
    const fn new() -> Self {
        Self {
            addr_mode: Cyw43439BluetoothPatchAddrMode::Unknown,
            hi_addr: 0,
            abs_base_addr32: 0,
        }
    }
}

fn round_up_4(value: usize) -> usize {
    (value + 3) & !3
}

#[must_use]
pub fn bt_shared_round_up_4(value: usize) -> usize {
    round_up_4(value)
}

pub fn for_each_patch_data_record(
    patch: &[u8],
    mut f: impl FnMut(u32, &[u8]) -> Result<(), Cyw43439Error>,
) -> Result<(), Cyw43439Error> {
    if patch.is_empty() {
        return Err(Cyw43439Error::invalid());
    }

    let version_len = usize::from(patch[0]);
    let Some(version_end) = version_len.checked_add(1) else {
        return Err(Cyw43439Error::invalid());
    };
    let Some(records_start) = version_end.checked_add(1) else {
        return Err(Cyw43439Error::invalid());
    };
    if records_start > patch.len() || version_end >= patch.len() {
        return Err(Cyw43439Error::invalid());
    }

    let mut cursor = records_start;
    let mut state = Cyw43439BluetoothPatchState::new();
    while cursor < patch.len() {
        let record_len = usize::from(patch[cursor]);
        cursor += 1;
        if record_len == 0 {
            break;
        }
        if cursor + 3 + record_len > patch.len() {
            return Err(Cyw43439Error::invalid());
        }
        let addr = u16::from_be_bytes([patch[cursor], patch[cursor + 1]]);
        let record_type = patch[cursor + 2];
        cursor += 3;
        let data = &patch[cursor..cursor + record_len];
        cursor += record_len;

        match record_type {
            0x00 => {
                let mut dest_addr = u32::from(addr);
                match state.addr_mode {
                    Cyw43439BluetoothPatchAddrMode::Extended => {
                        dest_addr = dest_addr.saturating_add(u32::from(state.hi_addr) << 16);
                    }
                    Cyw43439BluetoothPatchAddrMode::Segment => {
                        dest_addr = dest_addr.saturating_add(u32::from(state.hi_addr) << 4);
                    }
                    Cyw43439BluetoothPatchAddrMode::Linear32 => {
                        dest_addr = dest_addr.saturating_add(state.abs_base_addr32);
                    }
                    Cyw43439BluetoothPatchAddrMode::Unknown => {}
                }
                f(dest_addr, data)?;
            }
            0x02 => {
                if data.len() < 2 {
                    return Err(Cyw43439Error::invalid());
                }
                state.hi_addr = u16::from_be_bytes([data[0], data[1]]);
                state.addr_mode = Cyw43439BluetoothPatchAddrMode::Segment;
            }
            0x04 => {
                if data.len() < 2 {
                    return Err(Cyw43439Error::invalid());
                }
                state.hi_addr = u16::from_be_bytes([data[0], data[1]]);
                state.addr_mode = Cyw43439BluetoothPatchAddrMode::Extended;
            }
            0x05 => {
                if data.len() < 4 {
                    return Err(Cyw43439Error::invalid());
                }
                state.abs_base_addr32 = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                state.addr_mode = Cyw43439BluetoothPatchAddrMode::Linear32;
            }
            0x01 => break,
            _ => {}
        }
    }

    Ok(())
}

/// Documented Bluetooth-facing host transport for CYW43439.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cyw43439BluetoothTransport {
    /// Standard HCI UART using the H4 packet format.
    HciUartH4,
    /// Standard HCI UART using the H5 three-wire packet format.
    HciUartH5,
    /// Board-selected shared SPI host path that tunnels Bluetooth HCI traffic over the same host
    /// transport used by the WLAN side.
    ///
    /// The CYW43439 datasheet documents a high-speed HCI UART as the canonical Bluetooth host
    /// interface. Some board stacks, including Pico W/Pico 2 W class bring-up paths, route the
    /// Bluetooth control plane over the shared CYW43 SPI lane instead. This is explicitly a
    /// board-selected escape hatch, not universal CYW43439 law.
    BoardSharedSpiHci,
}

/// Host-side transport clock/baud plan for one CYW43439 Bluetooth lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cyw43439BluetoothTransportClockProfile {
    /// UART-backed Bluetooth HCI transport with one intended baud rate.
    HciUart {
        /// Intended UART baud rate.
        target_baud: Option<u32>,
        /// Current host-side source clock feeding the transport block when one is known.
        host_source_clock_hz: Option<u64>,
    },
    /// Board-shared SPI path tunneling Bluetooth HCI traffic.
    BoardSharedSpiHci {
        /// Intended SPI transport clock rate.
        target_clock_hz: Option<u32>,
        /// Current host-side source clock feeding the transport block when one is known.
        host_source_clock_hz: Option<u64>,
    },
}

/// CYW43439 uses the contract-owned canonical Bluetooth HCI packet vocabulary.
pub type Cyw43439BluetoothPacketType = BluetoothHciPacketType;
/// CYW43439 uses the contract-owned canonical Bluetooth HCI command header.
pub type Cyw43439BluetoothCommandHeader = BluetoothHciCommandHeader;
/// CYW43439 uses the contract-owned canonical Bluetooth HCI event header.
pub type Cyw43439BluetoothEventHeader = BluetoothHciEventHeader;
/// CYW43439 uses the contract-owned canonical Bluetooth HCI ACL header.
pub type Cyw43439BluetoothAclHeader = BluetoothHciAclHeader;

/// One acquired Bluetooth transport lease over a CYW43439 hardware substrate.
#[derive(Debug)]
pub struct Cyw43439BluetoothTransportLease<'a, H: Cyw43439HardwareContract> {
    hardware: &'a mut H,
}

impl<'a, H> Cyw43439BluetoothTransportLease<'a, H>
where
    H: Cyw43439HardwareContract,
{
    /// Acquires one Bluetooth transport lease from the underlying hardware substrate.
    pub fn acquire(hardware: &'a mut H) -> Result<Self, Cyw43439Error> {
        hardware.acquire_transport(Cyw43439Radio::Bluetooth)?;
        Ok(Self { hardware })
    }

    /// Encodes and writes one packet-prefixed Bluetooth transport frame.
    pub fn write_packet(
        &mut self,
        packet_type: Cyw43439BluetoothPacketType,
        body: &[u8],
        out: &mut [u8],
    ) -> Result<usize, Cyw43439Error> {
        let Some(total_len) = body.len().checked_add(1) else {
            return Err(Cyw43439Error::resource_exhausted());
        };
        if out.len() < total_len {
            return Err(Cyw43439Error::resource_exhausted());
        }

        out[0] = packet_type.as_u8();
        out[1..total_len].copy_from_slice(body);
        self.hardware
            .write_controller_transport(Cyw43439Radio::Bluetooth, &out[..total_len])?;
        Ok(total_len)
    }

    /// Encodes and writes one HCI command packet with caller-owned scratch storage.
    pub fn write_command(
        &mut self,
        header: Cyw43439BluetoothCommandHeader,
        parameters: &[u8],
        out: &mut [u8],
    ) -> Result<usize, Cyw43439Error> {
        if usize::from(header.parameter_length) != parameters.len() {
            return Err(Cyw43439Error::invalid());
        }

        let header_bytes = header.encode();
        let Some(body_len) = header_bytes.len().checked_add(parameters.len()) else {
            return Err(Cyw43439Error::resource_exhausted());
        };
        if out.len() < body_len + 1 {
            return Err(Cyw43439Error::resource_exhausted());
        }

        out[1..1 + header_bytes.len()].copy_from_slice(&header_bytes);
        out[1 + header_bytes.len()..1 + body_len].copy_from_slice(parameters);
        out[0] = Cyw43439BluetoothPacketType::Command.as_u8();
        self.hardware
            .write_controller_transport(Cyw43439Radio::Bluetooth, &out[..1 + body_len])?;
        Ok(1 + body_len)
    }

    /// Encodes and writes one HCI ACL packet with caller-owned scratch storage.
    pub fn write_acl(
        &mut self,
        header: Cyw43439BluetoothAclHeader,
        payload: &[u8],
        out: &mut [u8],
    ) -> Result<usize, Cyw43439Error> {
        if usize::from(header.payload_length) != payload.len() {
            return Err(Cyw43439Error::invalid());
        }

        let header_bytes = header.encode();
        let Some(body_len) = header_bytes.len().checked_add(payload.len()) else {
            return Err(Cyw43439Error::resource_exhausted());
        };
        if out.len() < body_len + 1 {
            return Err(Cyw43439Error::resource_exhausted());
        }

        out[0] = Cyw43439BluetoothPacketType::AclData.as_u8();
        out[1..1 + header_bytes.len()].copy_from_slice(&header_bytes);
        out[1 + header_bytes.len()..1 + body_len].copy_from_slice(payload);
        self.hardware
            .write_controller_transport(Cyw43439Radio::Bluetooth, &out[..1 + body_len])?;
        Ok(1 + body_len)
    }

    /// Reads one packet-prefixed Bluetooth transport frame into caller-owned storage.
    pub fn read_packet<'b>(
        &mut self,
        out: &'b mut [u8],
    ) -> Result<Option<(Cyw43439BluetoothPacketType, &'b [u8])>, Cyw43439Error> {
        let read = self
            .hardware
            .read_controller_transport(Cyw43439Radio::Bluetooth, out)?;
        if read == 0 {
            return Ok(None);
        }

        let packet_type =
            Cyw43439BluetoothPacketType::from_u8(out[0]).ok_or_else(Cyw43439Error::invalid)?;
        Ok(Some((packet_type, &out[1..read])))
    }
}

impl<H> Drop for Cyw43439BluetoothTransportLease<'_, H>
where
    H: Cyw43439HardwareContract,
{
    fn drop(&mut self) {
        self.hardware.release_transport(Cyw43439Radio::Bluetooth);
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
        Cyw43439TransportTopology,
        Cyw43439WlanTransport,
        Cyw43439WlanTransportClockProfile,
    };

    use super::Cyw43439BluetoothAclHeader;
    use super::Cyw43439BluetoothCommandHeader;
    use super::Cyw43439BluetoothEventHeader;
    use super::Cyw43439BluetoothPacketType;
    use super::Cyw43439BluetoothTransport;
    use super::Cyw43439BluetoothTransportLease;

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
            Ok(Cyw43439BluetoothTransport::HciUartH4)
        }

        fn bluetooth_transport_clock_profile(
            &self,
        ) -> Result<super::Cyw43439BluetoothTransportClockProfile, Cyw43439Error> {
            Ok(super::Cyw43439BluetoothTransportClockProfile::HciUart {
                target_baud: Some(3_000_000),
                host_source_clock_hz: Some(150_000_000),
            })
        }

        fn wifi_support(&self) -> WifiSupport {
            WifiSupport::unsupported()
        }

        fn wifi_adapters(&self) -> &'static [WifiAdapterDescriptor] {
            &[]
        }

        fn wifi_transport(&self) -> Result<Cyw43439WlanTransport, Cyw43439Error> {
            Err(Cyw43439Error::unsupported())
        }

        fn wifi_transport_clock_profile(
            &self,
        ) -> Result<Cyw43439WlanTransportClockProfile, Cyw43439Error> {
            Err(Cyw43439Error::unsupported())
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
            if radio != Cyw43439Radio::Bluetooth || self.transport_acquired {
                return Err(Cyw43439Error::busy());
            }
            self.transport_acquired = true;
            Ok(())
        }

        fn release_transport(&mut self, radio: Cyw43439Radio) {
            if radio == Cyw43439Radio::Bluetooth {
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
            if radio != Cyw43439Radio::Bluetooth || !self.transport_acquired {
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
            if radio != Cyw43439Radio::Bluetooth || !self.transport_acquired {
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
    fn packet_type_round_trips() {
        let packet_types = [
            Cyw43439BluetoothPacketType::Command,
            Cyw43439BluetoothPacketType::AclData,
            Cyw43439BluetoothPacketType::ScoData,
            Cyw43439BluetoothPacketType::Event,
            Cyw43439BluetoothPacketType::IsoData,
        ];

        for packet_type in packet_types {
            assert_eq!(
                Cyw43439BluetoothPacketType::from_u8(packet_type.as_u8()),
                Some(packet_type),
            );
        }

        assert_eq!(Cyw43439BluetoothPacketType::from_u8(0), None);
    }

    #[test]
    fn command_header_round_trips() {
        let header = Cyw43439BluetoothCommandHeader {
            opcode: 0x0c03,
            parameter_length: 3,
        };
        assert_eq!(
            Cyw43439BluetoothCommandHeader::decode(header.encode()),
            header,
        );
    }

    #[test]
    fn event_header_round_trips() {
        let header = Cyw43439BluetoothEventHeader {
            event_code: 0x0e,
            parameter_length: 4,
        };
        assert_eq!(
            Cyw43439BluetoothEventHeader::decode(header.encode()),
            header
        );
    }

    #[test]
    fn acl_header_round_trips() {
        let header = Cyw43439BluetoothAclHeader {
            handle_and_flags: 0x2041,
            payload_length: 64,
        };
        assert_eq!(Cyw43439BluetoothAclHeader::decode(header.encode()), header);
    }

    #[test]
    fn transport_lease_writes_prefixed_command_packet() {
        let mut hardware = FakeHardware::default();
        let written = {
            let mut lease = Cyw43439BluetoothTransportLease::acquire(&mut hardware).unwrap();
            let header = Cyw43439BluetoothCommandHeader {
                opcode: 0x0c03,
                parameter_length: 3,
            };
            let mut scratch = [0_u8; 16];
            lease
                .write_command(header, &[0x01, 0x02, 0x03], &mut scratch)
                .unwrap()
        };

        assert_eq!(written, 7);
        assert_eq!(hardware.writes.len(), 1);
        assert_eq!(
            hardware.writes[0],
            [
                Cyw43439BluetoothPacketType::Command.as_u8(),
                0x03,
                0x0c,
                0x03,
                0x01,
                0x02,
                0x03,
            ]
        );
    }

    #[test]
    fn transport_lease_reads_prefixed_packet() {
        let mut hardware = FakeHardware::with_reads([vec![
            Cyw43439BluetoothPacketType::Event.as_u8(),
            0x0e,
            0x01,
            0x00,
        ]]);
        {
            let mut lease = Cyw43439BluetoothTransportLease::acquire(&mut hardware).unwrap();
            let mut scratch = [0_u8; 16];
            let (packet_type, payload) = lease.read_packet(&mut scratch).unwrap().unwrap();
            assert_eq!(packet_type, Cyw43439BluetoothPacketType::Event);
            assert_eq!(payload, &[0x0e, 0x01, 0x00]);
        }
        assert!(!hardware.transport_acquired);
    }
}
