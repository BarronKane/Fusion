//! Canonical Bluetooth HCI spec frames.

pub use super::super::{
    BluetoothAddress,
    BluetoothAddressKind,
    BluetoothHciAclHeader,
    BluetoothHciCommandHeader,
    BluetoothHciEventHeader,
    BluetoothHciFrame,
    BluetoothHciPacketType,
};

/// Canonical HCI Reset opcode.
pub const BLUETOOTH_HCI_OPCODE_RESET: u16 = 0x0c03;
/// Canonical HCI Set Event Mask opcode.
pub const BLUETOOTH_HCI_OPCODE_SET_EVENT_MASK: u16 = 0x0c01;
/// Canonical HCI Read Local Version Information opcode.
pub const BLUETOOTH_HCI_OPCODE_READ_LOCAL_VERSION_INFORMATION: u16 = 0x1001;
/// Canonical HCI Read Local Supported Commands opcode.
pub const BLUETOOTH_HCI_OPCODE_READ_LOCAL_SUPPORTED_COMMANDS: u16 = 0x1002;
/// Canonical HCI Read Local Supported Features opcode.
pub const BLUETOOTH_HCI_OPCODE_READ_LOCAL_SUPPORTED_FEATURES: u16 = 0x1003;
/// Canonical HCI Read Buffer Size opcode.
pub const BLUETOOTH_HCI_OPCODE_READ_BUFFER_SIZE: u16 = 0x1005;
/// Canonical HCI Read BD_ADDR opcode.
pub const BLUETOOTH_HCI_OPCODE_READ_BD_ADDR: u16 = 0x1009;
/// Canonical LE Read Buffer Size opcode.
pub const BLUETOOTH_HCI_OPCODE_LE_READ_BUFFER_SIZE: u16 = 0x2002;
/// Canonical LE Read Local Supported Features opcode.
pub const BLUETOOTH_HCI_OPCODE_LE_READ_LOCAL_SUPPORTED_FEATURES: u16 = 0x2003;
/// Canonical LE Set Event Mask opcode.
pub const BLUETOOTH_HCI_OPCODE_LE_SET_EVENT_MASK: u16 = 0x2001;
/// Canonical LE Set Advertising Parameters opcode.
pub const BLUETOOTH_HCI_OPCODE_LE_SET_ADVERTISING_PARAMETERS: u16 = 0x2006;
/// Canonical LE Set Advertising Data opcode.
pub const BLUETOOTH_HCI_OPCODE_LE_SET_ADVERTISING_DATA: u16 = 0x2008;
/// Canonical LE Set Scan Response Data opcode.
pub const BLUETOOTH_HCI_OPCODE_LE_SET_SCAN_RESPONSE_DATA: u16 = 0x2009;
/// Canonical LE Set Advertising Enable opcode.
pub const BLUETOOTH_HCI_OPCODE_LE_SET_ADVERTISING_ENABLE: u16 = 0x200a;
/// Canonical HCI Command Complete event code.
pub const BLUETOOTH_HCI_EVENT_COMMAND_COMPLETE: u8 = 0x0e;
/// Canonical HCI Command Status event code.
pub const BLUETOOTH_HCI_EVENT_COMMAND_STATUS: u8 = 0x0f;
/// Canonical HCI LE Meta Event code.
pub const BLUETOOTH_HCI_EVENT_LE_META: u8 = 0x3e;

/// One canonical HCI command frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothHciCommandFrame<'a> {
    pub header: BluetoothHciCommandHeader,
    pub parameters: &'a [u8],
}

/// One canonical HCI event frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothHciEventFrame<'a> {
    pub header: BluetoothHciEventHeader,
    pub parameters: &'a [u8],
}

/// One canonical HCI ACL data frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothHciAclFrame<'a> {
    pub header: BluetoothHciAclHeader,
    pub payload: &'a [u8],
}

