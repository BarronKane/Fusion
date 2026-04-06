//! Canonical Bluetooth L2CAP spec frames.

/// Logical L2CAP channel identifier on the link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothL2capChannelIdentifier(pub u16);

/// One canonical L2CAP basic header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothL2capBasicHeader {
    pub payload_length: u16,
    pub channel_id: BluetoothL2capChannelIdentifier,
}

/// One canonical L2CAP frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothL2capFrame<'a> {
    pub header: BluetoothL2capBasicHeader,
    pub payload: &'a [u8],
}
