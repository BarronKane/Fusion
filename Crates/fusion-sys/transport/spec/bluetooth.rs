//! Transport-facing canonical Bluetooth frame contracts.

use fusion_hal::contract::drivers::net::bluetooth::BluetoothCanonicalFrame;

use crate::transport::TransportError;

/// Sink for canonical Bluetooth frames over any concrete transport chain.
pub trait BluetoothCanonicalFrameSinkContract {
    /// Sends one canonical Bluetooth frame.
    ///
    /// # Errors
    ///
    /// Returns one honest transport error when the underlying chain cannot carry the frame.
    fn try_send_bluetooth_frame(
        &mut self,
        frame: BluetoothCanonicalFrame<'_>,
    ) -> Result<(), TransportError>;
}

/// Source for canonical Bluetooth frames over any concrete transport chain.
pub trait BluetoothCanonicalFrameSourceContract {
    /// Receives one canonical Bluetooth frame into caller-owned storage.
    ///
    /// # Errors
    ///
    /// Returns one honest transport error when the underlying chain cannot surface the next
    /// canonical frame honestly.
    fn try_receive_bluetooth_frame<'a>(
        &mut self,
        frame_storage: &'a mut [u8],
    ) -> Result<Option<BluetoothCanonicalFrame<'a>>, TransportError>;
}
