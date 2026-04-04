use crate::claims::{ClaimsDigest, ImageSealId};
use crate::courier::CourierLaunchDescriptor;
use super::*;

fn demo_plan(max_child_couriers: usize, max_live_fibers: usize) -> CourierPlan {
    CourierPlan::new(max_child_couriers, max_live_fibers)
        .with_fiber_metadata_capacity(1)
        .with_child_observation_capacity(1)
        .with_recent_dead_depth(4)
}

const DOMAIN_ID: DomainId = DomainId::new(0x5056_4153);
const PRIMARY_COURIER: CourierId = CourierId::new(1);
const SCOPED_COURIER: CourierId = CourierId::new(2);
const FIBER_CONTEXT: ContextId = ContextId::new(0x100);
const BLOCK_CONTEXT: ContextId = ContextId::new(0x101);

#[test]
fn couriers_enumerate_only_their_visible_contexts() {
    let mut registry: DomainRegistry<'_, 4, 8, 4> = DomainRegistry::new(DomainDescriptor {
        id: DOMAIN_ID,
        name: "pvas",
        kind: DomainKind::NativeSubstrate,
        caps: DomainCaps::COURIER_REGISTRY
            | DomainCaps::CONTEXT_REGISTRY
            | DomainCaps::COURIER_VISIBILITY,
    });

    registry
        .register_courier(CourierDescriptor {
            id: PRIMARY_COURIER,
            name: "primary",
            caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS
                | CourierCaps::PROJECT_CONTEXTS
                | CourierCaps::SPAWN_SUB_FIBERS
                | CourierCaps::DEBUG_CHANNEL,
            visibility: CourierVisibility::Full,
            claim_awareness: ClaimAwareness::Black,
            claim_context: Some(ClaimContextId::new(0xAAA0)),
            plan: demo_plan(2, 4),
        })
        .expect("primary courier should register");
    registry
        .register_context(
            PRIMARY_COURIER,
            ContextDescriptor {
                id: FIBER_CONTEXT,
                name: "primary.main",
                kind: ContextKind::FiberMetadata,
                caps: ContextCaps::PROJECTABLE | ContextCaps::CONTROL_ENDPOINT,
                claim_context: Some(ClaimContextId::new(0xAAA0)),
            },
        )
        .expect("fiber metadata context should register");
    registry
        .register_context(
            PRIMARY_COURIER,
            ContextDescriptor {
                id: BLOCK_CONTEXT,
                name: "nvme0n1p1",
                kind: ContextKind::StorageEndpoint,
                caps: ContextCaps::PROJECTABLE | ContextCaps::CHANNEL_BACKED,
                claim_context: None,
            },
        )
        .expect("block context should register");
    registry
        .register_courier(CourierDescriptor {
            id: SCOPED_COURIER,
            name: "scoped",
            caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
            visibility: CourierVisibility::Scoped,
            claim_awareness: ClaimAwareness::Blind,
            claim_context: None,
            plan: demo_plan(0, 1),
        })
        .expect("scoped courier should register");
    registry
        .grant_context(SCOPED_COURIER, BLOCK_CONTEXT, ContextProjectionKind::Alias)
        .expect("scoped courier should receive one projected block context");

    let primary = registry
        .courier(PRIMARY_COURIER)
        .expect("primary courier should exist");
    assert_eq!(
        primary.courier_support().visibility,
        CourierVisibility::Full
    );
    assert_eq!(
        primary.courier_support().claim_awareness,
        ClaimAwareness::Black
    );
    assert_eq!(
        primary.courier_support().claim_context,
        Some(ClaimContextId::new(0xAAA0))
    );
    assert_eq!(primary.visible_context_count(), 2);

    let scoped = registry
        .courier(SCOPED_COURIER)
        .expect("scoped courier should exist");
    assert_eq!(
        scoped.courier_support().visibility,
        CourierVisibility::Scoped
    );
    assert_eq!(
        scoped.courier_support().claim_awareness,
        ClaimAwareness::Blind
    );
    assert_eq!(scoped.visible_context_count(), 1);
    assert!(!scoped.can_observe_context(FIBER_CONTEXT));
    assert!(scoped.can_observe_context(BLOCK_CONTEXT));

    let visible: [Option<&str>; 2] = {
        let mut names = [None; 2];
        for (index, context) in scoped.visible_contexts().enumerate() {
            names[index] = Some(context.record().descriptor.name);
            assert_eq!(
                context.context_support().projection,
                ContextProjectionKind::Alias
            );
        }
        names
    };
    assert_eq!(visible, [Some("nvme0n1p1"), None]);
}