/// One canonical HCI packet-family view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BluetoothHciFrameView<'a> {
    Command(BluetoothHciCommandFrame<'a>),
    Event(BluetoothHciEventFrame<'a>),
    Acl(BluetoothHciAclFrame<'a>),
    Sco(&'a [u8]),
    Iso(&'a [u8]),
    Opaque(BluetoothHciFrame<'a>),
}

/// Parsed Command Complete event parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothHciCommandComplete<'a> {
    pub num_hci_command_packets: u8,
    pub opcode: u16,
    pub return_parameters: &'a [u8],
}

/// Parsed Command Status event parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothHciCommandStatus {
    pub status: u8,
    pub num_hci_command_packets: u8,
    pub opcode: u16,
}

/// Parsed HCI Read Local Version Information return parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothHciLocalVersionInformation {
    pub status: u8,
    pub hci_version: u8,
    pub hci_revision: u16,
    pub lmp_pal_version: u8,
    pub manufacturer_name: u16,
    pub lmp_pal_subversion: u16,
}

/// Parsed HCI supported-command bitmap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothHciSupportedCommands {
    pub bytes: [u8; 64],
}

/// Parsed HCI supported-feature bitmap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothHciFeatureSet {
    pub bytes: [u8; 8],
}

/// Parsed HCI BR/EDR buffer sizing limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothHciBufferSize {
    pub status: u8,
    pub acl_max_data_length: u16,
    pub sco_max_data_length: u8,
    pub acl_max_packet_count: u16,
    pub sco_max_packet_count: u16,
}

/// Parsed HCI LE buffer sizing limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothHciLeBufferSize {
    pub status: u8,
    pub le_acl_max_data_length: u16,
    pub le_acl_max_packet_count: u8,
}

/// Canonical HCI event-mask bitmap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothHciEventMask {
    pub bytes: [u8; 8],
}

/// Canonical LE advertising PDU family used by the HCI legacy advertising commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum BluetoothHciLeAdvertisingType {
    ConnectableUndirected = 0x00,
    ConnectableDirectedHighDuty = 0x01,
    ScannableUndirected = 0x02,
    NonConnectableUndirected = 0x03,
    ConnectableDirectedLowDuty = 0x04,
}

/// Canonical HCI own-address type used by LE controller commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum BluetoothHciLeOwnAddressType {
    PublicDevice = 0x00,
    RandomDevice = 0x01,
    ResolvableOrPublic = 0x02,
    ResolvableOrRandom = 0x03,
}

/// Canonical HCI peer-address type used by LE controller commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum BluetoothHciLePeerAddressType {
    PublicDevice = 0x00,
    RandomDevice = 0x01,
}

/// Canonical LE advertising-channel bitmap used by legacy LE advertising commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothHciLeAdvertisingChannelMap {
    pub bits: u8,
}

/// Canonical LE advertising filter-policy values used by legacy LE advertising commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum BluetoothHciLeAdvertisingFilterPolicy {
    ProcessAll = 0x00,
    WhiteListScanOnly = 0x01,
    WhiteListConnectOnly = 0x02,
    WhiteListScanAndConnect = 0x03,
}

/// Canonical legacy LE advertising-parameter payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothHciLeAdvertisingParameters {
    pub interval_min: u16,
    pub interval_max: u16,
    pub advertising_type: BluetoothHciLeAdvertisingType,
    pub own_address_type: BluetoothHciLeOwnAddressType,
    pub peer_address_type: BluetoothHciLePeerAddressType,
    pub peer_address: BluetoothAddress,
    pub channel_map: BluetoothHciLeAdvertisingChannelMap,
    pub filter_policy: BluetoothHciLeAdvertisingFilterPolicy,
}

/// Canonical legacy LE advertising or scan-response data payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothHciLeAdvertisingData<'a> {
    pub bytes: &'a [u8],
}

impl BluetoothHciFeatureSet {
    /// Returns the feature bitmap in little-endian integer form for easier probe-side inspection.
    #[must_use]
    pub const fn as_le_u64(self) -> u64 {
        u64::from_le_bytes(self.bytes)
    }
}

impl BluetoothHciEventMask {
    /// Builds one canonical HCI event mask from a little-endian bitfield.
    #[must_use]
    pub const fn from_le_u64(bits: u64) -> Self {
        Self {
            bytes: bits.to_le_bytes(),
        }
    }
}

