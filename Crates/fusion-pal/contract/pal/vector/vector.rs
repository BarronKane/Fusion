//! Backend-neutral interrupt-vector ownership vocabulary.

mod caps;
mod error;
mod types;
mod unsupported;

pub use caps::*;
use crate::contract::pal::runtime::thread::ThreadCoreId;
pub use error::*;
pub use types::*;
pub use unsupported::*;

/// Capability trait for vector-ownership backends.
pub trait VectorBase {
    /// Reports the truthful vector-ownership surface for this backend.
    fn support(&self) -> VectorSupport;

    /// Returns the current table mode known to this backend.
    fn table_mode(&self) -> VectorTableMode;

    /// Returns the number of peripheral IRQ slots surfaced by this backend.
    fn slot_count(&self) -> u16;

    /// Returns the visible state of one slot.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the slot is out of range or the backend cannot observe it.
    fn slot_state(&self, slot: IrqSlot) -> Result<SlotState, VectorError>;

    /// Returns the owning core for one slot when the active mode justifies it.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the slot is out of range or the backend cannot observe the
    /// current affinity.
    fn core_affinity(&self, slot: IrqSlot) -> Result<Option<ThreadCoreId>, VectorError>;
}

/// Ownership-control trait for vector-table backends.
pub trait VectorOwnershipControl: VectorBase {
    /// Backend-defined mutable builder type.
    type Builder: VectorTableBuilderControl;

    /// Adopts the current hardware vector table into one owned builder.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the requested mode is unsupported or the backend cannot
    /// adopt the current hardware table.
    fn adopt_and_clone(&self, mode: VectorTableMode) -> Result<Self::Builder, VectorError>;

    /// Creates one overlay-compatible mutable builder.
    ///
    /// # Errors
    ///
    /// Returns an honest error when overlay-compatible registration is unsupported.
    fn overlay_builder(&self, mode: VectorTableMode) -> Result<Self::Builder, VectorError>;
}

/// Mutable builder control for vector-table ownership backends.
pub trait VectorTableBuilderControl {
    /// Backend-defined sealed table type.
    type Sealed: VectorSealedQuery;

    /// Returns the support surface captured by this builder.
    fn support(&self) -> VectorSupport;

    /// Returns the active mode of this builder.
    fn mode(&self) -> VectorTableMode;

    /// Binds one peripheral IRQ slot.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the slot is invalid, reserved, or already bound.
    fn bind(&mut self, binding: VectorSlotBinding) -> Result<(), VectorError>;

    /// Binds one system exception.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the exception is reserved or unsupported.
    fn bind_system(&mut self, binding: VectorSystemBinding) -> Result<(), VectorError>;

    /// Unbinds one peripheral IRQ slot.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the slot is invalid, foreign, or reserved.
    fn unbind(&mut self, slot: IrqSlot) -> Result<(), VectorError>;

    /// Seals the builder and returns one immutable runtime handle.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the builder fails seal-time validation.
    fn seal(self) -> Result<Self::Sealed, VectorError>;
}

/// Immutable runtime query and deferred-pending surface for one sealed vector table.
pub trait VectorSealedQuery {
    /// Returns the visible state of one slot.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the slot is out of range or the backend cannot observe it.
    fn slot_state(&self, slot: IrqSlot) -> Result<SlotState, VectorError>;

    /// Returns the number of peripheral IRQ slots surfaced by this sealed table.
    fn slot_count(&self) -> u16;

    /// Returns the active mode of this sealed table.
    fn mode(&self) -> VectorTableMode;

    /// Returns the owning core for one slot when the active mode justifies it.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the slot is out of range or the backend cannot observe the
    /// current affinity.
    fn core_affinity(&self, slot: IrqSlot) -> Result<Option<ThreadCoreId>, VectorError>;

    /// Returns whether one slot is currently pending.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the slot is out of range or pending-state observation is
    /// unsupported.
    fn is_pending(&self, slot: IrqSlot) -> Result<bool, VectorError>;

    /// Takes up to `output.len()` deferred-dispatch cookies from one deferred lane.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the selected lane is unsupported or pending-state extraction
    /// cannot be performed.
    fn take_pending(
        &self,
        lane: VectorDispatchLane,
        output: &mut [VectorDispatchCookie],
    ) -> Result<usize, VectorError>;
}
