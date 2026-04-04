use crate::courier::{
    CourierCaps,
    CourierImplementationKind,
    CourierVisibility,
};
use crate::domain::DomainId;
use super::*;

type DemoRegistry<'a> = ClaimContextRegistry<'a, 4, 4, 4, 16>;
type DemoAuthority<'a> = CourierAuthorityRegistry<'a, 4, 4, 4, 4, 16>;
type TinyRegistry<'a> = ClaimContextRegistry<'a, 1, 1, 1, 4>;

fn local_seal(id: u64) -> LocalAdmissionSeal {
    LocalAdmissionSeal::new(
        ImageSealId::new(id),
        ClaimsDigest::zero(),
        ClaimsDigest::zero(),
        ClaimsDigest::zero(),
        47,
    )
}

fn black_courier(context: ClaimContextId) -> CourierSupport {
    CourierSupport {
        caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
        implementation: CourierImplementationKind::Native,
        domain: DomainId::new(7),
        visibility: CourierVisibility::Scoped,
        claim_awareness: ClaimAwareness::Black,
        claim_context: Some(context),
    }
}

fn black_courier_descriptor<'a>(
    courier: CourierId,
    principal: PrincipalId<'a>,
    parent: Option<CourierId>,
) -> CourierAuthorityDescriptor<'a> {
    CourierAuthorityDescriptor {
        courier,
        principal,
        parent,
        awareness: ClaimAwareness::Black,
    }
}

#[test]
fn registry_can_search_active_claims_and_snapshot_contexts() {
    let mut registry: DemoRegistry<'_> = ClaimContextRegistry::new();
    registry
        .register_context(ClaimContextDescriptor {
            id: ClaimContextId::new(1),
            principal: PrincipalId::parse("httpd@kernel-local[cache]").unwrap(),
            image_seal: local_seal(10),
            awareness: ClaimAwareness::Black,
        })
        .expect("context should register");
    registry
        .grant_claim(
            ClaimContextId::new(1),
            ClaimGrant {
                qualified: QualifiedClaimId::parse("httpd@kernel-local[cache]=>net.tcp.9094")
                    .unwrap(),
                group: Some(ClaimGroupName::parse("net.listen").unwrap()),
                source: ClaimGrantSource::LocalPolicy,
                lifetime: ClaimGrantLifetime::Retained,
                state: ClaimGrantState::Granted,
                issued_at_unix_seconds: 100,
                expires_at_unix_seconds: Some(200),
                seal: local_seal(10),
            },
        )
        .expect("claim should grant");

    let snapshot = registry
        .snapshot_by_principal(PrincipalId::parse("httpd@kernel-local[cache]").unwrap())
        .expect("snapshot should exist");
    assert_eq!(snapshot.descriptor.image_seal.granted_claim_count, 1);
    assert_eq!(
        snapshot.claims[0]
            .expect("claim should exist")
            .qualified
            .as_str(),
        "httpd@kernel-local[cache]=>net.tcp.9094"
    );

    let search =
        registry.search_active_claims::<4>(QualifiedClaimPattern::parse("*=>net.*").unwrap(), 150);
    assert_eq!(search.total_matches, 1);
    assert_eq!(
        search.matches[0]
            .expect("search hit should exist")
            .qualified
            .as_str(),
        "httpd@kernel-local[cache]=>net.tcp.9094"
    );
}

#[test]
fn courier_and_fiber_requests_require_black_switch_and_matching_context() {
    let mut registry: DemoRegistry<'_> = ClaimContextRegistry::new();
    registry
        .register_context(ClaimContextDescriptor {
            id: ClaimContextId::new(1),
            principal: PrincipalId::parse("httpd@kernel-local[cache]").unwrap(),
            image_seal: local_seal(10),
            awareness: ClaimAwareness::Black,
        })
        .expect("context should register");
    let qualified = QualifiedClaimId::parse("httpd@kernel-local[cache]=>net.tcp.9094").unwrap();
    registry
        .grant_claim(
            ClaimContextId::new(1),
            ClaimGrant {
                qualified,
                group: None,
                source: ClaimGrantSource::LocalPolicy,
                lifetime: ClaimGrantLifetime::Retained,
                state: ClaimGrantState::Granted,
                issued_at_unix_seconds: 100,
                expires_at_unix_seconds: None,
                seal: local_seal(10),
            },
        )
        .expect("claim should grant");

    let courier = black_courier(ClaimContextId::new(1));
    assert!(
        registry
            .request_claim_for_courier(courier, qualified, 150)
            .is_ok()
    );
    assert!(
        registry
            .request_claim_for_fiber(
                courier,
                ClaimAwareness::Black,
                Some(ClaimContextId::new(1)),
                qualified,
                150,
            )
            .is_ok()
    );
    assert!(matches!(
        registry.request_claim_for_fiber(
            courier,
            ClaimAwareness::Blind,
            Some(ClaimContextId::new(1)),
            qualified,
            150,
        ),
        Err(error) if error.kind() == ClaimsErrorKind::PermissionDenied
    ));
}

