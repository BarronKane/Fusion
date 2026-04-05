//! Base provider and adapter control contracts.

use super::WifiAdapterContract;
use super::WifiAdapterDescriptor;
use super::WifiAdapterId;
use super::WifiAdapterSupport;
use super::WifiError;
use super::WifiSupport;

/// Capability trait for generic Wi-Fi backends.
pub trait WifiBaseContract {
    /// Reports the truthful Wi-Fi surface for this backend.
    fn support(&self) -> WifiSupport;

    /// Returns the statically or dynamically surfaced Wi-Fi adapter descriptors.
    #[must_use]
    fn adapters(&self) -> &'static [WifiAdapterDescriptor];

    /// Returns one surfaced Wi-Fi adapter descriptor by stable id.
    #[must_use]
    fn adapter(&self, id: WifiAdapterId) -> Option<&'static WifiAdapterDescriptor> {
        self.adapters()
            .iter()
            .find(|descriptor| descriptor.id == id)
    }
}

/// Control contract for generic Wi-Fi backends.
pub trait WifiControlContract: WifiBaseContract {
    /// Concrete opened-adapter handle returned by this backend.
    type Adapter: WifiAdapterContract;

    /// Opens one surfaced Wi-Fi adapter/controller.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the adapter is invalid, unsupported, or unavailable.
    fn open_adapter(&mut self, adapter: WifiAdapterId) -> Result<Self::Adapter, WifiError>;
}

/// Shared contract for one opened Wi-Fi adapter/controller.
pub trait WifiOwnedAdapterContract {
    /// Returns the static surfaced adapter descriptor.
    fn descriptor(&self) -> &'static WifiAdapterDescriptor;

    /// Returns one truthful support snapshot for the opened adapter.
    #[must_use]
    fn capabilities(&self) -> WifiAdapterSupport {
        self.descriptor().support
    }
}
