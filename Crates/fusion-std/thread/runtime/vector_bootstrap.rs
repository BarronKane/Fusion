use fusion_sys::thread::vector::{
    VectorError,
    VectorErrorKind,
    VectorTableBuilder,
    ensure_runtime_reserved_wake_vectors_best_effort as sys_ensure_runtime_reserved_wake_vectors_best_effort,
    with_runtime_vector_builder as sys_with_runtime_vector_builder,
};

use super::ExecutorError;
use crate::sync::SyncErrorKind;

pub(crate) fn ensure_runtime_reserved_wake_vectors_best_effort() {
    sys_ensure_runtime_reserved_wake_vectors_best_effort();
}

pub(crate) fn with_runtime_vector_builder<R>(
    bind: impl FnOnce(&mut VectorTableBuilder) -> R,
) -> Result<R, ExecutorError> {
    sys_with_runtime_vector_builder(bind).map_err(executor_error_from_vector)
}

const fn executor_error_from_vector(error: VectorError) -> ExecutorError {
    match error.kind() {
        VectorErrorKind::Unsupported => ExecutorError::Unsupported,
        VectorErrorKind::Invalid
        | VectorErrorKind::Reserved
        | VectorErrorKind::CoreMismatch
        | VectorErrorKind::WorldMismatch
        | VectorErrorKind::SealViolation => ExecutorError::Sync(SyncErrorKind::Invalid),
        VectorErrorKind::AlreadyBound
        | VectorErrorKind::NotBound
        | VectorErrorKind::StateConflict
        | VectorErrorKind::Sealed => ExecutorError::Sync(SyncErrorKind::Busy),
        VectorErrorKind::ResourceExhausted => ExecutorError::Sync(SyncErrorKind::Overflow),
        VectorErrorKind::Platform(_) => ExecutorError::Sync(SyncErrorKind::Busy),
    }
}
