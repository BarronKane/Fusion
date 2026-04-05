//! Monitor-mode contracts.

use super::WifiError;
use super::WifiMonitorParameters;
use super::WifiMonitorSessionId;
use super::WifiOwnedAdapterContract;
use super::WifiReceivedFrame;

/// Monitor-mode control for one opened Wi-Fi adapter.
pub trait WifiMonitorControlContract: WifiOwnedAdapterContract {
    /// Starts one monitor-mode capture session.
    ///
    /// # Errors
    ///
    /// Returns one honest error when monitor mode is unsupported or the request fails.
    fn start_monitor(
        &mut self,
        parameters: WifiMonitorParameters,
    ) -> Result<WifiMonitorSessionId, WifiError>;

    /// Stops one monitor-mode capture session.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the session is invalid or stopping fails.
    fn stop_monitor(&mut self, session: WifiMonitorSessionId) -> Result<(), WifiError>;

    /// Returns one captured frame, if one is available.
    ///
    /// The backend writes variable-length frame bytes into caller-owned storage and returns one
    /// borrowed view over that memory.
    ///
    /// # Errors
    ///
    /// Returns one honest error when monitor mode is unsupported or retrieval fails.
    fn next_monitor_frame<'a>(
        &mut self,
        session: WifiMonitorSessionId,
        frame: &'a mut [u8],
    ) -> Result<Option<WifiReceivedFrame<'a>>, WifiError>;
}
