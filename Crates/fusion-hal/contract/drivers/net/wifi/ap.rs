//! Hosted access-point contracts.

use super::WifiAccessPointId;
use super::WifiApConfiguration;
use super::WifiAssociatedClient;
use super::WifiError;
use super::WifiOwnedAdapterContract;

/// Hosted access-point control for one opened Wi-Fi adapter.
pub trait WifiAccessPointControlContract: WifiOwnedAdapterContract {
    /// Starts one hosted access point.
    ///
    /// # Errors
    ///
    /// Returns one honest error when hosted AP mode is unsupported or the request fails.
    fn start_access_point(
        &mut self,
        configuration: WifiApConfiguration<'_>,
    ) -> Result<WifiAccessPointId, WifiError>;

    /// Stops one hosted access point.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the AP identifier is invalid or stopping fails.
    fn stop_access_point(&mut self, ap: WifiAccessPointId) -> Result<(), WifiError>;

    /// Lists associated clients for one hosted access point into caller-owned storage.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the AP identifier is invalid or listing fails.
    fn associated_clients(
        &self,
        ap: WifiAccessPointId,
        out: &mut [WifiAssociatedClient],
    ) -> Result<usize, WifiError>;
}
