//! Base provider and adapter control contracts.

use super::BluetoothAdapter;
use super::BluetoothAdapterDescriptor;
use super::BluetoothAdapterId;
use super::BluetoothAdapterSupport;
use super::BluetoothError;
use super::BluetoothSupport;

/// Capability trait for generic Bluetooth backends.
pub trait BluetoothBase {
    /// Reports the truthful Bluetooth surface for this backend.
    fn support(&self) -> BluetoothSupport;

    /// Returns the statically or dynamically surfaced Bluetooth adapter descriptors.
    #[must_use]
    fn adapters(&self) -> &'static [BluetoothAdapterDescriptor];

    /// Returns one surfaced Bluetooth adapter descriptor by stable id.
    #[must_use]
    fn adapter(&self, id: BluetoothAdapterId) -> Option<&'static BluetoothAdapterDescriptor> {
        self.adapters()
            .iter()
            .find(|descriptor| descriptor.id == id)
    }
}

/// Control contract for generic Bluetooth backends.
pub trait BluetoothControl: BluetoothBase {
    /// Concrete opened-adapter handle returned by this backend.
    type Adapter: BluetoothAdapter;

    /// Opens one surfaced Bluetooth adapter/controller.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the adapter is invalid, unsupported, or unavailable.
    fn open_adapter(
        &mut self,
        adapter: BluetoothAdapterId,
    ) -> Result<Self::Adapter, BluetoothError>;
}

/// Shared contract for one opened Bluetooth adapter/controller.
pub trait BluetoothOwnedAdapter {
    /// Returns the static surfaced adapter descriptor.
    fn descriptor(&self) -> &'static BluetoothAdapterDescriptor;

    /// Returns one truthful support snapshot for the opened adapter.
    #[must_use]
    fn capabilities(&self) -> BluetoothAdapterSupport {
        self.descriptor().support
    }
}