#[test]
fn duplicate_courier_ids_are_rejected() {
    let mut registry: DomainRegistry<'_, 4, 4, 4> = DomainRegistry::new(DomainDescriptor {
        id: DOMAIN_ID,
        name: "pvas",
        kind: DomainKind::NativeSubstrate,
        caps: DomainCaps::COURIER_REGISTRY | DomainCaps::COURIER_VISIBILITY,
    });
    registry
        .register_courier(CourierDescriptor {
            id: PRIMARY_COURIER,
            name: "primary",
            caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
            visibility: CourierVisibility::Full,
            claim_awareness: ClaimAwareness::Blind,
            claim_context: None,
            plan: demo_plan(1, 2),
        })
        .expect("primary courier should register");

    let result = registry.register_courier(CourierDescriptor {
        id: PRIMARY_COURIER,
        name: "duplicate",
        caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
        visibility: CourierVisibility::Scoped,
        claim_awareness: ClaimAwareness::Black,
        claim_context: Some(ClaimContextId::new(1)),
        plan: demo_plan(0, 1),
    });
    assert!(matches!(
        result,
        Err(error) if error.kind() == DomainErrorKind::StateConflict
    ));
}

#[test]
fn child_couriers_and_fibers_are_visible_through_courier_handles() {
    let mut registry: DomainRegistry<'_, 4, 4, 4, 2, 4> = DomainRegistry::new(DomainDescriptor {
        id: DOMAIN_ID,
        name: "pvas",
        kind: DomainKind::NativeSubstrate,
        caps: DomainCaps::COURIER_REGISTRY | DomainCaps::COURIER_VISIBILITY,
    });
    let root_seal = LocalAdmissionSeal::new(
        ImageSealId::new(1),
        ClaimsDigest::zero(),
        ClaimsDigest::zero(),
        ClaimsDigest::zero(),
        47,
    );
    registry
        .register_courier(CourierDescriptor {
            id: PRIMARY_COURIER,
            name: "root",
            caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS | CourierCaps::SPAWN_SUB_FIBERS,
            visibility: CourierVisibility::Full,
            claim_awareness: ClaimAwareness::Black,
            claim_context: Some(ClaimContextId::new(0xAAA0)),
            plan: demo_plan(2, 4),
        })
        .expect("root courier should register");
    registry
        .register_child_courier(
            PRIMARY_COURIER,
            CourierDescriptor {
                id: SCOPED_COURIER,
                name: "httpd",
                caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS | CourierCaps::SPAWN_SUB_FIBERS,
                visibility: CourierVisibility::Scoped,
                claim_awareness: ClaimAwareness::Black,
                claim_context: Some(ClaimContextId::new(0xBBB0)),
                plan: demo_plan(0, 2),
            },
            PrincipalId::parse("httpd#01@web[cache.pvas-local]:443").unwrap(),
            root_seal,
            47,
            10,
            FiberId::new(9),
        )
        .expect("child courier should register");
    registry
        .register_fiber(
            SCOPED_COURIER,
            ManagedFiberSnapshot {
                id: FiberId::new(9),
                state: crate::fiber::FiberState::Created,
                started: false,
                claim_awareness: ClaimAwareness::Black,
                claim_context: Some(ClaimContextId::new(0xBBB0)),
            },
            1,
            true,
            Some(FiberMetadataAttachment::new(11)),
            10,
        )
        .expect("root fiber should register under child courier");

    let parent = registry.courier(PRIMARY_COURIER).unwrap();
    assert_eq!(parent.plan(), demo_plan(2, 4));
    assert_eq!(parent.child_courier_count(), 1);
    let child = parent.child_couriers().next().unwrap();
    assert_eq!(child.child, SCOPED_COURIER);
    assert_eq!(child.root_fiber, FiberId::new(9));

    let launched = registry.courier(SCOPED_COURIER).unwrap();
    assert_eq!(launched.parent_courier(), Some(PRIMARY_COURIER));
    assert_eq!(launched.fiber_count(), 1);
    let root = launched.fiber(FiberId::new(9)).unwrap();
    assert!(root.is_root);
    assert_eq!(
        root.metadata_attachment,
        Some(FiberMetadataAttachment::new(11))
    );
}

