//! Native domain/courier/context contract vocabulary.

mod caps;
mod error;
mod unsupported;

pub use caps::*;
pub use error::*;
pub use unsupported::*;

use crate::contract::pal::claims::{ClaimAwareness, ClaimContextId};

/// Stable identifier for one native Fusion domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DomainId(u64);

impl DomainId {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Stable identifier for one courier inside a domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CourierId(u64);

impl CourierId {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Stable identifier for one visible context/endpoint inside a domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContextId(u64);

impl ContextId {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// One native domain category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DomainKind {
    /// The local machine or Fusion substrate is the domain scope.
    NativeSubstrate,
    /// The domain is surfaced by a hosted platform.
    HostedProjection,
    /// The domain is a remote authenticated projection.
    RemoteProjection,
}

/// Effective visibility envelope for one courier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CourierVisibility {
    /// The courier can enumerate the whole visible domain surface.
    Full,
    /// The courier can enumerate only explicitly projected contexts.
    Scoped,
}

/// One context/endpoint category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContextKind {
    FiberMetadata,
    ChannelEndpoint,
    DeviceEndpoint,
    ServiceEndpoint,
    DebugEndpoint,
    TerminalEndpoint,
    StorageEndpoint,
    MemoryEndpoint,
    Custom,
}

/// How one context is surfaced to the observing courier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContextProjectionKind {
    /// The context is owned directly by the courier.
    Owned,
    /// The context is delegated from another courier.
    Delegated,
    /// The context is an alias or projection of another owning surface.
    Alias,
}

/// Full support surface for one domain implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DomainSupport {
    pub caps: DomainCaps,
    pub implementation: DomainImplementationKind,
    pub kind: DomainKind,
}

impl DomainSupport {
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: DomainCaps::empty(),
            implementation: DomainImplementationKind::Unsupported,
            kind: DomainKind::NativeSubstrate,
        }
    }
}

/// Full support surface for one courier implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CourierSupport {
    pub caps: CourierCaps,
    pub implementation: CourierImplementationKind,
    pub domain: DomainId,
    pub visibility: CourierVisibility,
    pub claim_awareness: ClaimAwareness,
    pub claim_context: Option<ClaimContextId>,
}

impl CourierSupport {
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: CourierCaps::empty(),
            implementation: CourierImplementationKind::Unsupported,
            domain: DomainId::new(0),
            visibility: CourierVisibility::Scoped,
            claim_awareness: ClaimAwareness::Blind,
            claim_context: None,
        }
    }

    #[must_use]
    pub const fn domain_id(self) -> DomainId {
        self.domain
    }

    #[must_use]
    pub const fn visibility(self) -> CourierVisibility {
        self.visibility
    }

    #[must_use]
    pub const fn claim_awareness(self) -> ClaimAwareness {
        self.claim_awareness
    }

    #[must_use]
    pub const fn claim_context(self) -> Option<ClaimContextId> {
        self.claim_context
    }

    #[must_use]
    pub const fn is_claim_enabled(self) -> bool {
        self.claim_awareness.is_black() && self.claim_context.is_some()
    }

    #[must_use]
    pub const fn is_full_visibility(self) -> bool {
        matches!(self.visibility, CourierVisibility::Full)
    }

    #[must_use]
    pub const fn is_scoped_visibility(self) -> bool {
        matches!(self.visibility, CourierVisibility::Scoped)
    }
}

/// Full support surface for one visible context implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContextSupport {
    pub caps: ContextCaps,
    pub implementation: ContextImplementationKind,
    pub domain: DomainId,
    pub owner: CourierId,
    pub kind: ContextKind,
    pub projection: ContextProjectionKind,
    pub claim_context: Option<ClaimContextId>,
}

impl ContextSupport {
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: ContextCaps::empty(),
            implementation: ContextImplementationKind::Unsupported,
            domain: DomainId::new(0),
            owner: CourierId::new(0),
            kind: ContextKind::Custom,
            projection: ContextProjectionKind::Alias,
            claim_context: None,
        }
    }
}

/// Base contract for one native Fusion domain.
pub trait DomainBase {
    fn domain_id(&self) -> DomainId;

    fn name(&self) -> &str;

    fn domain_support(&self) -> DomainSupport;
}

/// Base contract for one native Fusion courier.
pub trait CourierBase {
    fn courier_id(&self) -> CourierId;

    fn name(&self) -> &str;

    fn courier_support(&self) -> CourierSupport;
}

/// Visibility surface for one courier.
pub trait CourierVisibilityControl: CourierBase {
    fn visible_context_count(&self) -> usize;

    fn can_observe_context(&self, context: ContextId) -> bool;
}

/// Base contract for one visible Fusion context/endpoint.
pub trait ContextBase {
    fn context_id(&self) -> ContextId;

    fn name(&self) -> &str;

    fn context_support(&self) -> ContextSupport;
}
