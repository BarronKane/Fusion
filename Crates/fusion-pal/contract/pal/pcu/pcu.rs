//! Backend-neutral PCU contract vocabulary.

mod caps;
mod error;
mod types;
mod unsupported;

pub use caps::*;
pub use error::*;
pub use types::*;
pub use unsupported::*;

/// Capability trait for generic PCU backends.
pub trait PcuBase {
    /// Reports the truthful generic PCU surface for this backend.
    fn support(&self) -> PcuSupport;

    /// Returns the surfaced PCU device descriptors.
    #[must_use]
    fn devices(&self) -> &'static [PcuDeviceDescriptor];
}

/// Control contract for generic PCU backends.
pub trait PcuControl: PcuBase {
    /// Claims one PCU device exclusively.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the device is unknown, unsupported, or already claimed.
    fn claim_device(&self, device: PcuDeviceId) -> Result<PcuDeviceClaim, PcuError>;

    /// Releases one previously claimed device.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the claim no longer matches backend state.
    fn release_device(&self, claim: PcuDeviceClaim) -> Result<(), PcuError>;
}
