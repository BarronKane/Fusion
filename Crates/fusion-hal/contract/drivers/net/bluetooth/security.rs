//! Security, pairing, and bonding control contracts.

use super::BluetoothAddress;
use super::BluetoothBondState;
use super::BluetoothConnectionId;
use super::BluetoothError;
use super::BluetoothOwnedAdapter;
use super::BluetoothPairingParameters;

/// Security/pairing/bonding control for one opened Bluetooth adapter.
pub trait BluetoothSecurityControl: BluetoothOwnedAdapter {
    /// Initiates or confirms pairing on one active connection.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the request is invalid or unsupported.
    fn pair(
        &mut self,
        connection: BluetoothConnectionId,
        parameters: BluetoothPairingParameters,
    ) -> Result<(), BluetoothError>;

    /// Deletes one stored bond for one peer.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the peer is unknown or deletion fails.
    fn delete_bond(&mut self, peer: BluetoothAddress) -> Result<(), BluetoothError>;

    /// Returns the current bond state for one peer.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when state cannot be queried.
    fn bond_state(&self, peer: BluetoothAddress) -> Result<BluetoothBondState, BluetoothError>;
}