#[test]
fn launch_control_registers_child_courier_launch_truth() {
    let mut registry: DomainRegistry<'_, 4, 4, 4, 2, 4> = DomainRegistry::new(DomainDescriptor {
        id: DOMAIN_ID,
        name: "pvas",
        kind: DomainKind::NativeSubstrate,
        caps: DomainCaps::COURIER_REGISTRY | DomainCaps::COURIER_VISIBILITY,
    });
    registry
        .register_courier(CourierDescriptor {
            id: PRIMARY_COURIER,
            name: "root",
            caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS | CourierCaps::SPAWN_SUB_FIBERS,
            visibility: CourierVisibility::Full,
            claim_awareness: ClaimAwareness::Black,
            claim_context: Some(ClaimContextId::new(0xAAA0)),
            plan: demo_plan(2, 4),
        })
        .expect("root courier should register");

    let request = CourierChildLaunchRequest {
        parent: PRIMARY_COURIER,
        descriptor: CourierLaunchDescriptor {
            id: SCOPED_COURIER,
            name: "httpd",
            caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS | CourierCaps::SPAWN_SUB_FIBERS,
            visibility: CourierVisibility::Scoped,
            claim_awareness: ClaimAwareness::Black,
            claim_context: Some(ClaimContextId::new(0xBBB0)),
            plan: demo_plan(0, 2),
        },
        principal: PrincipalId::parse("httpd#01@web[cache.pvas-local]:443").unwrap(),
        image_seal: LocalAdmissionSeal::new(
            ImageSealId::new(7),
            ClaimsDigest::zero(),
            ClaimsDigest::zero(),
            ClaimsDigest::zero(),
            47,
        ),
        launch_epoch: 47,
    };

    registry
        .launch_control()
        .register_child_courier(request, 99, FiberId::new(42))
        .expect("launch control should register child courier");

    let parent = registry.courier(PRIMARY_COURIER).unwrap();
    let child = parent.child_couriers().next().unwrap();
    assert_eq!(child.child, SCOPED_COURIER);
    assert_eq!(child.root_fiber, FiberId::new(42));
    assert_eq!(child.launched_at_tick, 99);
}