impl BluetoothHciLeAdvertisingChannelMap {
    pub const CHANNEL_37: Self = Self { bits: 0x01 };
    pub const CHANNEL_38: Self = Self { bits: 0x02 };
    pub const CHANNEL_39: Self = Self { bits: 0x04 };
    pub const ALL: Self = Self {
        bits: Self::CHANNEL_37.bits | Self::CHANNEL_38.bits | Self::CHANNEL_39.bits,
    };
}

impl BluetoothHciLeAdvertisingParameters {
    pub const ENCODED_LEN: usize = 15;

    /// Encodes one canonical legacy LE advertising parameter payload.
    #[must_use]
    pub fn encode(self) -> [u8; Self::ENCODED_LEN] {
        let mut out = [0_u8; Self::ENCODED_LEN];
        out[0..2].copy_from_slice(&self.interval_min.to_le_bytes());
        out[2..4].copy_from_slice(&self.interval_max.to_le_bytes());
        out[4] = self.advertising_type as u8;
        out[5] = self.own_address_type as u8;
        out[6] = self.peer_address_type as u8;
        out[7..13].copy_from_slice(&self.peer_address.bytes);
        out[13] = self.channel_map.bits;
        out[14] = self.filter_policy as u8;
        out
    }
}

impl<'a> BluetoothHciLeAdvertisingData<'a> {
    pub const ENCODED_LEN: usize = 32;

    /// Encodes one canonical legacy LE advertising or scan-response payload.
    pub fn encode(self) -> Option<[u8; BluetoothHciLeAdvertisingData::ENCODED_LEN]> {
        if self.bytes.len() > 31 {
            return None;
        }
        let mut out = [0_u8; Self::ENCODED_LEN];
        out[0] = self.bytes.len() as u8;
        out[1..1 + self.bytes.len()].copy_from_slice(self.bytes);
        Some(out)
    }
}

impl<'a> BluetoothHciFrameView<'a> {
    /// Returns the canonical HCI packet family carried by this frame.
    #[must_use]
    pub const fn packet_type(self) -> BluetoothHciPacketType {
        match self {
            Self::Command(_) => BluetoothHciPacketType::Command,
            Self::Event(_) => BluetoothHciPacketType::Event,
            Self::Acl(_) => BluetoothHciPacketType::AclData,
            Self::Sco(_) => BluetoothHciPacketType::ScoData,
            Self::Iso(_) => BluetoothHciPacketType::IsoData,
            Self::Opaque(frame) => frame.packet_type,
        }
    }
}

impl<'a> BluetoothHciEventFrame<'a> {
    /// Parses this event frame as a Command Complete event when the payload matches.
    #[must_use]
    pub fn as_command_complete(self) -> Option<BluetoothHciCommandComplete<'a>> {
        if self.header.event_code != BLUETOOTH_HCI_EVENT_COMMAND_COMPLETE
            || self.parameters.len() < 3
        {
            return None;
        }
        Some(BluetoothHciCommandComplete {
            num_hci_command_packets: self.parameters[0],
            opcode: u16::from_le_bytes([self.parameters[1], self.parameters[2]]),
            return_parameters: &self.parameters[3..],
        })
    }

    /// Parses this event frame as a Command Status event when the payload matches.
    #[must_use]
    pub fn as_command_status(self) -> Option<BluetoothHciCommandStatus> {
        if self.header.event_code != BLUETOOTH_HCI_EVENT_COMMAND_STATUS || self.parameters.len() < 4
        {
            return None;
        }
        Some(BluetoothHciCommandStatus {
            status: self.parameters[0],
            num_hci_command_packets: self.parameters[1],
            opcode: u16::from_le_bytes([self.parameters[2], self.parameters[3]]),
        })
    }
}

impl<'a> BluetoothHciCommandComplete<'a> {
    /// Parses the return parameters as Read Local Version Information.
    #[must_use]
    pub fn local_version_information(self) -> Option<BluetoothHciLocalVersionInformation> {
        if self.opcode != BLUETOOTH_HCI_OPCODE_READ_LOCAL_VERSION_INFORMATION
            || self.return_parameters.len() != 9
        {
            return None;
        }
        Some(BluetoothHciLocalVersionInformation {
            status: self.return_parameters[0],
            hci_version: self.return_parameters[1],
            hci_revision: u16::from_le_bytes([
                self.return_parameters[2],
                self.return_parameters[3],
            ]),
            lmp_pal_version: self.return_parameters[4],
            manufacturer_name: u16::from_le_bytes([
                self.return_parameters[5],
                self.return_parameters[6],
            ]),
            lmp_pal_subversion: u16::from_le_bytes([
                self.return_parameters[7],
                self.return_parameters[8],
            ]),
        })
    }