#[test]
fn one_shot_claims_are_consumed_after_one_successful_request() {
    let mut registry: DemoRegistry<'_> = ClaimContextRegistry::new();
    registry
        .register_context(ClaimContextDescriptor {
            id: ClaimContextId::new(1),
            principal: PrincipalId::parse("httpd@kernel-local[cache]").unwrap(),
            image_seal: local_seal(10),
            awareness: ClaimAwareness::Black,
        })
        .expect("context should register");
    let qualified = QualifiedClaimId::parse("httpd@kernel-local[cache]=>net.tcp.9094").unwrap();
    registry
        .grant_claim(
            ClaimContextId::new(1),
            ClaimGrant {
                qualified,
                group: None,
                source: ClaimGrantSource::LocalPolicy,
                lifetime: ClaimGrantLifetime::OneShot,
                state: ClaimGrantState::Granted,
                issued_at_unix_seconds: 100,
                expires_at_unix_seconds: None,
                seal: local_seal(10),
            },
        )
        .expect("claim should grant");

    let courier = black_courier(ClaimContextId::new(1));
    assert!(
        registry
            .request_claim_for_courier(courier, qualified, 150)
            .is_ok()
    );
    assert!(matches!(
        registry.request_claim_for_courier(courier, qualified, 151),
        Err(error) if error.kind() == ClaimsErrorKind::PermissionDenied
    ));
    assert!(matches!(
        registry.snapshot(ClaimContextId::new(1)).unwrap().claims[0],
        Some(grant) if grant.state == ClaimGrantState::Consumed
    ));
    assert_eq!(
        registry
            .snapshot(ClaimContextId::new(1))
            .unwrap()
            .descriptor
            .image_seal
            .granted_claim_count,
        0
    );
}

#[test]
fn revoked_claims_can_be_regranted_in_place() {
    let mut registry: DemoRegistry<'_> = ClaimContextRegistry::new();
    registry
        .register_context(ClaimContextDescriptor {
            id: ClaimContextId::new(1),
            principal: PrincipalId::parse("httpd@kernel-local[cache]").unwrap(),
            image_seal: local_seal(10),
            awareness: ClaimAwareness::Black,
        })
        .expect("context should register");
    let qualified = QualifiedClaimId::parse("httpd@kernel-local[cache]=>net.tcp.9094").unwrap();
    registry
        .grant_claim(
            ClaimContextId::new(1),
            ClaimGrant {
                qualified,
                group: None,
                source: ClaimGrantSource::LocalPolicy,
                lifetime: ClaimGrantLifetime::Retained,
                state: ClaimGrantState::Granted,
                issued_at_unix_seconds: 100,
                expires_at_unix_seconds: None,
                seal: local_seal(10),
            },
        )
        .expect("initial grant should succeed");
    registry
        .revoke_claim(ClaimContextId::new(1), qualified)
        .expect("revocation should succeed");
    assert!(matches!(
        registry.request_claim_for_courier(black_courier(ClaimContextId::new(1)), qualified, 125),
        Err(error) if error.kind() == ClaimsErrorKind::Revoked
    ));
    assert_eq!(
        registry
            .snapshot(ClaimContextId::new(1))
            .unwrap()
            .descriptor
            .image_seal
            .granted_claim_count,
        0
    );
    registry
        .grant_claim(
            ClaimContextId::new(1),
            ClaimGrant {
                qualified,
                group: None,
                source: ClaimGrantSource::LocalPolicy,
                lifetime: ClaimGrantLifetime::Retained,
                state: ClaimGrantState::Granted,
                issued_at_unix_seconds: 150,
                expires_at_unix_seconds: Some(250),
                seal: local_seal(10),
            },
        )
        .expect("regrant should succeed");

    let snapshot = registry.snapshot(ClaimContextId::new(1)).unwrap();
    assert!(matches!(
        snapshot.claims[0],
        Some(grant)
            if grant.state == ClaimGrantState::Granted
                && grant.issued_at_unix_seconds == 150
                && grant.expires_at_unix_seconds == Some(250)
    ));
}