#[test]
fn child_progress_updates_parent_and_child_launch_state() {
    let mut registry: DomainRegistry<'_, 4, 4, 4, 2, 2> = DomainRegistry::new(DomainDescriptor {
        id: DOMAIN_ID,
        name: "pvas",
        kind: DomainKind::NativeSubstrate,
        caps: DomainCaps::COURIER_REGISTRY | DomainCaps::COURIER_VISIBILITY,
    });
    let seal = LocalAdmissionSeal::new(
        ImageSealId::new(2),
        ClaimsDigest::zero(),
        ClaimsDigest::zero(),
        ClaimsDigest::zero(),
        51,
    );
    registry
        .register_courier(CourierDescriptor {
            id: PRIMARY_COURIER,
            name: "root",
            caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
            visibility: CourierVisibility::Full,
            claim_awareness: ClaimAwareness::Black,
            claim_context: Some(ClaimContextId::new(1)),
            plan: demo_plan(1, 1),
        })
        .unwrap();
    registry
        .register_child_courier(
            PRIMARY_COURIER,
            CourierDescriptor {
                id: SCOPED_COURIER,
                name: "child",
                caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
                visibility: CourierVisibility::Scoped,
                claim_awareness: ClaimAwareness::Blind,
                claim_context: None,
                plan: demo_plan(0, 1),
            },
            PrincipalId::parse("child@svc[kernel-local]").unwrap(),
            seal,
            51,
            20,
            FiberId::new(1),
        )
        .unwrap();
    registry
        .mark_child_stale(PRIMARY_COURIER, SCOPED_COURIER)
        .unwrap();
    assert_eq!(
        registry
            .courier(SCOPED_COURIER)
            .unwrap()
            .launch_record()
            .unwrap()
            .responsiveness,
        CourierResponsiveness::Stale
    );
    registry
        .record_child_progress(PRIMARY_COURIER, SCOPED_COURIER, 44)
        .unwrap();
    let child = registry.courier(SCOPED_COURIER).unwrap();
    assert_eq!(child.launch_record().unwrap().last_progress_tick, 44);
    assert_eq!(
        child.launch_record().unwrap().responsiveness,
        CourierResponsiveness::Responsive
    );
    let parent = registry.courier(PRIMARY_COURIER).unwrap();
    assert_eq!(
        parent.child_couriers().next().unwrap().last_progress_tick,
        44
    );
}

#[test]
fn courier_owned_metadata_updates_drive_authoritative_progress() {
    let mut registry: DomainRegistry<'_, 4, 4, 4, 2, 2, 8> =
        DomainRegistry::new(DomainDescriptor {
            id: DOMAIN_ID,
            name: "pvas",
            kind: DomainKind::NativeSubstrate,
            caps: DomainCaps::COURIER_REGISTRY | DomainCaps::COURIER_VISIBILITY,
        });
    let seal = LocalAdmissionSeal::new(
        ImageSealId::new(3),
        ClaimsDigest::zero(),
        ClaimsDigest::zero(),
        ClaimsDigest::zero(),
        61,
    );
    registry
        .register_courier(CourierDescriptor {
            id: PRIMARY_COURIER,
            name: "root",
            caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
            visibility: CourierVisibility::Full,
            claim_awareness: ClaimAwareness::Black,
            claim_context: Some(ClaimContextId::new(1)),
            plan: CourierPlan::new(1, 2)
                .with_app_metadata_capacity(8)
                .with_obligation_capacity(1),
        })
        .unwrap();
    registry
        .register_child_courier(
            PRIMARY_COURIER,
            CourierDescriptor {
                id: SCOPED_COURIER,
                name: "worker",
                caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
                visibility: CourierVisibility::Scoped,
                claim_awareness: ClaimAwareness::Blind,
                claim_context: None,
                plan: CourierPlan::new(0, 2)
                    .with_app_metadata_capacity(8)
                    .with_obligation_capacity(1),
            },
            PrincipalId::parse("worker@svc[kernel-local]").unwrap(),
            seal,
            61,
            20,
            FiberId::new(9),
        )
        .unwrap();
    registry
        .register_fiber(
            SCOPED_COURIER,
            ManagedFiberSnapshot {
                id: FiberId::new(9),
                state: crate::fiber::FiberState::Created,
                started: false,
                claim_awareness: ClaimAwareness::Blind,
                claim_context: None,
            },
            1,
            true,
            None,
            20,
        )
        .unwrap();
    registry
        .mark_child_stale(PRIMARY_COURIER, SCOPED_COURIER)
        .unwrap();

    registry
        .upsert_courier_metadata(SCOPED_COURIER, "title", "worker", 55)
        .unwrap();
    registry
        .upsert_fiber_metadata(SCOPED_COURIER, FiberId::new(9), "phase", "boot", 56)
        .unwrap();
    registry
        .upsert_child_courier_metadata(PRIMARY_COURIER, SCOPED_COURIER, "status", "warm", 57)
        .unwrap();
    registry
        .upsert_async_metadata(SCOPED_COURIER, "executor", "ready", 58)
        .unwrap();

    let child = registry.courier(SCOPED_COURIER).unwrap();
    assert_eq!(child.launch_record().unwrap().last_progress_tick, 58);
    assert_eq!(
        child.launch_record().unwrap().responsiveness,
        CourierResponsiveness::Responsive
    );
    assert_eq!(
        child
            .courier_metadata_entry("title")
            .expect("courier metadata should exist")
            .value,
        "worker"
    );
    assert_eq!(
        child
            .fiber_metadata_entry(FiberId::new(9), "phase")
            .expect("fiber metadata should exist")
            .value,
        "boot"
    );
    let parent = registry.courier(PRIMARY_COURIER).unwrap();
    assert_eq!(
        parent.child_couriers().next().unwrap().last_progress_tick,
        58
    );
    assert_eq!(
        parent
            .child_courier_metadata_entry(SCOPED_COURIER, "status")
            .expect("parent-owned child metadata should exist")
            .value,
        "warm"
    );
    assert_eq!(
        child
            .async_metadata_entry("executor")
            .expect("async metadata should exist")
            .value,
        "ready"
    );
}

