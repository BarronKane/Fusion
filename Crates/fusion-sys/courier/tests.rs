use crate::domain::DomainId;
use crate::fiber::{
    FiberReturn,
    ManagedFiberSnapshot,
};
use super::*;

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
        validate_fiber_claim_context(support, ClaimAwareness::Black, Some(ClaimContextId::new(3)))
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

#[test]
fn child_courier_registry_tracks_observed_progress_and_responsiveness() {
    let principal = PrincipalId::parse("httpd#01@web[cache.pvas-local]:443").unwrap();
    let seal = LocalAdmissionSeal::new(
        crate::claims::ImageSealId::new(1),
        crate::claims::ClaimsDigest::zero(),
        crate::claims::ClaimsDigest::zero(),
        crate::claims::ClaimsDigest::zero(),
        47,
    );
    let mut children: ChildCourierRegistry<'_, 2> = ChildCourierRegistry::new();
    children
        .register(ChildCourierLaunchRecord::new(
            CourierId::new(2),
            CourierId::new(1),
            principal,
            seal,
            CourierClaimMetadata {
                awareness: ClaimAwareness::Black,
                context: Some(ClaimContextId::new(3)),
            },
            47,
            10,
            FiberId::new(9),
        ))
        .unwrap();
    assert_eq!(
        children.child(CourierId::new(2)).unwrap().responsiveness,
        CourierResponsiveness::Responsive
    );
    children.mark_stale(CourierId::new(2)).unwrap();
    assert_eq!(
        children.child(CourierId::new(2)).unwrap().responsiveness,
        CourierResponsiveness::Stale
    );
    children.record_progress(CourierId::new(2), 22).unwrap();
    let record = children.child(CourierId::new(2)).unwrap();
    assert_eq!(record.last_progress_tick, 22);
    assert_eq!(record.responsiveness, CourierResponsiveness::Responsive);
}

#[test]
fn courier_fiber_ledger_tracks_snapshot_updates_and_terminal_state() {
    let mut ledger: CourierFiberLedger<2> = CourierFiberLedger::new();
    let created = ManagedFiberSnapshot {
        id: FiberId::new(7),
        state: FiberState::Created,
        started: false,
        claim_awareness: ClaimAwareness::Blind,
        claim_context: None,
    };
    ledger
        .register(
            created,
            1,
            CourierFiberClass::Dynamic,
            true,
            Some(FiberMetadataAttachment::new(11)),
            100,
        )
        .unwrap();
    let running = ManagedFiberSnapshot {
        state: FiberState::Running,
        started: true,
        ..created
    };
    ledger.update_from_snapshot(running, 125).unwrap();
    let record = ledger.fiber(FiberId::new(7)).unwrap();
    assert_eq!(record.state, FiberState::Running);
    assert!(record.started);
    assert_eq!(record.last_transition_tick, 125);
    assert_eq!(record.metadata_attachment.unwrap().get(), 11);

    ledger
        .mark_terminal(
            FiberId::new(7),
            FiberTerminalStatus::Completed(FiberReturn::new(99)),
            200,
        )
        .unwrap();
    let terminal = ledger.fiber(FiberId::new(7)).unwrap();
    assert_eq!(terminal.state, FiberState::Completed);
    assert_eq!(
        terminal.terminal,
        Some(FiberTerminalStatus::Completed(FiberReturn::new(99)))
    );
}

#[test]
fn courier_metadata_store_upserts_and_removes_entries() {
    let mut metadata: CourierMetadataStore<'_, 4> = CourierMetadataStore::new();
    metadata
        .upsert(CourierMetadataSubject::Courier, "title", "httpd", 11)
        .unwrap();
    metadata
        .upsert(
            CourierMetadataSubject::Fiber(FiberId::new(7)),
            "phase",
            "boot",
            12,
        )
        .unwrap();
    metadata
        .upsert(CourierMetadataSubject::Courier, "title", "cache-httpd", 13)
        .unwrap();

    assert_eq!(
        metadata
            .entry(CourierMetadataSubject::Courier, "title")
            .unwrap()
            .value,
        "cache-httpd"
    );
    assert_eq!(
        metadata
            .entries(CourierMetadataSubject::Fiber(FiberId::new(7)))
            .next()
            .unwrap()
            .value,
        "boot"
    );
    metadata
        .remove(CourierMetadataSubject::Fiber(FiberId::new(7)), "phase")
        .unwrap();
    assert!(
        metadata
            .entry(CourierMetadataSubject::Fiber(FiberId::new(7)), "phase")
            .is_none()
    );
}

#[test]
fn courier_obligation_registry_tracks_progress_and_aging() {
    let mut obligations: CourierObligationRegistry<'_, 4> = CourierObligationRegistry::new();
    let obligation = obligations
        .register(
            CourierObligationSpec::new(
                CourierMetadataSubject::Fiber(FiberId::new(7)),
                CourierObligationBinding::Input("hw.keyboard@kernel-local[pvas.me]"),
                5,
                10,
            ),
            100,
        )
        .unwrap();
    assert_eq!(
        obligations.evaluate(103).unwrap(),
        CourierResponsiveness::Responsive
    );
    assert_eq!(
        obligations.evaluate(105).unwrap(),
        CourierResponsiveness::Stale
    );
    assert_eq!(
        obligations.evaluate(110).unwrap(),
        CourierResponsiveness::NonResponsive
    );
    obligations.record_progress(obligation, 111).unwrap();
    assert_eq!(
        obligations.evaluate(111).unwrap(),
        CourierResponsiveness::Responsive
    );
    assert_eq!(
        obligations
            .obligation(obligation)
            .unwrap()
            .last_progress_tick,
        111
    );
}

#[test]
fn courier_plan_validation_treats_runnable_capacity_as_aggregate_cap() {
    assert!(
        CourierPlan::new(1, 4)
            .with_async_capacity(4)
            .with_runnable_capacity(4)
            .is_valid()
    );
    assert!(
        !CourierPlan::new(1, 2)
            .with_async_capacity(1)
            .with_runnable_capacity(8)
            .is_valid()
    );
}
