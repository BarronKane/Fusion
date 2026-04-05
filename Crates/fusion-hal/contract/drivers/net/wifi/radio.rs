//! Radio and adapter-state control contracts.

use super::WifiChannelDescriptor;
use super::WifiError;
use super::WifiOwnedAdapterContract;
use super::WifiRegulatoryDomain;

/// Radio and channel-management contract for one opened Wi-Fi adapter.
pub trait WifiRadioControlContract: WifiOwnedAdapterContract {
    /// Powers the adapter radio on or off.
    ///
    /// # Errors
    ///
    /// Returns one honest error when power control is unsupported or fails.
    fn set_powered(&mut self, powered: bool) -> Result<(), WifiError>;

    /// Returns whether the adapter radio is powered.
    ///
    /// # Errors
    ///
    /// Returns one honest error when power state cannot be queried.
    fn is_powered(&self) -> Result<bool, WifiError>;

    /// Returns the currently selected channel if one exists.
    ///
    /// # Errors
    ///
    /// Returns one honest error when channel state cannot be queried.
    fn current_channel(&self) -> Result<Option<WifiChannelDescriptor>, WifiError>;

    /// Selects one channel directly.
    ///
    /// # Errors
    ///
    /// Returns one honest error when direct channel control is unsupported or invalid.
    fn set_channel(&mut self, channel: WifiChannelDescriptor) -> Result<(), WifiError>;

    /// Returns the adapter regulatory domain if surfaced.
    ///
    /// # Errors
    ///
    /// Returns one honest error when regulatory state cannot be queried.
    fn regulatory_domain(&self) -> Result<Option<WifiRegulatoryDomain>, WifiError> {
        Ok(self.descriptor().regulatory_domain)
    }
}
