//! Wi-Fi scanning contracts.

use super::WifiError;
use super::WifiOwnedAdapterContract;
use super::WifiScanParameters;
use super::WifiScanReport;
use super::WifiScanSessionId;

/// Wi-Fi scan control for one opened adapter.
pub trait WifiScanControlContract: WifiOwnedAdapterContract {
    /// Starts one Wi-Fi scan session.
    ///
    /// # Errors
    ///
    /// Returns one honest error when scanning is unsupported or the request is invalid.
    fn start_scan(
        &mut self,
        parameters: WifiScanParameters,
    ) -> Result<WifiScanSessionId, WifiError>;

    /// Stops one Wi-Fi scan session.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the session is invalid or stopping fails.
    fn stop_scan(&mut self, session: WifiScanSessionId) -> Result<(), WifiError>;

    /// Returns the next scan report, if one is available.
    ///
    /// The backend writes the variable-length information elements into caller-owned storage and
    /// returns one borrowed view over that memory.
    ///
    /// # Errors
    ///
    /// Returns one honest error when scanning is unsupported or retrieval fails.
    fn next_scan_report<'a>(
        &mut self,
        session: WifiScanSessionId,
        information_elements: &'a mut [u8],
    ) -> Result<Option<WifiScanReport<'a>>, WifiError>;
}
