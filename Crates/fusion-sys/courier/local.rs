//! Zero-boilerplate local syscall surface for the current owning courier.

use crate::__local_syscall::{
    current_courier_id as current_local_courier_id,
    current_courier_identity,
    current_courier_snapshot,
    request_local_channel,
    request_local_service,
    resolve_local_fusion_surface_ref,
    resolve_local_qualified_courier_name,
    CourierLocalSnapshot,
    LocalCourierPedigree,
    LocalQualifiedCourierName,
    LocalSurfaceRequest,
};
use crate::courier::{
    CourierMetadata,
    CourierResponsiveness,
    CourierRuntimeLedger,
    CourierScopeRole,
    CourierSupport,
};
use crate::domain::{
    CourierId,
    DomainError,
    DomainId,
};

/// Returns one full snapshot of the current owning courier.
///
/// # Errors
///
/// Returns an honest error when no local-syscall provider is installed or the caller is not
/// running inside managed execution.
pub fn snapshot() -> Result<CourierLocalSnapshot, DomainError> {
    current_courier_snapshot()
}

/// Returns the current owning courier identifier.
///
/// # Errors
///
/// Returns an honest error when the current courier cannot be resolved.
pub fn id() -> Result<CourierId, DomainError> {
    current_local_courier_id().or_else(|_| Ok(current_courier_identity()?.id()))
}

/// Returns the current owning courier local name.
///
/// # Errors
///
/// Returns an honest error when the current courier cannot be resolved.
pub fn name() -> Result<&'static str, DomainError> {
    Ok(current_courier_identity()?.metadata.name)
}

/// Returns the current owning courier scope role.
///
/// # Errors
///
/// Returns an honest error when the current courier cannot be resolved.
pub fn scope_role() -> Result<CourierScopeRole, DomainError> {
    Ok(current_courier_identity()?.scope_role())
}

/// Returns the current owning courier support surface.
///
/// # Errors
///
/// Returns an honest error when the current courier cannot be resolved.
pub fn support() -> Result<CourierSupport, DomainError> {
    Ok(current_courier_identity()?.metadata.support)
}

/// Returns the current owning courier metadata snapshot.
///
/// # Errors
///
/// Returns an honest error when the current courier cannot be resolved.
pub fn metadata() -> Result<CourierMetadata<'static>, DomainError> {
    Ok(current_courier_identity()?.metadata)
}

/// Returns the parent courier identifier when one exists.
///
/// # Errors
///
/// Returns an honest error when the current courier cannot be resolved.
pub fn parent_id() -> Result<Option<CourierId>, DomainError> {
    Ok(current_courier_identity()?.parent)
}

/// Returns the current execution domain identifier.
///
/// # Errors
///
/// Returns an honest error when the current courier cannot be resolved.
pub fn domain_id() -> Result<DomainId, DomainError> {
    Ok(current_courier_identity()?.domain_id())
}

/// Returns the current execution domain display name.
///
/// # Errors
///
/// Returns an honest error when the current courier cannot be resolved.
pub fn domain_name() -> Result<&'static str, DomainError> {
    Ok(current_courier_identity()?.domain_name)
}

/// Returns the qualified courier name for the current owning courier.
///
/// # Errors
///
/// Returns an honest error when the current courier cannot be resolved.
pub fn qualified_name() -> Result<LocalQualifiedCourierName, DomainError> {
    Ok(current_courier_identity()?.qualified_name)
}

/// Returns the full preserved pedigree for the current owning courier.
///
/// # Errors
///
/// Returns an honest error when the current courier cannot be resolved.
pub fn pedigree() -> Result<LocalCourierPedigree, DomainError> {
    Ok(current_courier_identity()?.pedigree)
}

/// Returns the current courier runtime ledger.
///
/// # Errors
///
/// Returns an honest error when the current courier cannot be resolved.
pub fn runtime_ledger() -> Result<CourierRuntimeLedger, DomainError> {
    Ok(snapshot()?.runtime_ledger)
}

/// Returns the current courier responsiveness classification.
///
/// # Errors
///
/// Returns an honest error when the current courier cannot be resolved.
pub fn responsiveness() -> Result<CourierResponsiveness, DomainError> {
    Ok(snapshot()?.responsiveness)
}

/// Resolves one qualified courier name inside the current execution domain.
///
/// # Errors
///
/// Returns an honest error when the target courier does not resolve.
pub fn resolve_qualified_name(target: &str) -> Result<CourierId, DomainError> {
    resolve_local_qualified_courier_name(target)
}

/// Resolves one Fusion surface reference inside the current execution domain.
///
/// # Errors
///
/// Returns an honest error when the target courier authority does not resolve.
pub fn resolve_surface(target: &str) -> Result<CourierId, DomainError> {
    resolve_local_fusion_surface_ref(target)
}

/// Requests one named channel from the current owning courier.
///
/// This is the future local syscall seam for courier-owned channel registries. Today it may
/// legitimately return `unsupported` when the current runtime has no honest per-courier channel
/// registry yet.
///
/// # Errors
///
/// Returns an honest error when the provider cannot satisfy the request.
pub fn request_channel(name: &'static str) -> Result<LocalSurfaceRequest, DomainError> {
    request_local_channel(name)
}

/// Requests one named service from the current owning courier.
///
/// This is the future local syscall seam for courier-owned service registries. Today it may
/// legitimately return `unsupported` when the current runtime has no honest per-courier service
/// registry yet.
///
/// # Errors
///
/// Returns an honest error when the provider cannot satisfy the request.
pub fn request_service(name: &'static str) -> Result<LocalSurfaceRequest, DomainError> {
    request_local_service(name)
}
