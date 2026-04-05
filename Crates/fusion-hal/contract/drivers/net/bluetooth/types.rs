//! Shared generic Bluetooth identifier, descriptor, and request vocabulary.

use bitflags::bitflags;

use super::super::NetVendorIdentity;
use super::caps::BluetoothAdvertisingCaps;
use super::caps::BluetoothAttCaps;
use super::caps::BluetoothConnectionCaps;
use super::caps::BluetoothGattCaps;
use super::caps::BluetoothIsoCaps;
use super::caps::BluetoothLePhyCaps;
use super::caps::BluetoothL2capCaps;
use super::caps::BluetoothRoleCaps;
use super::caps::BluetoothScanningCaps;
use super::caps::BluetoothSecurityCaps;
use super::caps::BluetoothTransportCaps;

/// Bluetooth Core Specification version identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BluetoothVersion {
    /// Major version number.
    pub major: u8,
    /// Minor version number.
    pub minor: u8,
}

impl BluetoothVersion {
    /// Creates one Bluetooth Core Specification version identifier.
    #[must_use]
    pub const fn new(major: u8, minor: u8) -> Self {
        Self { major, minor }
    }
}

/// Supported Bluetooth Core Specification version range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothVersionRange {
    /// Lowest version the adapter/provider can honestly conform to.
    pub minimum: BluetoothVersion,
    /// Highest version the adapter/provider can honestly conform to.
    pub maximum: BluetoothVersion,
}

/// Stable surfaced adapter/controller identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothAdapterId(pub u16);

/// Stable connection identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothConnectionId(pub u16);

/// Stable advertising-set identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothAdvertisingSetId(pub u8);

/// Stable scan-session identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothScanSessionId(pub u8);

/// Stable GATT service handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothGattServiceHandle(pub u16);

/// Stable ATT attribute handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothAttAttributeHandle(pub u16);

/// Stable GATT characteristic handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothGattCharacteristicHandle(pub u16);

/// Stable GATT descriptor handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothGattDescriptorHandle(pub u16);

/// Stable L2CAP protocol/service multiplexer value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothL2capPsm(pub u16);

/// Stable L2CAP channel identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothL2capChannelId(pub u16);

/// Public or random address category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BluetoothAddressKind {
    Public,
    RandomStatic,
    RandomPrivateResolvable,
    RandomPrivateNonResolvable,
    Anonymous,
}

/// One surfaced Bluetooth device address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothAddress {
    /// On-air address bytes in canonical LSB-first controller order.
    pub bytes: [u8; 6],
    /// Address category.
    pub kind: BluetoothAddressKind,
}

/// Primary Bluetooth transport family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BluetoothTransport {
    BrEdr,
    Le,
}

/// LE PHY selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BluetoothLePhy {
    Le1M,
    Le2M,
    LeCodedS2,
    LeCodedS8,
}

/// Adapter-local IO capability used during pairing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BluetoothIoCapability {
    DisplayOnly,
    DisplayYesNo,
    KeyboardOnly,
    NoInputNoOutput,
    KeyboardDisplay,
}

/// Current bond state for one peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BluetoothBondState {
    None,
    Bonding,
    Bonded,
}

/// Required security level for one operation or connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BluetoothSecurityLevel {
    Unauthenticated,
    Authenticated,
    SecureConnections,
}

/// Advertising-mode selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BluetoothAdvertisingMode {
    ConnectableUndirected,
    ScannableUndirected,
    NonConnectableUndirected,
    DirectedHighDuty,
    DirectedLowDuty,
}

/// Scan-mode selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BluetoothScanMode {
    Passive,
    Active,
}

/// One surfaced connection role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BluetoothConnectionRole {
    Central,
    Peripheral,
}

/// One surfaced L2CAP channel mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BluetoothL2capChannelMode {
    Basic,
    CreditBased,
    EnhancedCreditBased,
}

/// Full truthful support snapshot for one surfaced Bluetooth adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothAdapterSupport {
    pub transports: BluetoothTransportCaps,
    pub roles: BluetoothRoleCaps,
    pub le_phys: BluetoothLePhyCaps,
    pub advertising: BluetoothAdvertisingCaps,
    pub scanning: BluetoothScanningCaps,
    pub connection: BluetoothConnectionCaps,
    pub security: BluetoothSecurityCaps,
    pub l2cap: BluetoothL2capCaps,
    pub att: BluetoothAttCaps,
    pub gatt: BluetoothGattCaps,
    pub iso: BluetoothIsoCaps,
    pub max_connections: u16,
    pub max_advertising_sets: u8,
    pub max_periodic_advertising_sets: u8,
    pub max_att_mtu: u16,
    pub max_attribute_value_len: u16,
    pub max_l2cap_channels: u16,
    pub max_l2cap_sdu_len: u16,
}

/// Static descriptor for one surfaced Bluetooth adapter/controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothAdapterDescriptor {
    pub id: BluetoothAdapterId,
    pub name: &'static str,
    pub vendor_identity: Option<NetVendorIdentity>,
    pub address: Option<BluetoothAddress>,
    pub version: BluetoothVersionRange,
    pub support: BluetoothAdapterSupport,
}

/// Advertising parameters for one advertising set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothAdvertisingParameters {
    pub mode: BluetoothAdvertisingMode,
    pub connectable: bool,
    pub scannable: bool,
    pub discoverable: bool,
    pub anonymous: bool,
    pub interval_min_units: u32,
    pub interval_max_units: u32,
    pub primary_phy: BluetoothLePhy,
    pub secondary_phy: Option<BluetoothLePhy>,
}

