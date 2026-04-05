//! Scanning and discovery control contracts.

use super::BluetoothError;
use super::BluetoothOwnedAdapterContract;
use super::BluetoothScanParameters;
use super::BluetoothScanReport;
use super::BluetoothScanSessionId;

/// Scanning/discovery control for one opened Bluetooth adapter.
pub trait BluetoothScanningControlContract: BluetoothOwnedAdapterContract {
    /// Starts one scan session.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the request is invalid or unsupported.
    fn start_scan(
        &mut self,
        parameters: BluetoothScanParameters,
    ) -> Result<BluetoothScanSessionId, BluetoothError>;

    /// Stops one scan session.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the session is invalid or stop fails.
    fn stop_scan(&mut self, session: BluetoothScanSessionId) -> Result<(), BluetoothError>;

    /// Pulls the next scan report into caller-owned storage.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the session is invalid or reporting fails.
    fn next_scan_report<'a>(
        &mut self,
        session: BluetoothScanSessionId,
        data: &'a mut [u8],
    ) -> Result<Option<BluetoothScanReport<'a>>, BluetoothError>;
}
