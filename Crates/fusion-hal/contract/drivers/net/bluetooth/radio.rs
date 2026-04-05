//! Radio and adapter power control contracts.

use super::BluetoothError;
use super::BluetoothOwnedAdapterContract;

/// Radio/base control for one opened Bluetooth adapter.
pub trait BluetoothRadioControlContract: BluetoothOwnedAdapterContract {
    /// Powers the adapter on or off.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when power control is unsupported or fails.
    fn set_powered(&mut self, powered: bool) -> Result<(), BluetoothError>;

    /// Returns whether the adapter is currently powered.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when power state cannot be queried.
    fn is_powered(&self) -> Result<bool, BluetoothError>;
}
