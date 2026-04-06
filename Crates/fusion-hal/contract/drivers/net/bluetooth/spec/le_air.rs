//! Canonical Bluetooth LE over-the-air packet frames.

pub use super::super::BluetoothLePhy;

/// BLE link-layer channel family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BluetoothLeAirChannelKind {
    Advertising,
    Data,
}

/// BLE advertising-channel PDU family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BluetoothLeAdvertisingPduKind {
    AdvInd,
    AdvDirectInd,
    AdvNonconnInd,
    ScanReq,
    ScanRsp,
    ConnectInd,
    AdvScanInd,
    AdvExtInd,
    AuxAdvInd,
    AuxScanReq,
    AuxScanRsp,
    AuxSyncInd,
    AuxChainInd,
    AuxConnectReq,
    AuxConnectRsp,
    Unknown(u8),
}

/// BLE data-channel PDU family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BluetoothLeDataPduKind {
    ContinuationOrEmpty,
    StartOrComplete,
    Control,
    Isochronous,
    Unknown(u8),
}

/// Optional BLE constant-tone extension payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothLeConstantToneExtension<'a> {
    pub slot_duration_us: u8,
    pub payload: &'a [u8],
}

/// Canonical BLE advertising-channel PDU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothLeAdvertisingPdu<'a> {
    pub kind: BluetoothLeAdvertisingPduKind,
    pub payload: &'a [u8],
}

/// Canonical BLE data-channel PDU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothLeDataPdu<'a> {
    pub kind: BluetoothLeDataPduKind,
    pub llid: u8,
    pub nesn: bool,
    pub sn: bool,
    pub more_data: bool,
    pub payload: &'a [u8],
}

/// Canonical BLE over-the-air PDU payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BluetoothLeAirPdu<'a> {
    Advertising(BluetoothLeAdvertisingPdu<'a>),
    Data(BluetoothLeDataPdu<'a>),
    Raw(&'a [u8]),
}

/// One canonical BLE over-the-air packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothLeAirFrame<'a> {
    pub phy: BluetoothLePhy,
    pub channel_kind: BluetoothLeAirChannelKind,
    pub channel_index: u8,
    pub preamble: u8,
    pub access_address: u32,
    pub pdu: BluetoothLeAirPdu<'a>,
    pub crc: [u8; 3],
    pub cte: Option<BluetoothLeConstantToneExtension<'a>>,
}