#[test]
fn courier_obligations_drive_child_responsiveness() {
    let mut registry: DomainRegistry<'_, 4, 4, 4, 2, 2, 8> =
        DomainRegistry::new(DomainDescriptor {
            id: DOMAIN_ID,
            name: "pvas",
            kind: DomainKind::NativeSubstrate,
            caps: DomainCaps::COURIER_REGISTRY | DomainCaps::COURIER_VISIBILITY,
        });
    let seal = LocalAdmissionSeal::new(
        ImageSealId::new(4),
        ClaimsDigest::zero(),
        ClaimsDigest::zero(),
        ClaimsDigest::zero(),
        67,
    );
    registry
        .register_courier(CourierDescriptor {
            id: PRIMARY_COURIER,
            name: "root",
            caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
            visibility: CourierVisibility::Full,
            claim_awareness: ClaimAwareness::Black,
            claim_context: Some(ClaimContextId::new(1)),
            plan: CourierPlan::new(1, 2)
                .with_app_metadata_capacity(4)
                .with_obligation_capacity(4),
        })
        .unwrap();
    registry
        .register_child_courier(
            PRIMARY_COURIER,
            CourierDescriptor {
                id: SCOPED_COURIER,
                name: "text-editor",
                caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
                visibility: CourierVisibility::Scoped,
                claim_awareness: ClaimAwareness::Blind,
                claim_context: None,
                plan: CourierPlan::new(0, 1)
                    .with_app_metadata_capacity(4)
                    .with_obligation_capacity(4),
            },
            PrincipalId::parse("editor@user[pvas-local]").unwrap(),
            seal,
            67,
            10,
            FiberId::new(1),
        )
        .unwrap();
    let obligation = registry
        .register_input_obligation(
            SCOPED_COURIER,
            CourierMetadataSubject::Courier,
            "hw.keyboard@kernel-local[pvas.me]",
            5,
            10,
            10,
        )
        .unwrap();
    assert_eq!(
        registry
            .evaluate_courier_responsiveness(SCOPED_COURIER, 15)
            .unwrap(),
        CourierResponsiveness::Stale
    );
    assert_eq!(
        registry
            .courier(SCOPED_COURIER)
            .unwrap()
            .launch_record()
            .unwrap()
            .responsiveness,
        CourierResponsiveness::Stale
    );
    assert_eq!(
        registry
            .courier(PRIMARY_COURIER)
            .unwrap()
            .child_couriers()
            .next()
            .unwrap()
            .responsiveness,
        CourierResponsiveness::Stale
    );
    registry
        .record_obligation_progress(SCOPED_COURIER, obligation, 16)
        .unwrap();
    assert_eq!(
        registry
            .courier(SCOPED_COURIER)
            .unwrap()
            .launch_record()
            .unwrap()
            .responsiveness,
        CourierResponsiveness::Responsive
    );
    assert_eq!(
        registry
            .evaluate_courier_responsiveness(SCOPED_COURIER, 26)
            .unwrap(),
        CourierResponsiveness::NonResponsive
    );
    let child = registry.courier(SCOPED_COURIER).unwrap();
    assert_eq!(child.obligation_count(), 1);
    let obligation = child.obligation(obligation).unwrap();
    assert_eq!(
        obligation.kind,
        crate::courier::CourierObligationKind::Input
    );
    assert_eq!(
        obligation.binding,
        CourierObligationBinding::Input("hw.keyboard@kernel-local[pvas.me]")
    );
    assert_eq!(obligation.label, "hw.keyboard@kernel-local[pvas.me]");
}

