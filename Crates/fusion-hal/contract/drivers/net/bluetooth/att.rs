//! ATT transaction control contracts.

use super::BluetoothAttAttributeHandle;
use super::BluetoothAttAttributeValue;
use super::BluetoothConnectionId;
use super::BluetoothError;
use super::BluetoothOwnedAdapter;

/// ATT client control for one opened Bluetooth adapter.
pub trait BluetoothAttClient: BluetoothOwnedAdapter {
    /// Negotiates ATT MTU for one active connection.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the request is invalid or unsupported.
    fn exchange_mtu(
        &mut self,
        connection: BluetoothConnectionId,
        preferred_mtu: u16,
    ) -> Result<u16, BluetoothError>;

    /// Reads one raw ATT attribute value into caller-owned storage.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the read fails.
    fn read_attribute<'a>(
        &mut self,
        connection: BluetoothConnectionId,
        attribute: BluetoothAttAttributeHandle,
        out: &'a mut [u8],
    ) -> Result<BluetoothAttAttributeValue<'a>, BluetoothError>;

    /// Writes one raw ATT attribute value.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the write fails.
    fn write_attribute(
        &mut self,
        connection: BluetoothConnectionId,
        attribute: BluetoothAttAttributeHandle,
        value: &[u8],
        with_response: bool,
    ) -> Result<(), BluetoothError>;

    /// Queues one prepared attribute write fragment.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the request is invalid or unsupported.
    fn prepare_write_attribute(
        &mut self,
        connection: BluetoothConnectionId,
        attribute: BluetoothAttAttributeHandle,
        offset: u16,
        value: &[u8],
    ) -> Result<(), BluetoothError>;

    /// Executes or cancels queued prepared writes.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when execution fails.
    fn execute_prepared_writes(
        &mut self,
        connection: BluetoothConnectionId,
        commit: bool,
    ) -> Result<(), BluetoothError>;
}
