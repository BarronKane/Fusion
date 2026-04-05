//! Advertising control contracts.

use super::BluetoothAdvertisingParameters;
use super::BluetoothAdvertisingSetId;
use super::BluetoothError;
use super::BluetoothOwnedAdapterContract;

/// Advertising control for one opened Bluetooth adapter.
pub trait BluetoothAdvertisingControlContract: BluetoothOwnedAdapterContract {
    /// Starts one advertising set.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the request is invalid or unsupported.
    fn start_advertising(
        &mut self,
        parameters: BluetoothAdvertisingParameters,
        data: &[u8],
        scan_response: Option<&[u8]>,
    ) -> Result<BluetoothAdvertisingSetId, BluetoothError>;

    /// Stops one advertising set.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the advertising set is invalid or stop fails.
    fn stop_advertising(
        &mut self,
        advertising_set: BluetoothAdvertisingSetId,
    ) -> Result<(), BluetoothError>;
}
