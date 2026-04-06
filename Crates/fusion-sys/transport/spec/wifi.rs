//! Transport-facing canonical Wi-Fi frame contracts.

use fusion_hal::contract::drivers::net::wifi::WifiCanonicalFrame;

use crate::transport::TransportError;

/// Sink for canonical Wi-Fi frames over any concrete transport chain.
pub trait WifiCanonicalFrameSinkContract {
    /// Sends one canonical Wi-Fi frame.
    ///
    /// # Errors
    ///
    /// Returns one honest transport error when the underlying chain cannot carry the frame.
    fn try_send_wifi_frame(&mut self, frame: WifiCanonicalFrame<'_>) -> Result<(), TransportError>;
}

/// Source for canonical Wi-Fi frames over any concrete transport chain.
pub trait WifiCanonicalFrameSourceContract {
    /// Receives one canonical Wi-Fi frame into caller-owned storage.
    ///
    /// # Errors
    ///
    /// Returns one honest transport error when the underlying chain cannot surface the next
    /// canonical frame honestly.
    fn try_receive_wifi_frame<'a>(
        &mut self,
        frame_storage: &'a mut [u8],
    ) -> Result<Option<WifiCanonicalFrame<'a>>, TransportError>;
}
