//! L2CAP channel control contracts.

use super::BluetoothConnectionId;
use super::BluetoothError;
use super::BluetoothL2capChannelDescriptor;
use super::BluetoothL2capChannelId;
use super::BluetoothL2capChannelParameters;
use super::BluetoothL2capSdu;
use super::BluetoothOwnedAdapterContract;

/// L2CAP channel control for one opened Bluetooth adapter.
pub trait BluetoothL2capControlContract: BluetoothOwnedAdapterContract {
    /// Opens one L2CAP channel on one active connection.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the request is invalid or unsupported.
    fn open_l2cap_channel(
        &mut self,
        connection: BluetoothConnectionId,
        parameters: BluetoothL2capChannelParameters,
    ) -> Result<BluetoothL2capChannelId, BluetoothError>;

    /// Closes one active L2CAP channel.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the channel is invalid or close fails.
    fn close_l2cap_channel(
        &mut self,
        channel: BluetoothL2capChannelId,
    ) -> Result<(), BluetoothError>;

    /// Returns one current L2CAP channel descriptor.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the channel is invalid or unavailable.
    fn l2cap_channel(
        &self,
        channel: BluetoothL2capChannelId,
    ) -> Result<BluetoothL2capChannelDescriptor, BluetoothError>;

    /// Sends one SDU over one open L2CAP channel.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the send fails.
    fn send_l2cap(
        &mut self,
        channel: BluetoothL2capChannelId,
        payload: &[u8],
    ) -> Result<(), BluetoothError>;

    /// Receives one SDU into caller-owned storage from one open L2CAP channel.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when receive fails.
    fn recv_l2cap<'a>(
        &mut self,
        channel: BluetoothL2capChannelId,
        out: &'a mut [u8],
    ) -> Result<Option<BluetoothL2capSdu<'a>>, BluetoothError>;
}
