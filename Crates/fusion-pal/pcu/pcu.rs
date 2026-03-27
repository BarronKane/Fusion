//! Backend-neutral coprocessor vocabulary.

mod error;
mod generic_caps;
mod generic_types;
mod generic_unsupported;

pub use error::*;
pub use generic_caps::*;
pub use generic_types::*;
pub use generic_unsupported::*;

/// Capability trait for generic coprocessor backends.
pub trait PcuBase {
    /// Reports the truthful generic coprocessor surface for this backend.
    fn support(&self) -> PcuSupport;

    /// Returns the surfaced coprocessor device descriptors.
    #[must_use]
    fn devices(&self) -> &'static [PcuDeviceDescriptor];
}

/// Control contract for generic coprocessor backends.
pub trait PcuControl: PcuBase {
    /// Claims one coprocessor device exclusively.
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