#[test]
fn terminal_claim_slots_and_trie_nodes_are_reused_for_new_scopes() {
    let mut registry: TinyRegistry<'_> = ClaimContextRegistry::new();
    registry
        .register_context(ClaimContextDescriptor {
            id: ClaimContextId::new(1),
            principal: PrincipalId::parse("httpd@kernel-local[cache]").unwrap(),
            image_seal: local_seal(10),
            awareness: ClaimAwareness::Black,
        })
        .expect("context should register");
    let first = QualifiedClaimId::parse("httpd@kernel-local[cache]=>net.tcp.443").unwrap();
    let second = QualifiedClaimId::parse("httpd@kernel-local[cache]=>hw.nic.rx").unwrap();
    registry
        .grant_claim(
            ClaimContextId::new(1),
            ClaimGrant {
                qualified: first,
                group: None,
                source: ClaimGrantSource::LocalPolicy,
                lifetime: ClaimGrantLifetime::Retained,
                state: ClaimGrantState::Granted,
                issued_at_unix_seconds: 100,
                expires_at_unix_seconds: None,
                seal: local_seal(10),
            },
        )
        .expect("first claim should grant");
    registry
        .revoke_claim(ClaimContextId::new(1), first)
        .expect("first claim should revoke");
    registry
        .grant_claim(
            ClaimContextId::new(1),
            ClaimGrant {
                qualified: second,
                group: None,
                source: ClaimGrantSource::LocalPolicy,
                lifetime: ClaimGrantLifetime::Retained,
                state: ClaimGrantState::Granted,
                issued_at_unix_seconds: 110,
                expires_at_unix_seconds: None,
                seal: local_seal(10),
            },
        )
        .expect("second claim should reuse terminal slot and trie nodes");
    assert_eq!(
        registry.snapshot(ClaimContextId::new(1)).unwrap().claims[0]
            .unwrap()
            .qualified,
        second
    );
}

#[test]
fn expired_claims_drop_out_of_active_count() {
    let mut registry: DemoRegistry<'_> = ClaimContextRegistry::new();
    registry
        .register_context(ClaimContextDescriptor {
            id: ClaimContextId::new(1),
            principal: PrincipalId::parse("httpd@kernel-local[cache]").unwrap(),
            image_seal: local_seal(10),
            awareness: ClaimAwareness::Black,
        })
        .expect("context should register");
    registry
        .grant_claim(
            ClaimContextId::new(1),
            ClaimGrant {
                qualified: QualifiedClaimId::parse("httpd@kernel-local[cache]=>net.tcp.9094")
                    .unwrap(),
                group: None,
                source: ClaimGrantSource::LocalPolicy,
                lifetime: ClaimGrantLifetime::ExpiresAt { unix_seconds: 125 },
                state: ClaimGrantState::Granted,
                issued_at_unix_seconds: 100,
                expires_at_unix_seconds: Some(125),
                seal: local_seal(10),
            },
        )
        .expect("claim should grant");
    registry.expire_stale_claims(125);
    let snapshot = registry.snapshot(ClaimContextId::new(1)).unwrap();
    assert!(matches!(
        snapshot.claims[0],
        Some(grant) if grant.state == ClaimGrantState::Expired
    ));
    assert_eq!(snapshot.descriptor.image_seal.granted_claim_count, 0);
}

#[test]
fn bilateral_attachment_bonds_are_attached_to_both_contexts() {
    let mut registry: DemoRegistry<'_> = ClaimContextRegistry::new();
    registry
        .register_context(ClaimContextDescriptor {
            id: ClaimContextId::new(1),
            principal: PrincipalId::parse("firewall@net[kernel]").unwrap(),
            image_seal: local_seal(20),
            awareness: ClaimAwareness::Black,
        })
        .expect("provider should register");
    registry
        .register_context(ClaimContextDescriptor {
            id: ClaimContextId::new(2),
            principal: PrincipalId::parse("httpd@cache[server]").unwrap(),
            image_seal: local_seal(21),
            awareness: ClaimAwareness::Black,
        })
        .expect("consumer should register");

    let bond = registry
        .issue_attachment_bond(
            AttachmentBondId::new(77),
            ClaimContextId::new(1),
            ClaimContextId::new(2),
            ClaimName::parse("net.tcp.443").unwrap(),
            TransportAttachmentLaw::ExclusiveSpsc,
            100,
            Some(200),
            1,
        )
        .expect("bond should issue");
    assert_eq!(bond.provider.principal.as_str(), "firewall@net[kernel]");
    assert_eq!(bond.consumer.principal.as_str(), "httpd@cache[server]");
    assert_eq!(
        registry.snapshot(ClaimContextId::new(1)).unwrap().bonds[0]
            .unwrap()
            .id,
        AttachmentBondId::new(77)
    );
    assert_eq!(
        registry.snapshot(ClaimContextId::new(2)).unwrap().bonds[0]
            .unwrap()
            .id,
        AttachmentBondId::new(77)
    );
}

