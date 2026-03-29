//! Driver-facing PCU contract vocabulary.

#[path = "caps.rs"]
pub mod caps;
#[path = "device.rs"]
pub mod device;
#[path = "error.rs"]
pub mod error;
#[path = "invocation.rs"]
pub mod invocation;
#[path = "ir/ir.rs"]
pub mod ir;
#[path = "unsupported.rs"]
pub mod unsupported;

pub use caps::*;
pub use device::*;
pub use error::*;
pub use invocation::*;
pub use ir::*;
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
