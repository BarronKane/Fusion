//! Backend-neutral unsupported vector-ownership implementation.

use super::{
    IrqSlot,
    SlotState,
    ThreadCoreId,
    VectorBaseContract,
    VectorDispatchCookie,
    VectorDispatchLane,
    VectorError,
    VectorInlineHandler,
    VectorOwnershipControlContract,
    VectorPriority,
    VectorSealedQueryContract,
    VectorSlotBinding,
    VectorSupport,
    VectorSystemBinding,
    VectorTableBuilderControlContract,
    VectorTableMode,
};

/// Unsupported vector provider placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedVector;

/// Unsupported vector builder placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedVectorBuilder;

/// Unsupported sealed vector-table placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedSealedVectorTable;

impl UnsupportedVector {
    /// Creates a new unsupported vector provider placeholder.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl VectorBaseContract for UnsupportedVector {
    fn support(&self) -> VectorSupport {
        VectorSupport::unsupported()
    }

    fn table_mode(&self) -> VectorTableMode {
        VectorTableMode::unowned_shared()
    }

    fn slot_count(&self) -> u16 {
        0
    }

    fn slot_state(&self, _slot: IrqSlot) -> Result<SlotState, VectorError> {
        Err(VectorError::unsupported())
    }

    fn core_affinity(&self, _slot: IrqSlot) -> Result<Option<ThreadCoreId>, VectorError> {
        Err(VectorError::unsupported())
    }
}

impl VectorOwnershipControlContract for UnsupportedVector {
    type Builder = UnsupportedVectorBuilder;

    fn adopt_and_clone(&self, _mode: VectorTableMode) -> Result<Self::Builder, VectorError> {
        Err(VectorError::unsupported())
    }

    fn overlay_builder(&self, _mode: VectorTableMode) -> Result<Self::Builder, VectorError> {
        Err(VectorError::unsupported())
    }
}

impl VectorTableBuilderControlContract for UnsupportedVectorBuilder {
    type Sealed = UnsupportedSealedVectorTable;

    fn support(&self) -> VectorSupport {
        VectorSupport::unsupported()
    }

    fn mode(&self) -> VectorTableMode {
        VectorTableMode::unowned_shared()
    }

    fn bind(&mut self, _binding: VectorSlotBinding) -> Result<(), VectorError> {
        Err(VectorError::unsupported())
    }

    fn bind_system(&mut self, _binding: VectorSystemBinding) -> Result<(), VectorError> {
        Err(VectorError::unsupported())
    }

    fn unbind(&mut self, _slot: IrqSlot) -> Result<(), VectorError> {
        Err(VectorError::unsupported())
    }

    fn seal(self) -> Result<Self::Sealed, VectorError> {
        Err(VectorError::unsupported())
    }
}

impl VectorSealedQueryContract for UnsupportedSealedVectorTable {
    fn slot_state(&self, _slot: IrqSlot) -> Result<SlotState, VectorError> {
        Err(VectorError::unsupported())
    }

    fn slot_count(&self) -> u16 {
        0
    }

    fn mode(&self) -> VectorTableMode {
        VectorTableMode::unowned_shared()
    }

    fn core_affinity(&self, _slot: IrqSlot) -> Result<Option<ThreadCoreId>, VectorError> {
        Err(VectorError::unsupported())
    }

    fn is_pending(&self, _slot: IrqSlot) -> Result<bool, VectorError> {
        Err(VectorError::unsupported())
    }

    fn take_pending(
        &self,
        _lane: super::VectorDispatchLane,
        _output: &mut [super::VectorDispatchCookie],
    ) -> Result<usize, VectorError> {
        Err(VectorError::unsupported())
    }
}

/// Unsupported reserved `PendSV` binding hook.
///
/// # Errors
///
/// Always returns `unsupported()` because this backend does not own a hardware vector table.
pub const fn bind_reserved_pendsv_dispatch(
    _builder: &mut UnsupportedVectorBuilder,
    _priority: Option<VectorPriority>,
    _handler: VectorInlineHandler,
) -> Result<(), VectorError> {
    Err(VectorError::unsupported())
}

/// Unsupported reserved event-timeout wake binding hook.
///
/// # Errors
///
/// Always returns `unsupported()` because this backend does not own a hardware vector table.
pub const fn bind_reserved_event_timeout_wake(
    _builder: &mut UnsupportedVectorBuilder,
    _priority: Option<VectorPriority>,
) -> Result<(), VectorError> {
    Err(VectorError::unsupported())
}

/// Unsupported active-scope deferred-pending extraction hook.
///
/// # Errors
///
/// Always returns `unsupported()` because this backend does not own a hardware vector table.
pub const fn take_pending_active_scope(
    _lane: VectorDispatchLane,
    _output: &mut [VectorDispatchCookie],
) -> Result<usize, VectorError> {
    Err(VectorError::unsupported())
}
