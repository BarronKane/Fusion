//! Wi-Fi mesh contracts.

use super::WifiError;
use super::WifiMeshConfiguration;
use super::WifiMeshId;
use super::WifiOwnedAdapterContract;

/// Wi-Fi mesh control for one opened adapter.
pub trait WifiMeshControlContract: WifiOwnedAdapterContract {
    /// Joins one Wi-Fi mesh.
    ///
    /// # Errors
    ///
    /// Returns one honest error when mesh mode is unsupported or the join fails.
    fn join_mesh(
        &mut self,
        configuration: WifiMeshConfiguration<'_>,
    ) -> Result<WifiMeshId, WifiError>;

    /// Leaves one active Wi-Fi mesh.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the mesh identifier is invalid or leaving fails.
    fn leave_mesh(&mut self, mesh: WifiMeshId) -> Result<(), WifiError>;
}
