//! Station-mode contracts.

use super::WifiConnectParameters;
use super::WifiConnectionDescriptor;
use super::WifiError;
use super::WifiLinkId;
use super::WifiOwnedAdapterContract;

/// Station-mode control for one opened adapter.
pub trait WifiStationControlContract: WifiOwnedAdapterContract {
    /// Joins one Wi-Fi network as a station.
    ///
    /// # Errors
    ///
    /// Returns one honest error when station mode is unsupported or the request fails.
    fn connect(&mut self, parameters: WifiConnectParameters<'_>) -> Result<WifiLinkId, WifiError>;

    /// Disconnects one station link.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the link is invalid or disconnect fails.
    fn disconnect(&mut self, link: WifiLinkId) -> Result<(), WifiError>;

    /// Returns one truthful connection descriptor.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the link is invalid or cannot be queried.
    fn connection(&self, link: WifiLinkId) -> Result<WifiConnectionDescriptor, WifiError>;

    /// Returns the current primary station link, if one exists.
    ///
    /// # Errors
    ///
    /// Returns one honest error when station state cannot be queried.
    fn current_station_link(&self) -> Result<Option<WifiLinkId>, WifiError>;

    /// Roams one active station link to one new target.
    ///
    /// # Errors
    ///
    /// Returns one honest error when roaming is unsupported or the request fails.
    fn roam(
        &mut self,
        link: WifiLinkId,
        parameters: WifiConnectParameters<'_>,
    ) -> Result<(), WifiError>;
}
