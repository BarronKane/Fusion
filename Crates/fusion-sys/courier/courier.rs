//! fusion-sys courier contracts plus claim-aware mediation helpers.

pub use fusion_pal::sys::courier::*;

use crate::claims::{ClaimAwareness, ClaimContextId, ClaimsError};

/// Snapshot of one courier's public identity and support surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CourierMetadata<'a> {
    pub id: CourierId,
    pub name: &'a str,
    pub support: CourierSupport,
}

impl CourierMetadata<'_> {
    #[must_use]
    pub const fn domain_id(self) -> crate::domain::DomainId {
        self.support.domain_id()
    }

    #[must_use]
    pub const fn visibility(self) -> CourierVisibility {
        self.support.visibility()
    }

    #[must_use]
    pub const fn claim_metadata(self) -> CourierClaimMetadata {
        CourierClaimMetadata {
            awareness: self.support.claim_awareness(),
            context: self.support.claim_context(),
        }
    }
}

/// Claim-facing snapshot of one courier's current mediation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CourierClaimMetadata {
    pub awareness: ClaimAwareness,
    pub context: Option<ClaimContextId>,
}

impl CourierClaimMetadata {
    #[must_use]
    pub const fn is_enabled(self) -> bool {
        self.awareness.is_black() && self.context.is_some()
    }
}

/// Claim-facing helper surface for any courier implementation.
pub trait CourierClaims: CourierBase {
    /// Returns whether this courier is currently claim-blind or black/claim-enabled.
    fn claim_awareness(&self) -> ClaimAwareness {
        self.courier_support().claim_awareness()
    }

    /// Returns the active claim-context identifier carried by this courier, if any.
    fn claim_context(&self) -> Option<ClaimContextId> {
        self.courier_support().claim_context()
    }

    /// Returns one compact snapshot of the courier's current claim mediation state.
    fn claim_metadata(&self) -> CourierClaimMetadata {
        CourierClaimMetadata {
            awareness: self.claim_awareness(),
            context: self.claim_context(),
        }
    }

    /// Returns whether this courier can currently mediate claim requests.
    fn is_claim_enabled(&self) -> bool {
        self.claim_metadata().is_enabled()
    }

    /// Returns the live claim-context ID or one honest denial when the courier cannot mediate.
    fn require_claim_context(&self) -> Result<ClaimContextId, ClaimsError> {
        if self.claim_awareness().is_blind() {
            return Err(ClaimsError::permission_denied());
        }
        self.claim_context()
            .ok_or_else(ClaimsError::permission_denied)
    }

    /// Validates that this courier is mediating the supplied claim context.
    fn validate_claim_context(&self, expected: ClaimContextId) -> Result<(), ClaimsError> {
        validate_courier_claim_context(self.courier_support(), expected)
    }

    /// Validates that one black fiber is running under this courier's current claim context.
    fn validate_fiber_claim_context(
        &self,
        fiber_awareness: ClaimAwareness,
        fiber_claim_context: Option<ClaimContextId>,
    ) -> Result<ClaimContextId, ClaimsError> {
        validate_fiber_claim_context(self.courier_support(), fiber_awareness, fiber_claim_context)
    }
}

impl<T: CourierBase> CourierClaims for T {}

/// Readable metadata/introspection surface for one courier.
pub trait CourierIntrospection: CourierClaims {
    /// Returns one stable metadata snapshot for this courier.
    fn metadata(&self) -> CourierMetadata<'_> {
        CourierMetadata {
            id: self.courier_id(),
            name: self.name(),
            support: self.courier_support(),
        }
    }

    /// Returns the owning domain identifier for this courier.
    fn domain_id(&self) -> crate::domain::DomainId {
        self.metadata().domain_id()
    }

    /// Returns the implementation kind for this courier.
    fn implementation_kind(&self) -> CourierImplementationKind {
        self.courier_support().implementation
    }

    /// Returns the courier's capabilities.
    fn caps(&self) -> CourierCaps {
        self.courier_support().caps
    }

    /// Returns the courier's visibility mode.
    fn visibility(&self) -> CourierVisibility {
        self.metadata().visibility()
    }

    /// Returns whether the courier exposes full domain-wide context visibility.
    fn is_full_visibility(&self) -> bool {
        self.courier_support().is_full_visibility()
    }

    /// Returns whether the courier is scoped to its explicit visible-context set.
    fn is_scoped_visibility(&self) -> bool {
        self.courier_support().is_scoped_visibility()
    }
}

impl<T: CourierBase> CourierIntrospection for T {}