/// Scanning parameters for one scan session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothScanParameters {
    pub mode: BluetoothScanMode,
    pub transport: BluetoothTransport,
    pub interval_units: u16,
    pub window_units: u16,
    pub active_duplicate_filtering: bool,
}

/// One scan report surfaced by one backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothScanReport<'a> {
    pub address: BluetoothAddress,
    pub connectable: bool,
    pub scannable: bool,
    pub directed: bool,
    pub rssi_dbm: i8,
    pub tx_power_dbm: Option<i8>,
    pub primary_phy: Option<BluetoothLePhy>,
    pub secondary_phy: Option<BluetoothLePhy>,
    pub data: &'a [u8],
}

/// Connection parameters for one outbound or updated connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothConnectionParameters {
    pub transport: BluetoothTransport,
    pub preferred_role: Option<BluetoothConnectionRole>,
    pub connection_interval_min_units: u16,
    pub connection_interval_max_units: u16,
    pub max_latency: u16,
    pub supervision_timeout_units: u16,
    pub preferred_phy: Option<BluetoothLePhy>,
}

/// One surfaced connection descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothConnectionDescriptor {
    pub id: BluetoothConnectionId,
    pub peer: BluetoothAddress,
    pub transport: BluetoothTransport,
    pub role: BluetoothConnectionRole,
    pub bonded: BluetoothBondState,
    pub encrypted: bool,
    pub authenticated: bool,
    pub mtu: Option<u16>,
    pub phy: Option<BluetoothLePhy>,
}

/// L2CAP channel parameters for opening one logical channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothL2capChannelParameters {
    pub psm: BluetoothL2capPsm,
    pub mode: BluetoothL2capChannelMode,
    pub mtu: u16,
    pub mps: Option<u16>,
    pub initial_credits: Option<u16>,
}

/// One surfaced L2CAP channel descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothL2capChannelDescriptor {
    pub id: BluetoothL2capChannelId,
    pub connection: BluetoothConnectionId,
    pub psm: BluetoothL2capPsm,
    pub mode: BluetoothL2capChannelMode,
    pub local_mtu: u16,
    pub peer_mtu: u16,
    pub local_mps: Option<u16>,
    pub peer_mps: Option<u16>,
    pub credits: Option<u16>,
}

/// Pairing/bonding request parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothPairingParameters {
    pub bond: bool,
    pub mitm_required: bool,
    pub secure_connections_required: bool,
    pub io_capability: BluetoothIoCapability,
    pub oob_present: bool,
}

bitflags! {
    /// GATT characteristic property flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct BluetoothGattProperties: u16 {
        const BROADCAST                  = 1 << 0;
        const READ                       = 1 << 1;
        const WRITE_WITHOUT_RESPONSE     = 1 << 2;
        const WRITE                      = 1 << 3;
        const NOTIFY                     = 1 << 4;
        const INDICATE                   = 1 << 5;
        const AUTHENTICATED_SIGNED_WRITE = 1 << 6;
        const EXTENDED_PROPERTIES        = 1 << 7;
    }
}

bitflags! {
    /// GATT access-permission flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct BluetoothGattPermissions: u16 {
        const READ                       = 1 << 0;
        const READ_ENCRYPTED             = 1 << 1;
        const READ_AUTHENTICATED         = 1 << 2;
        const WRITE                      = 1 << 3;
        const WRITE_ENCRYPTED            = 1 << 4;
        const WRITE_AUTHENTICATED        = 1 << 5;
    }
}

/// One published GATT descriptor definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothGattDescriptorDefinition<'a> {
    pub handle: BluetoothGattDescriptorHandle,
    pub uuid: &'a [u8],
    pub permissions: BluetoothGattPermissions,
}

/// One published GATT characteristic definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothGattCharacteristicDefinition<'a> {
    pub handle: BluetoothGattCharacteristicHandle,
    pub uuid: &'a [u8],
    pub properties: BluetoothGattProperties,
    pub permissions: BluetoothGattPermissions,
    pub descriptors: &'a [BluetoothGattDescriptorDefinition<'a>],
}

/// One published GATT service definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothGattServiceDefinition<'a> {
    pub handle: BluetoothGattServiceHandle,
    pub uuid: &'a [u8],
    pub primary: bool,
    pub characteristics: &'a [BluetoothGattCharacteristicDefinition<'a>],
}

/// One discovered GATT service range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothGattServiceRange {
    pub handle: BluetoothGattServiceHandle,
    pub end_group_handle: BluetoothAttAttributeHandle,
    pub uuid_len: u8,
    pub uuid: [u8; 16],
}

/// One borrowed attribute-value view returned into caller-owned storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothGattAttributeValue<'a> {
    pub value: &'a [u8],
    pub truncated: bool,
}

/// One borrowed ATT attribute-value view returned into caller-owned storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothAttAttributeValue<'a> {
    pub value: &'a [u8],
    pub truncated: bool,
}

/// One borrowed L2CAP SDU returned into caller-owned storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothL2capSdu<'a> {
    pub payload: &'a [u8],
    pub truncated: bool,
}

/// One discovered GATT characteristic range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothGattCharacteristicRange {
    pub handle: BluetoothGattCharacteristicHandle,
    pub value_handle: BluetoothAttAttributeHandle,
    pub uuid_len: u8,
    pub uuid: [u8; 16],
    pub properties: BluetoothGattProperties,
}

/// One discovered GATT descriptor range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothGattDescriptorRange {
    pub handle: BluetoothGattDescriptorHandle,
    pub uuid_len: u8,
    pub uuid: [u8; 16],
}
