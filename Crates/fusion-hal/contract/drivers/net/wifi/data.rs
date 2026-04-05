//! Wi-Fi data-plane contracts.

use super::WifiError;
use super::WifiLinkId;
use super::WifiOwnedAdapterContract;
use super::WifiReceivedFrame;

/// Wi-Fi data-plane control for one opened adapter.
pub trait WifiDataControlContract: WifiOwnedAdapterContract {
    /// Transmits one payload over one active Wi-Fi link.
    ///
    /// # Errors
    ///
    /// Returns one honest error when data transmission is unsupported or fails.
    fn transmit(&mut self, link: WifiLinkId, payload: &[u8]) -> Result<(), WifiError>;

    /// Receives one payload or raw Wi-Fi frame from one active link.
    ///
    /// The backend writes variable-length frame bytes into caller-owned storage and returns one
    /// borrowed view over that memory.
    ///
    /// # Errors
    ///
    /// Returns one honest error when data reception is unsupported or fails.
    fn receive<'a>(
        &mut self,
        link: WifiLinkId,
        frame: &'a mut [u8],
    ) -> Result<Option<WifiReceivedFrame<'a>>, WifiError>;
}
