//! Connection lifecycle control contracts.

use super::BluetoothAddress;
use super::BluetoothConnectionDescriptor;
use super::BluetoothConnectionId;
use super::BluetoothConnectionParameters;
use super::BluetoothError;
use super::BluetoothOwnedAdapterContract;

/// Connection control for one opened Bluetooth adapter.
pub trait BluetoothConnectionControlContract: BluetoothOwnedAdapterContract {
    /// Creates one outbound connection to one peer.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the request is invalid or unsupported.
    fn connect(
        &mut self,
        peer: BluetoothAddress,
        parameters: BluetoothConnectionParameters,
    ) -> Result<BluetoothConnectionId, BluetoothError>;

    /// Disconnects one active connection.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the connection is invalid or disconnect fails.
    fn disconnect(&mut self, connection: BluetoothConnectionId) -> Result<(), BluetoothError>;

    /// Returns one current connection descriptor.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the connection is invalid or unavailable.
    fn connection(
        &self,
        connection: BluetoothConnectionId,
    ) -> Result<BluetoothConnectionDescriptor, BluetoothError>;
}