    /// Parses the return parameters as Read BD_ADDR.
    #[must_use]
    pub fn bd_addr(self) -> Option<(u8, BluetoothAddress)> {
        if self.opcode != BLUETOOTH_HCI_OPCODE_READ_BD_ADDR || self.return_parameters.len() != 7 {
            return None;
        }
        Some((
            self.return_parameters[0],
            BluetoothAddress {
                bytes: [
                    self.return_parameters[1],
                    self.return_parameters[2],
                    self.return_parameters[3],
                    self.return_parameters[4],
                    self.return_parameters[5],
                    self.return_parameters[6],
                ],
                kind: BluetoothAddressKind::Public,
            },
        ))
    }

    /// Parses the return parameters as Read Local Supported Commands.
    #[must_use]
    pub fn local_supported_commands(self) -> Option<(u8, BluetoothHciSupportedCommands)> {
        if self.opcode != BLUETOOTH_HCI_OPCODE_READ_LOCAL_SUPPORTED_COMMANDS
            || self.return_parameters.len() != 65
        {
            return None;
        }
        let mut bytes = [0_u8; 64];
        bytes.copy_from_slice(&self.return_parameters[1..]);
        Some((
            self.return_parameters[0],
            BluetoothHciSupportedCommands { bytes },
        ))
    }

    /// Parses the return parameters as Read Local Supported Features.
    #[must_use]
    pub fn local_supported_features(self) -> Option<(u8, BluetoothHciFeatureSet)> {
        if self.opcode != BLUETOOTH_HCI_OPCODE_READ_LOCAL_SUPPORTED_FEATURES
            || self.return_parameters.len() != 9
        {
            return None;
        }
        let mut bytes = [0_u8; 8];
        bytes.copy_from_slice(&self.return_parameters[1..]);
        Some((self.return_parameters[0], BluetoothHciFeatureSet { bytes }))
    }

    /// Parses the return parameters as LE Read Local Supported Features.
    #[must_use]
    pub fn le_local_supported_features(self) -> Option<(u8, BluetoothHciFeatureSet)> {
        if self.opcode != BLUETOOTH_HCI_OPCODE_LE_READ_LOCAL_SUPPORTED_FEATURES
            || self.return_parameters.len() != 9
        {
            return None;
        }
        let mut bytes = [0_u8; 8];
        bytes.copy_from_slice(&self.return_parameters[1..]);
        Some((self.return_parameters[0], BluetoothHciFeatureSet { bytes }))
    }

    /// Parses the return parameters as Read Buffer Size.
    #[must_use]
    pub fn buffer_size(self) -> Option<BluetoothHciBufferSize> {
        if self.opcode != BLUETOOTH_HCI_OPCODE_READ_BUFFER_SIZE || self.return_parameters.len() != 8
        {
            return None;
        }
        Some(BluetoothHciBufferSize {
            status: self.return_parameters[0],
            acl_max_data_length: u16::from_le_bytes([
                self.return_parameters[1],
                self.return_parameters[2],
            ]),
            sco_max_data_length: self.return_parameters[3],
            acl_max_packet_count: u16::from_le_bytes([
                self.return_parameters[4],
                self.return_parameters[5],
            ]),
            sco_max_packet_count: u16::from_le_bytes([
                self.return_parameters[6],
                self.return_parameters[7],
            ]),
        })
    }

