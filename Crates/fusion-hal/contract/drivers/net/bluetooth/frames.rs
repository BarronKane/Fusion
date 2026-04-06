//! Canonical Bluetooth frame transport control contract.

use super::BluetoothCanonicalFrame;
use super::BluetoothError;

/// Low-level canonical frame seam for Bluetooth adapters that can honestly move framed Bluetooth
/// traffic to and from the host.
///
/// This intentionally sits below the higher-level GAP/L2CAP/ATT/GATT contracts. Drivers like
/// CYW43439 stop at truthful canonical frame production/consumption; transport stacks can build
/// richer protocol policy above this seam later.
pub trait BluetoothCanonicalFrameControlContract {
    /// Waits until at least one canonical Bluetooth frame is available to read.
    ///
    /// Implementations should return `Ok(false)` on timeout without consuming any pending frame.
    fn wait_frame(&mut self, timeout_ms: Option<u32>) -> Result<bool, BluetoothError>;

    /// Sends one canonical Bluetooth frame using caller-owned scratch storage for any temporary
    /// transport encoding required by the controller backend.
    ///
    /// # Errors
    ///
    /// Returns `ResourceExhausted` if `scratch` cannot hold the encoded transport frame.
    fn send_frame(
        &mut self,
        frame: BluetoothCanonicalFrame<'_>,
        scratch: &mut [u8],
    ) -> Result<(), BluetoothError>;

    /// Receives one canonical Bluetooth frame into caller-owned storage.
    ///
    /// Returns `Ok(None)` when no frame is currently available.
    fn recv_frame<'a>(
        &mut self,
        out: &'a mut [u8],
    ) -> Result<Option<BluetoothCanonicalFrame<'a>>, BluetoothError>;
}
