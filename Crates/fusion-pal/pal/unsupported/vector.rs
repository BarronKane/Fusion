//! Backend-neutral unsupported vector-ownership export.

pub use crate::contract::hal::vector::{
    UnsupportedSealedVectorTable as PlatformSealedVectorTable,
    UnsupportedVector as PlatformVector,
    UnsupportedVectorBuilder as PlatformVectorBuilder,
    VectorDispatchCookie,
    VectorDispatchLane,
    VectorError,
    VectorInlineHandler,
    VectorPriority,
};

/// Returns the unsupported vector provider for the selected backend.
#[must_use]
pub const fn system_vector() -> PlatformVector {
    PlatformVector::new()
}

/// Unsupported reserved `PendSV` binding hook for selected hosted backends.
///
/// # Errors
///
/// Always returns `unsupported()` on hosted backends that do not own a hardware vector table.
pub const fn bind_reserved_pendsv_dispatch(
    _builder: &mut PlatformVectorBuilder,
    _priority: Option<VectorPriority>,
    _handler: VectorInlineHandler,
) -> Result<(), VectorError> {
    Err(VectorError::unsupported())
}

/// Unsupported active-scope deferred-pending extraction hook for selected hosted backends.
///
/// # Errors
///
/// Always returns `unsupported()` on hosted backends that do not own a hardware vector table.
pub const fn take_pending_active_scope(
    _lane: VectorDispatchLane,
    _output: &mut [VectorDispatchCookie],
) -> Result<usize, VectorError> {
    Err(VectorError::unsupported())
}