    /// Parses the return parameters as LE Read Buffer Size.
    #[must_use]
    pub fn le_buffer_size(self) -> Option<BluetoothHciLeBufferSize> {
        if self.opcode != BLUETOOTH_HCI_OPCODE_LE_READ_BUFFER_SIZE
            || self.return_parameters.len() != 4
        {
            return None;
        }
        Some(BluetoothHciLeBufferSize {
            status: self.return_parameters[0],
            le_acl_max_data_length: u16::from_le_bytes([
                self.return_parameters[1],
                self.return_parameters[2],
            ]),
            le_acl_max_packet_count: self.return_parameters[3],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BLUETOOTH_HCI_OPCODE_LE_READ_BUFFER_SIZE,
        BLUETOOTH_HCI_OPCODE_READ_BUFFER_SIZE,
        BluetoothAddress,
        BluetoothAddressKind,
        BluetoothHciCommandComplete,
        BluetoothHciEventMask,
        BluetoothHciLeAdvertisingChannelMap,
        BluetoothHciLeAdvertisingData,
        BluetoothHciLeAdvertisingFilterPolicy,
        BluetoothHciLeAdvertisingParameters,
        BluetoothHciLeAdvertisingType,
        BluetoothHciLeOwnAddressType,
        BluetoothHciLePeerAddressType,
    };

    #[test]
    fn read_buffer_size_parses() {
        let command_complete = BluetoothHciCommandComplete {
            num_hci_command_packets: 1,
            opcode: BLUETOOTH_HCI_OPCODE_READ_BUFFER_SIZE,
            return_parameters: &[0x00, 0xfd, 0x03, 0x40, 0x08, 0x00, 0x0a, 0x00],
        };
        let buffer_size = command_complete
            .buffer_size()
            .expect("buffer size should parse");
        assert_eq!(buffer_size.status, 0);
        assert_eq!(buffer_size.acl_max_data_length, 0x03fd);
        assert_eq!(buffer_size.sco_max_data_length, 0x40);
        assert_eq!(buffer_size.acl_max_packet_count, 8);
        assert_eq!(buffer_size.sco_max_packet_count, 10);
    }

    #[test]
    fn le_read_buffer_size_parses() {
        let command_complete = BluetoothHciCommandComplete {
            num_hci_command_packets: 1,
            opcode: BLUETOOTH_HCI_OPCODE_LE_READ_BUFFER_SIZE,
            return_parameters: &[0x00, 0xfb, 0x00, 0x08],
        };
        let buffer_size = command_complete
            .le_buffer_size()
            .expect("le buffer size should parse");
        assert_eq!(buffer_size.status, 0);
        assert_eq!(buffer_size.le_acl_max_data_length, 0x00fb);
        assert_eq!(buffer_size.le_acl_max_packet_count, 8);
    }

    #[test]
    fn event_mask_encodes_from_le_bits() {
        let mask = BluetoothHciEventMask::from_le_u64(0x1122_3344_5566_7788);
        assert_eq!(mask.bytes, [0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11]);
    }

    #[test]
    fn le_advertising_parameters_encode() {
        let encoded = BluetoothHciLeAdvertisingParameters {
            interval_min: 0x00a0,
            interval_max: 0x00a0,
            advertising_type: BluetoothHciLeAdvertisingType::ConnectableUndirected,
            own_address_type: BluetoothHciLeOwnAddressType::PublicDevice,
            peer_address_type: BluetoothHciLePeerAddressType::PublicDevice,
            peer_address: BluetoothAddress {
                bytes: [1, 2, 3, 4, 5, 6],
                kind: BluetoothAddressKind::Public,
            },
            channel_map: BluetoothHciLeAdvertisingChannelMap::ALL,
            filter_policy: BluetoothHciLeAdvertisingFilterPolicy::ProcessAll,
        }
        .encode();
        assert_eq!(
            encoded.len(),
            BluetoothHciLeAdvertisingParameters::ENCODED_LEN
        );
        assert_eq!(encoded[0..4], [0xa0, 0x00, 0xa0, 0x00]);
        assert_eq!(encoded[4], 0x00);
        assert_eq!(encoded[13], 0x07);
        assert_eq!(encoded[14], 0x00);
    }

    #[test]
    fn le_advertising_data_encode() {
        let encoded = BluetoothHciLeAdvertisingData { bytes: b"Fusion" }
            .encode()
            .expect("payload should fit");
        assert_eq!(encoded[0], 6);
        assert_eq!(&encoded[1..7], b"Fusion");
    }
}
