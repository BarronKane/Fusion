//! Zero-boilerplate introspection surface for the current execution context.

use crate::__local_syscall::{
    current_context_id as current_local_context_id,
    current_context_owning_courier,
    current_context_snapshot,
    ContextLocalSnapshot,
    LocalQualifiedCourierName,
};
use crate::domain::context::{
    ContextId,
    ContextKind,
    ContextSupport,
};
use crate::domain::{
    CourierId,
    DomainError,
    DomainId,
};

/// Returns one full snapshot of the current execution context.
///
/// # Errors
///
/// Returns an honest error when no local-syscall provider is installed or the caller is not
/// running inside managed execution.
pub fn snapshot() -> Result<ContextLocalSnapshot, DomainError> {
    current_context_snapshot()
}

/// Returns the current execution context identifier.
///
/// # Errors
///
/// Returns an honest error when the current context cannot be resolved.
pub fn id() -> Result<ContextId, DomainError> {
    current_local_context_id().or_else(|_| Ok(snapshot()?.id))
}

/// Returns the current execution context local name.
///
/// # Errors
///
/// Returns an honest error when the current context cannot be resolved.
pub fn name() -> Result<&'static str, DomainError> {
    Ok(snapshot()?.name)
}

/// Returns the current execution context support surface.
///
/// # Errors
///
/// Returns an honest error when the current context cannot be resolved.
pub fn support() -> Result<ContextSupport, DomainError> {
    Ok(snapshot()?.support)
}

/// Returns the current execution context kind.
///
/// # Errors
///
/// Returns an honest error when the current context cannot be resolved.
pub fn kind() -> Result<ContextKind, DomainError> {
    Ok(snapshot()?.kind())
}

/// Returns the owning courier identifier for the current execution context.
///
/// # Errors
///
/// Returns an honest error when the current context cannot be resolved.
pub fn courier_id() -> Result<CourierId, DomainError> {
    current_context_owning_courier().or_else(|_| Ok(snapshot()?.owning_courier))
}

/// Returns the current execution domain identifier.
///
/// # Errors
///
/// Returns an honest error when the current context cannot be resolved.
pub fn domain_id() -> Result<DomainId, DomainError> {
    Ok(snapshot()?.domain_id())
}

/// Returns the current execution domain display name.
///
/// # Errors
///
/// Returns an honest error when the current context cannot be resolved.
pub fn domain_name() -> Result<&'static str, DomainError> {
    Ok(snapshot()?.domain_name)
}

/// Returns the qualified name of the owning courier for the current execution context.
///
/// # Errors
///
/// Returns an honest error when the current context cannot be resolved.
pub fn qualified_courier_name() -> Result<LocalQualifiedCourierName, DomainError> {
    Ok(snapshot()?.qualified_courier_name)
}