#[test]
fn courier_plan_bounds_child_and_fiber_registration() {
    let mut registry: DomainRegistry<'_, 4, 4, 4, 1, 1> = DomainRegistry::new(DomainDescriptor {
        id: DOMAIN_ID,
        name: "pvas",
        kind: DomainKind::NativeSubstrate,
        caps: DomainCaps::COURIER_REGISTRY | DomainCaps::COURIER_VISIBILITY,
    });
    let seal = LocalAdmissionSeal::new(
        ImageSealId::new(3),
        ClaimsDigest::zero(),
        ClaimsDigest::zero(),
        ClaimsDigest::zero(),
        52,
    );
    registry
        .register_courier(CourierDescriptor {
            id: PRIMARY_COURIER,
            name: "root",
            caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
            visibility: CourierVisibility::Full,
            claim_awareness: ClaimAwareness::Blind,
            claim_context: None,
            plan: demo_plan(1, 1),
        })
        .unwrap();
    registry
        .register_child_courier(
            PRIMARY_COURIER,
            CourierDescriptor {
                id: SCOPED_COURIER,
                name: "first",
                caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
                visibility: CourierVisibility::Scoped,
                claim_awareness: ClaimAwareness::Blind,
                claim_context: None,
                plan: demo_plan(0, 1),
            },
            PrincipalId::parse("first@svc[kernel-local]").unwrap(),
            seal,
            52,
            1,
            FiberId::new(1),
        )
        .unwrap();
    let second_child = registry.register_child_courier(
        PRIMARY_COURIER,
        CourierDescriptor {
            id: CourierId::new(3),
            name: "second",
            caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
            visibility: CourierVisibility::Scoped,
            claim_awareness: ClaimAwareness::Blind,
            claim_context: None,
            plan: demo_plan(0, 1),
        },
        PrincipalId::parse("second@svc[kernel-local]").unwrap(),
        seal,
        52,
        2,
        FiberId::new(2),
    );
    assert!(matches!(
        second_child,
        Err(error) if error.kind() == DomainErrorKind::ResourceExhausted
    ));

    registry
        .register_fiber(
            SCOPED_COURIER,
            ManagedFiberSnapshot {
                id: FiberId::new(1),
                state: crate::fiber::FiberState::Created,
                started: false,
                claim_awareness: ClaimAwareness::Blind,
                claim_context: None,
            },
            1,
            true,
            None,
            1,
        )
        .unwrap();
    let second_fiber = registry.register_fiber(
        SCOPED_COURIER,
        ManagedFiberSnapshot {
            id: FiberId::new(2),
            state: crate::fiber::FiberState::Created,
            started: false,
            claim_awareness: ClaimAwareness::Blind,
            claim_context: None,
        },
        1,
        false,
        None,
        2,
    );
    assert!(matches!(
        second_fiber,
        Err(error) if error.kind() == DomainErrorKind::ResourceExhausted
    ));
}
