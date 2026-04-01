//! Kernel-side root-courier authority over claims, seals, and attachment bonds.
//!
//! This crate gets to use kernel wording because it is the kernel consumer. The generic Fusion
//! substrate beneath it still stays courier-rooted and kernel-agnostic.

use fusion_sys::claims::{
    AttachmentBond,
    ClaimAwareness,
    ClaimContextId,
    ClaimContextSnapshot,
    ClaimGrant,
    ClaimSearchResults,
    ClaimsDigest,
    ClaimsError,
    CourierAuthorityDescriptor,
    CourierAuthorityRegistry,
    PrincipalId,
    QualifiedClaimId,
    QualifiedClaimPattern,
    SealMismatchReason,
};
use fusion_sys::domain::CourierId;
use fusion_sys::transport::TransportAttachmentLaw;

/// Kernel consumer over the generic courier-rooted claims authority model.
///
/// The kernel boots one root courier with local authority and then composes child couriers beneath
/// it; the underlying claims model is still the same courier-boundary model hosted/library builds
/// use elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KernelCourierAuthority<
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_CLAIMS: usize,
    const MAX_BONDS: usize,
    const MAX_SCOPE_NODES: usize,
> {
    authority: CourierAuthorityRegistry<
        'a,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_CLAIMS,
        MAX_BONDS,
        MAX_SCOPE_NODES,
    >,
}

impl<
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_CLAIMS: usize,
    const MAX_BONDS: usize,
    const MAX_SCOPE_NODES: usize,
