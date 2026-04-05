//! DriverContract-facing PCU contract vocabulary.

pub mod caps;
pub mod device;
pub mod error;
pub mod invocation;
#[path = "ir/ir.rs"]
pub mod ir;
pub mod unsupported;

pub use caps::*;
pub use device::*;
pub use error::*;
pub use invocation::*;
pub use ir::*;

pub mod protocol;
pub use protocol::*;
pub use unsupported::*;

/// Capability trait for generic PCU backends.
pub trait PcuBaseContract {
    /// Reports the truthful generic PCU surface for this backend.
    fn support(&self) -> PcuSupport;

    /// Returns the surfaced PCU executor descriptors.
    #[must_use]
    fn executors(&self) -> &'static [PcuExecutorDescriptor];

    /// Back-compat alias while the tree stops saying “device” when it means “executor.”
    #[must_use]
    fn devices(&self) -> &'static [PcuExecutorDescriptor] {
        self.executors()
    }
}

/// Control contract for generic PCU backends.
pub trait PcuControlContract: PcuBaseContract {
    /// Claims one PCU executor exclusively.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the executor is unknown, unsupported, or already claimed.
    fn claim_executor(&self, executor: PcuExecutorId) -> Result<PcuExecutorClaim, PcuError>;

    /// Releases one previously claimed executor.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the claim no longer matches backend state.
    fn release_executor(&self, claim: PcuExecutorClaim) -> Result<(), PcuError>;

    /// Back-compat alias while the tree stops saying “device” when it means “executor.”
    ///
    /// # Errors
    ///
    /// Returns any honest executor-claim failure.
    fn claim_device(&self, device: PcuDeviceId) -> Result<PcuDeviceClaim, PcuError> {
        self.claim_executor(device)
    }

    /// Back-compat alias while the tree stops saying “device” when it means “executor.”
    ///
    /// # Errors
    ///
    /// Returns any honest executor-release failure.
    fn release_device(&self, claim: PcuDeviceClaim) -> Result<(), PcuError> {
        self.release_executor(claim)
    }
}
