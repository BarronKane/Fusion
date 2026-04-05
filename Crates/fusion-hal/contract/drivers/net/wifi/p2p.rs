//! Wi-Fi Direct contracts.

use super::WifiError;
use super::WifiOwnedAdapterContract;

/// Wi-Fi Direct control for one opened adapter.
pub trait WifiP2pControlContract: WifiOwnedAdapterContract {
    /// Starts one P2P discovery flow.
    ///
    /// # Errors
    ///
    /// Returns one honest error when Wi-Fi Direct is unsupported or discovery fails.
    fn start_p2p_discovery(&mut self) -> Result<(), WifiError>;

    /// Stops one P2P discovery flow.
    ///
    /// # Errors
    ///
    /// Returns one honest error when Wi-Fi Direct is unsupported or stopping fails.
    fn stop_p2p_discovery(&mut self) -> Result<(), WifiError>;
}