> KernelCourierAuthority<'a, MAX_COURIERS, MAX_CONTEXTS, MAX_CLAIMS, MAX_BONDS, MAX_SCOPE_NODES>
{
    /// Creates one kernel-side courier authority tree for one boot epoch.
    #[must_use]
    pub const fn new(boot_epoch: u64) -> Self {
        Self {
            authority: CourierAuthorityRegistry::new(boot_epoch),
        }
    }

    /// Admits one root courier with zero granted claims.
    ///
    /// # Errors
    ///
    /// Returns an honest error when storage is exhausted or the courier already exists.
    pub fn admit_root_courier(
        &mut self,
        courier: CourierId,
        principal: PrincipalId<'a>,
        awareness: ClaimAwareness,
        image_digest: ClaimsDigest,
        claims_digest: ClaimsDigest,
        remote_claims_digest: ClaimsDigest,
    ) -> Result<ClaimContextId, ClaimsError> {
        self.authority.register_root_courier(
            CourierAuthorityDescriptor {
                courier,
                principal,
                parent: None,
                awareness,
            },
            image_digest,
            claims_digest,
            remote_claims_digest,
        )
    }

    /// Admits one child courier under an existing parent courier.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the parent does not exist, the descriptor is invalid, or
    /// storage is exhausted.
    pub fn admit_child_courier(
        &mut self,
        parent: CourierId,
        courier: CourierId,
        principal: PrincipalId<'a>,
        awareness: ClaimAwareness,
        image_digest: ClaimsDigest,
        claims_digest: ClaimsDigest,
        remote_claims_digest: ClaimsDigest,
    ) -> Result<ClaimContextId, ClaimsError> {
        self.authority.register_child_courier(
            parent,
            CourierAuthorityDescriptor {
                courier,
                principal,
                parent: Some(parent),
                awareness,
            },
            image_digest,
            claims_digest,
            remote_claims_digest,
        )
    }

    /// Revalidates one courier against the currently observed image and claim digests.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier is unknown.
    pub fn revalidate_courier(
        &mut self,
        courier: CourierId,
        image_digest: ClaimsDigest,
        claims_digest: ClaimsDigest,
        remote_claims_digest: ClaimsDigest,
    ) -> Result<Option<SealMismatchReason>, ClaimsError> {
        self.authority.revalidate_courier(
            courier,
            image_digest,
            claims_digest,
            remote_claims_digest,
        )
    }

    /// Grants one claim through kernel policy.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier is unknown or the claim cannot be stored.
    pub fn grant_claim(
        &mut self,
        courier: CourierId,
        grant: ClaimGrant<'a>,
    ) -> Result<(), ClaimsError> {
        self.authority.grant_claim(courier, grant)
    }

    /// Revokes one previously granted claim.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier or claim does not exist.
    pub fn revoke_claim(
        &mut self,
        courier: CourierId,
        qualified: QualifiedClaimId<'a>,
    ) -> Result<(), ClaimsError> {
        self.authority.revoke_claim(courier, qualified)
    }

    /// Issues one kernel-issued attachment bond between two couriers.
    ///
    /// # Errors
    ///
    /// Returns an honest error when either courier is unknown or bond storage is exhausted.
    pub fn issue_attachment_bond(
        &mut self,
        provider: CourierId,
        consumer: CourierId,
        channel: fusion_sys::claims::ClaimName<'a>,
        law: TransportAttachmentLaw,
        issued_at_unix_seconds: u64,
        expires_at_unix_seconds: Option<u64>,
    ) -> Result<AttachmentBond<'a>, ClaimsError> {
        self.authority.issue_attachment_bond(
            provider,
            consumer,
            channel,
            law,
            issued_at_unix_seconds,
            expires_at_unix_seconds,
        )
    }

    /// Returns the current attachment-bond revocation epoch.
    #[must_use]
    pub const fn current_revocation_epoch(&self) -> u64 {
        self.authority.current_revocation_epoch()
    }

    /// Bumps the attachment-bond revocation epoch.
    #[must_use]
    pub fn bump_revocation_epoch(&mut self) -> u64 {
        self.authority.bump_revocation_epoch()
    }

    /// Expires stale claims through the composed authority tree.
    pub fn expire_stale_claims(&mut self, now_unix_seconds: u64) {
        self.authority.expire_stale_claims(now_unix_seconds);
    }

    /// Returns one current snapshot for the supplied context.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the context does not exist.
    pub fn inspect_context(
        &self,
        context: ClaimContextId,
    ) -> Result<ClaimContextSnapshot<'a, MAX_CLAIMS, MAX_BONDS>, ClaimsError> {
        self.authority.inspect_context(context)
    }

    /// Returns one current snapshot for the supplied principal.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the principal is unknown.
    pub fn inspect_principal(
        &self,
        principal: PrincipalId<'a>,
    ) -> Result<ClaimContextSnapshot<'a, MAX_CLAIMS, MAX_BONDS>, ClaimsError> {
        self.authority.inspect_principal(principal)
    }

    /// Searches currently active possessed claims matching one pattern.
    #[must_use]
    pub fn search_active_claims<const MAX_MATCHES: usize>(
        &self,
        pattern: QualifiedClaimPattern<'a>,
        now_unix_seconds: u64,
    ) -> ClaimSearchResults<'a, MAX_MATCHES> {
        self.authority
            .search_active_claims::<MAX_MATCHES>(pattern, now_unix_seconds)
    }

    /// Returns one shared view of the composed courier authority.
    #[must_use]
    pub const fn authority(
        &self,
    ) -> &CourierAuthorityRegistry<
        'a,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_CLAIMS,
        MAX_BONDS,
        MAX_SCOPE_NODES,
    > {
        &self.authority
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use fusion_sys::claims::{ClaimGrantLifetime, ClaimGrantSource, ClaimGrantState};

    #[test]
    fn revalidation_resets_local_seal_and_grants_on_digest_mismatch() {
        let mut kernel: KernelCourierAuthority<'_, 4, 4, 4, 4, 16> =
            KernelCourierAuthority::new(47);
        let principal = PrincipalId::parse("httpd@kernel-local[cache]").unwrap();
        let courier = CourierId::new(1);
        let context = kernel
            .admit_root_courier(
                courier,
                principal,
                ClaimAwareness::Black,
                ClaimsDigest::new([1; 32]),
                ClaimsDigest::new([2; 32]),
                ClaimsDigest::new([3; 32]),
            )
            .expect("courier should admit");
        kernel
            .grant_claim(
                courier,
                ClaimGrant {
                    qualified: QualifiedClaimId::parse("httpd@kernel-local[cache]=>net.tcp.9094")
                        .unwrap(),
                    group: None,
                    source: ClaimGrantSource::LocalPolicy,
                    lifetime: ClaimGrantLifetime::Retained,
                    state: ClaimGrantState::Granted,
                    issued_at_unix_seconds: 100,
                    expires_at_unix_seconds: None,
                    seal: fusion_sys::claims::LocalAdmissionSeal::new(
                        fusion_sys::claims::ImageSealId::new(999),
                        ClaimsDigest::zero(),
                        ClaimsDigest::zero(),
                        ClaimsDigest::zero(),
                        47,
                    ),
                },
            )
            .expect("claim should grant");

        let mismatch = kernel
            .revalidate_courier(
                courier,
                ClaimsDigest::new([9; 32]),
                ClaimsDigest::new([2; 32]),
                ClaimsDigest::new([3; 32]),
            )
            .expect("revalidation should succeed");
        assert_eq!(mismatch, Some(SealMismatchReason::ImageDigestChanged));

        let snapshot = kernel
            .inspect_context(context)
            .expect("snapshot should exist");
        assert_eq!(snapshot.claims, [None; 4]);
        assert_eq!(snapshot.descriptor.image_seal.id.get(), 2);
    }

    #[test]
    fn attachment_bond_revocation_tracks_current_epoch() {
        let mut kernel: KernelCourierAuthority<'_, 4, 4, 4, 4, 16> =
            KernelCourierAuthority::new(47);
        kernel
            .admit_root_courier(
                CourierId::new(1),
                PrincipalId::parse("firewall@net[kernel]").unwrap(),
                ClaimAwareness::Black,
                ClaimsDigest::zero(),
                ClaimsDigest::zero(),
                ClaimsDigest::zero(),
            )
            .expect("root courier should admit");
        kernel
            .admit_child_courier(
                CourierId::new(1),
                CourierId::new(2),
                PrincipalId::parse("httpd@cache[server]").unwrap(),
                ClaimAwareness::Black,
                ClaimsDigest::zero(),
                ClaimsDigest::zero(),
                ClaimsDigest::zero(),
            )
            .expect("child courier should admit");

        let bond = kernel
            .issue_attachment_bond(
                CourierId::new(1),
                CourierId::new(2),
                fusion_sys::claims::ClaimName::parse("net.tcp.443").unwrap(),
                TransportAttachmentLaw::ExclusiveSpsc,
                100,
                Some(200),
            )
            .expect("bond should issue");
        assert!(bond.is_active(150, kernel.current_revocation_epoch()));

        let _ = kernel.bump_revocation_epoch();
        assert!(!bond.is_active(150, kernel.current_revocation_epoch()));
    }
}
