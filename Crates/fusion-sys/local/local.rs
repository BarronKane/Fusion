//! Hidden ambient local-syscall substrate for context/courier self-introspection and action.
//!
//! The public surface belongs in `context::local` and `courier::local`. This module only owns
//! the installed provider seam so `fusion-sys` can offer zero-boilerplate ambient calls without
//! learning firmware/kernel policy directly.

use crate::courier::{
    CourierMetadata,
    CourierPedigree,
    CourierResponsiveness,
    CourierRuntimeLedger,
    CourierScopeRole,
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
use crate::locator::{
    FusionSurfaceRef,
    QualifiedCourierName,
};
use crate::sync::Mutex;

pub const LOCAL_CONTEXT_CHAIN_CAPACITY: usize = 8;
pub const LOCAL_COURIER_PEDIGREE_DEPTH: usize = 8;

pub type LocalQualifiedCourierName = QualifiedCourierName<'static, LOCAL_CONTEXT_CHAIN_CAPACITY>;
pub type LocalFusionSurfaceRef = FusionSurfaceRef<'static, LOCAL_CONTEXT_CHAIN_CAPACITY>;
pub type LocalCourierPedigree = CourierPedigree<'static, LOCAL_COURIER_PEDIGREE_DEPTH>;

/// Snapshot of the current execution context observed through its owning courier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContextLocalSnapshot {
    pub id: ContextId,
    pub name: &'static str,
    pub support: ContextSupport,
    pub owning_courier: CourierId,
    pub qualified_courier_name: LocalQualifiedCourierName,
    pub domain_name: &'static str,
}

impl ContextLocalSnapshot {
    #[must_use]
    pub const fn domain_id(self) -> DomainId {
        self.support.domain
    }

    #[must_use]
    pub const fn kind(self) -> ContextKind {
        self.support.kind
    }
}

/// Snapshot of the current owning courier and its runtime state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CourierLocalIdentity {
    pub metadata: CourierMetadata<'static>,
    pub parent: Option<CourierId>,
    pub qualified_name: LocalQualifiedCourierName,
    pub pedigree: LocalCourierPedigree,
    pub domain_name: &'static str,
}

impl CourierLocalIdentity {
    #[must_use]
    pub const fn id(self) -> CourierId {
        self.metadata.id
    }

    #[must_use]
    pub const fn scope_role(self) -> CourierScopeRole {
        self.metadata.scope_role
    }

    #[must_use]
    pub const fn domain_id(self) -> DomainId {
        self.metadata.support.domain
    }
}

/// Snapshot of the current owning courier and its runtime state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CourierLocalSnapshot {
    pub identity: CourierLocalIdentity,
    pub runtime_ledger: CourierRuntimeLedger,
    pub responsiveness: CourierResponsiveness,
}

/// One ambient requested surface record rooted at one resolved owning courier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalSurfaceRequest {
    pub owner: CourierId,
    pub surface: LocalFusionSurfaceRef,
}

/// Installed provider for the public `context::local` and `courier::local` syscall facade.
#[derive(Clone, Copy)]
pub struct LocalSyscallProvider {
    pub context_id: unsafe fn() -> Result<ContextId, DomainError>,
    pub context_owning_courier: unsafe fn() -> Result<CourierId, DomainError>,
    pub courier_id: unsafe fn() -> Result<CourierId, DomainError>,
    pub context_snapshot: unsafe fn() -> Result<ContextLocalSnapshot, DomainError>,
    pub courier_identity: unsafe fn() -> Result<CourierLocalIdentity, DomainError>,
    pub courier_snapshot: unsafe fn() -> Result<CourierLocalSnapshot, DomainError>,
    pub resolve_qualified_courier_name: unsafe fn(&str) -> Result<CourierId, DomainError>,
    pub resolve_fusion_surface_ref: unsafe fn(&str) -> Result<CourierId, DomainError>,
    pub request_channel: unsafe fn(&'static str) -> Result<LocalSurfaceRequest, DomainError>,
    pub request_service: unsafe fn(&'static str) -> Result<LocalSurfaceRequest, DomainError>,
}

static LOCAL_SYSCALL_PROVIDER: Mutex<Option<LocalSyscallProvider>> = Mutex::new(None);

pub fn install_local_syscall_provider(provider: LocalSyscallProvider) -> Result<(), DomainError> {
    let mut guard = LOCAL_SYSCALL_PROVIDER
        .lock()
        .map_err(|_| DomainError::busy())?;
    *guard = Some(provider);
    Ok(())
}

fn with_provider<R>(
    f: impl FnOnce(LocalSyscallProvider) -> Result<R, DomainError>,
) -> Result<R, DomainError> {
    let provider = LOCAL_SYSCALL_PROVIDER
        .lock()
        .map_err(|_| DomainError::busy())?
        .as_ref()
        .copied()
        .ok_or_else(DomainError::unsupported)?;
    f(provider)
}

pub fn current_context_snapshot() -> Result<ContextLocalSnapshot, DomainError> {
    with_provider(|provider| unsafe { (provider.context_snapshot)() })
}

pub fn current_context_id() -> Result<ContextId, DomainError> {
    with_provider(|provider| unsafe { (provider.context_id)() })
}

pub fn current_context_owning_courier() -> Result<CourierId, DomainError> {
    with_provider(|provider| unsafe { (provider.context_owning_courier)() })
}

pub fn current_courier_snapshot() -> Result<CourierLocalSnapshot, DomainError> {
    with_provider(|provider| unsafe { (provider.courier_snapshot)() })
}

pub fn current_courier_id() -> Result<CourierId, DomainError> {
    with_provider(|provider| unsafe { (provider.courier_id)() })
}

pub fn current_courier_identity() -> Result<CourierLocalIdentity, DomainError> {
    with_provider(|provider| unsafe { (provider.courier_identity)() })
}

pub fn resolve_local_qualified_courier_name(target: &str) -> Result<CourierId, DomainError> {
    with_provider(|provider| unsafe { (provider.resolve_qualified_courier_name)(target) })
}

pub fn resolve_local_fusion_surface_ref(target: &str) -> Result<CourierId, DomainError> {
    with_provider(|provider| unsafe { (provider.resolve_fusion_surface_ref)(target) })
}

pub fn request_local_channel(name: &'static str) -> Result<LocalSurfaceRequest, DomainError> {
    with_provider(|provider| unsafe { (provider.request_channel)(name) })
}

pub fn request_local_service(name: &'static str) -> Result<LocalSurfaceRequest, DomainError> {
    with_provider(|provider| unsafe { (provider.request_service)(name) })
}