/// Validates that one courier can mediate claims for the supplied claim context.
///
/// # Errors
///
/// Returns an honest denial when the courier is claim-blind or carries a different claim context.
pub fn validate_courier_claim_context(
    support: CourierSupport,
    expected: ClaimContextId,
) -> Result<(), ClaimsError> {
    if support.claim_awareness().is_blind() {
        return Err(ClaimsError::permission_denied());
    }
    if support.claim_context() != Some(expected) {
        return Err(ClaimsError::permission_denied());
    }
    Ok(())
}

/// Validates that one black fiber is still running under one courier-mediated claim context.
///
/// # Errors
///
/// Returns an honest denial when either side is claim-blind or the fiber points at a different
/// claim context than the courier currently mediates.
pub fn validate_fiber_claim_context(
    support: CourierSupport,
    fiber_awareness: ClaimAwareness,
    fiber_claim_context: Option<ClaimContextId>,
) -> Result<ClaimContextId, ClaimsError> {
    if !fiber_awareness.is_black() || support.claim_awareness().is_blind() {
        return Err(ClaimsError::permission_denied());
    }
    let context = support
        .claim_context()
        .ok_or_else(ClaimsError::permission_denied)?;
    if fiber_claim_context != Some(context) {
        return Err(ClaimsError::permission_denied());
    }
    Ok(context)
}

#[cfg(all(test, feature = "std", not(target_os = "none")))]
mod tests {
    use super::*;

    use crate::domain::DomainId;

    #[derive(Debug, Clone, Copy)]
    struct DemoCourier {
        id: CourierId,
        support: CourierSupport,
    }

    impl CourierBase for DemoCourier {
        fn courier_id(&self) -> CourierId {
            self.id
        }

        fn name(&self) -> &str {
            "demo"
        }

        fn courier_support(&self) -> CourierSupport {
            self.support
        }
    }

    #[test]
    fn black_courier_reports_claim_enablement() {
        let courier = DemoCourier {
            id: CourierId::new(1),
            support: CourierSupport {
                caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
                implementation: CourierImplementationKind::Native,
                domain: DomainId::new(7),
                visibility: CourierVisibility::Scoped,
                claim_awareness: ClaimAwareness::Black,
                claim_context: Some(ClaimContextId::new(3)),
            },
        };
        assert!(courier.is_claim_enabled());
        assert_eq!(courier.claim_context(), Some(ClaimContextId::new(3)));
    }

    #[test]
    fn courier_metadata_surfaces_identity_and_support() {
        let courier = DemoCourier {
            id: CourierId::new(1),
            support: CourierSupport {
                caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
                implementation: CourierImplementationKind::Native,
                domain: DomainId::new(7),
                visibility: CourierVisibility::Full,
                claim_awareness: ClaimAwareness::Black,
                claim_context: Some(ClaimContextId::new(3)),
            },
        };
        let metadata = courier.metadata();
        assert_eq!(metadata.id, CourierId::new(1));
        assert_eq!(metadata.name, "demo");
        assert_eq!(metadata.domain_id(), DomainId::new(7));
        assert!(metadata.claim_metadata().is_enabled());
        assert!(courier.is_full_visibility());
    }

    #[test]
    fn claim_context_validation_denies_blind_couriers() {
        let denied = validate_courier_claim_context(
            CourierSupport {
                caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
                implementation: CourierImplementationKind::Native,
                domain: DomainId::new(7),
                visibility: CourierVisibility::Scoped,
                claim_awareness: ClaimAwareness::Blind,
                claim_context: None,
            },
            ClaimContextId::new(3),
        );
        assert!(matches!(
            denied,
            Err(error) if error.kind() == crate::claims::ClaimsErrorKind::PermissionDenied
        ));
    }

    #[test]
    fn fiber_claim_context_validation_requires_black_fiber_and_matching_context() {
        let support = CourierSupport {
            caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
            implementation: CourierImplementationKind::Native,
            domain: DomainId::new(7),
            visibility: CourierVisibility::Scoped,
            claim_awareness: ClaimAwareness::Black,
            claim_context: Some(ClaimContextId::new(3)),
        };
        assert_eq!(
            validate_fiber_claim_context(
                support,
                ClaimAwareness::Black,
                Some(ClaimContextId::new(3))
            )
            .unwrap(),
            ClaimContextId::new(3)
        );
        assert!(matches!(
            validate_fiber_claim_context(
                support,
                ClaimAwareness::Blind,
                Some(ClaimContextId::new(3))
            ),
            Err(error) if error.kind() == crate::claims::ClaimsErrorKind::PermissionDenied
        ));
    }
}
