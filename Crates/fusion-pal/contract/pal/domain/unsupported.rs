//! Unsupported placeholders for native domain/courier/context contracts.

use super::{
    ContextBaseContract,
    ContextId,
    ContextProjectionKind,
    ContextSupport,
    CourierBaseContract,
    CourierId,
    CourierSupport,
    CourierVisibilityControlContract,
    DomainBaseContract,
    DomainId,
    DomainSupport,
};

/// Unsupported domain placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedDomain;

impl UnsupportedDomain {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl DomainBaseContract for UnsupportedDomain {
    fn domain_id(&self) -> DomainId {
        DomainId::new(0)
    }

    fn name(&self) -> &str {
        "unsupported-domain"
    }

    fn domain_support(&self) -> DomainSupport {
        DomainSupport::unsupported()
    }
}

/// Unsupported courier placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedCourier;

impl UnsupportedCourier {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl CourierBaseContract for UnsupportedCourier {
    fn courier_id(&self) -> CourierId {
        CourierId::new(0)
    }

    fn name(&self) -> &str {
        "unsupported-courier"
    }

    fn courier_support(&self) -> CourierSupport {
        CourierSupport::unsupported()
    }
}

impl CourierVisibilityControlContract for UnsupportedCourier {
    fn visible_context_count(&self) -> usize {
        0
    }

    fn can_observe_context(&self, _context: ContextId) -> bool {
        false
    }
}

/// Unsupported visible-context placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedContext;

impl UnsupportedContext {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ContextBaseContract for UnsupportedContext {
    fn context_id(&self) -> ContextId {
        ContextId::new(0)
    }

    fn name(&self) -> &str {
        "unsupported-context"
    }

    fn context_support(&self) -> ContextSupport {
        let mut support = ContextSupport::unsupported();
        support.domain = DomainId::new(0);
        support.owner = CourierId::new(0);
        support.projection = ContextProjectionKind::Alias;
        support
    }
}