#[test]
fn courier_authority_tracks_revocation_epoch_and_attached_bonds() {
    let mut authority: DemoAuthority<'_> = CourierAuthorityRegistry::new(47);
    authority
        .register_root_courier(
            black_courier_descriptor(
                CourierId::new(1),
                PrincipalId::parse("firewall@net[kernel]").unwrap(),
                None,
            ),
            ClaimsDigest::zero(),
            ClaimsDigest::zero(),
            ClaimsDigest::zero(),
        )
        .expect("root courier should register");
    authority
        .register_child_courier(
            CourierId::new(1),
            black_courier_descriptor(
                CourierId::new(2),
                PrincipalId::parse("httpd@cache[server]").unwrap(),
                Some(CourierId::new(1)),
            ),
            ClaimsDigest::zero(),
            ClaimsDigest::zero(),
            ClaimsDigest::zero(),
        )
        .expect("child courier should register");

    let bond = authority
        .issue_attachment_bond(
            CourierId::new(1),
            CourierId::new(2),
            ClaimName::parse("net.tcp.443").unwrap(),
            TransportAttachmentLaw::ExclusiveSpsc,
            100,
            Some(200),
        )
        .expect("bond should issue");
    assert!(bond.is_active(150, authority.current_revocation_epoch()));

    let _ = authority.bump_revocation_epoch();
    assert!(!bond.is_active(150, authority.current_revocation_epoch()));
}

#[test]
fn child_courier_claims_must_be_covered_by_parent_scope() {
    let mut authority: DemoAuthority<'_> = CourierAuthorityRegistry::new(47);
    authority
        .register_root_courier(
            black_courier_descriptor(
                CourierId::new(1),
                PrincipalId::parse("firewall@net[kernel]").unwrap(),
                None,
            ),
            ClaimsDigest::zero(),
            ClaimsDigest::zero(),
            ClaimsDigest::zero(),
        )
        .expect("root courier should register");
    authority
        .register_child_courier(
            CourierId::new(1),
            black_courier_descriptor(
                CourierId::new(2),
                PrincipalId::parse("httpd@cache[server]").unwrap(),
                Some(CourierId::new(1)),
            ),
            ClaimsDigest::zero(),
            ClaimsDigest::zero(),
            ClaimsDigest::zero(),
        )
        .expect("child courier should register");

    let child_grant = ClaimGrant {
        qualified: QualifiedClaimId::parse("httpd@cache[server]=>net.tcp.443").unwrap(),
        group: None,
        source: ClaimGrantSource::LocalPolicy,
        lifetime: ClaimGrantLifetime::Retained,
        state: ClaimGrantState::Granted,
        issued_at_unix_seconds: 100,
        expires_at_unix_seconds: None,
        seal: local_seal(20),
    };
    assert!(matches!(
        authority.grant_claim(CourierId::new(2), child_grant),
        Err(error) if error.kind() == ClaimsErrorKind::PermissionDenied
    ));

    authority
        .grant_claim(
            CourierId::new(1),
            ClaimGrant {
                qualified: QualifiedClaimId::parse("firewall@net[kernel]=>net.tcp.443").unwrap(),
                group: None,
                source: ClaimGrantSource::LocalPolicy,
                lifetime: ClaimGrantLifetime::Retained,
                state: ClaimGrantState::Granted,
                issued_at_unix_seconds: 100,
                expires_at_unix_seconds: None,
                seal: local_seal(10),
            },
        )
        .expect("parent grant should succeed");
    authority
        .grant_claim(CourierId::new(2), child_grant)
        .expect("child grant should succeed once parent carries the same scope");
}
