//! Wi-Fi security-management contracts.

use super::WifiError;
use super::WifiOwnedAdapterContract;

/// Security-state control for one opened Wi-Fi adapter.
pub trait WifiSecurityControlContract: WifiOwnedAdapterContract {
    /// Clears cached security state such as remembered keys or PMK state.
    ///
    /// # Errors
    ///
    /// Returns one honest error when security-state management is unsupported or fails.
    fn clear_cached_security_state(&mut self) -> Result<(), WifiError>;

    /// Sets whether protected management frames must be required where applicable.
    ///
    /// # Errors
    ///
    /// Returns one honest error when PMF policy control is unsupported or fails.
    fn set_management_frame_protection_required(&mut self, required: bool)
    -> Result<(), WifiError>;
}
