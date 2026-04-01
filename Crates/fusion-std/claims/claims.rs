//! Public claims sugar and inspection rendering layered over `fusion-sys`.

use core::fmt;

pub use fusion_sys::claims::*;
use fusion_sys::transport::TransportAttachmentLaw;

/// Display wrapper for one active-claims search result set.
pub struct ActiveClaimSearchDisplay<'a, const MAX_MATCHES: usize> {
    results: &'a ClaimSearchResults<'a, MAX_MATCHES>,
}

impl<'a, const MAX_MATCHES: usize> ActiveClaimSearchDisplay<'a, MAX_MATCHES> {
    #[must_use]
    pub const fn new(results: &'a ClaimSearchResults<'a, MAX_MATCHES>) -> Self {
        Self { results }
    }
}

impl<const MAX_MATCHES: usize> fmt::Display for ActiveClaimSearchDisplay<'_, MAX_MATCHES> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for hit in self.results.matches.iter().flatten() {
            writeln!(f, "{}", hit.qualified)?;
        }
        if self.results.total_matches > self.results.matches.len() {
            write!(
                f,
                "... {} more active claim(s) omitted",
                self.results.total_matches - self.results.matches.len()
            )?;
        }
        Ok(())
    }
}

/// Display wrapper for one live claim-context snapshot.
pub struct ClaimContextInspectionDisplay<'a, const MAX_CLAIMS: usize, const MAX_BONDS: usize> {
    snapshot: &'a ClaimContextSnapshot<'a, MAX_CLAIMS, MAX_BONDS>,
}

impl<'a, const MAX_CLAIMS: usize, const MAX_BONDS: usize>
    ClaimContextInspectionDisplay<'a, MAX_CLAIMS, MAX_BONDS>
{
    #[must_use]
    pub const fn new(snapshot: &'a ClaimContextSnapshot<'a, MAX_CLAIMS, MAX_BONDS>) -> Self {
        Self { snapshot }
    }
}

impl<const MAX_CLAIMS: usize, const MAX_BONDS: usize> fmt::Display
    for ClaimContextInspectionDisplay<'_, MAX_CLAIMS, MAX_BONDS>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let principal = self.snapshot.descriptor.principal;
        // The renderer expands the canonical `context@courier[authority]:port` principal so live
        // metadata inspection stays readable without inventing a second identity grammar. Grouped
        // claim-list output is renderer sugar layered over the one-claim-per-grant substrate below.
        writeln!(f, "principal:   {principal}")?;
        writeln!(f, "context:     {}", principal.context())?;
        writeln!(f, "courier:     {}", principal.courier())?;
        writeln!(f, "authority:   {}", principal.authority())?;
        if let Some(port) = principal.port() {
            writeln!(f, "port:        {port}")?;
        }
        writeln!(
            f,
            "black:       {}",
            self.snapshot.descriptor.awareness.is_black()
        )?;
        writeln!(
            f,
            "image seal:  {}",
            ShortSealDisplay(self.snapshot.descriptor.image_seal.id)
        )?;
        writeln!(
            f,
            "boot epoch:  {}",
            self.snapshot.descriptor.image_seal.boot_epoch
        )?;
        writeln!(f)?;
        writeln!(f, "active claims:")?;
        for claim in self.snapshot.claims.iter().flatten() {
            writeln!(
                f,
                "  {:<20} {:<8} issued:{}{}",
                claim.qualified.claim(),
                claim_state_label(claim.state),
                claim.issued_at_unix_seconds,
                ExpiryDisplay(claim.expires_at_unix_seconds)
            )?;
        }
        writeln!(f)?;
        writeln!(f, "active attestations:")?;
        for bond in self.snapshot.bonds.iter().flatten() {
            let half = if bond.provider.principal == principal {
                bond.provider
            } else {
                bond.consumer
            };
            writeln!(
                f,
                "  bond:{}  {} \u{2194} {}  {}",
                ShortBondDisplay(bond.id),
                half.peer,
                half.channel,
                attachment_law_label(half.law)
            )?;
        }
        Ok(())
    }
}

struct ShortSealDisplay(ImageSealId);

impl fmt::Display for ShortSealDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08x}", self.0.get())
    }
}

struct ShortBondDisplay(AttachmentBondId);

impl fmt::Display for ShortBondDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08x}", self.0.get())
    }
}

struct ExpiryDisplay(Option<u64>);

impl fmt::Display for ExpiryDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(expires_at) => write!(f, " ttl-until:{expires_at}"),
            None => Ok(()),
        }
    }
}

const fn claim_state_label(state: ClaimGrantState) -> &'static str {
    match state {
        ClaimGrantState::Pending => "pending",
        ClaimGrantState::Granted => "granted",
        ClaimGrantState::Consumed => "consumed",
        ClaimGrantState::Revoked => "revoked",
        ClaimGrantState::Expired => "expired",
    }
}

const fn attachment_law_label(law: TransportAttachmentLaw) -> &'static str {
    match law {
        TransportAttachmentLaw::ExclusiveSpsc => "exclusive:spsc",
        TransportAttachmentLaw::PromotableSpmc => "promotable:spmc",
        TransportAttachmentLaw::SharedSpmc => "shared:spmc",
    }
}

#[cfg(all(test, feature = "std", not(target_os = "none")))]
mod tests {
    use super::*;
    use std::format;

    #[test]
    fn inspection_renderer_prints_context_claims_and_bonds() {
        let principal = PrincipalId::parse("httpd@kernel-local[cache]:9094").unwrap();
        let snapshot: ClaimContextSnapshot<'_, 2, 2> = ClaimContextSnapshot {
            descriptor: ClaimContextDescriptor {
                id: ClaimContextId::new(1),
                principal,
                image_seal: LocalAdmissionSeal::new(
                    ImageSealId::new(0xa7f3bc91),
                    ClaimsDigest::zero(),
                    ClaimsDigest::zero(),
                    ClaimsDigest::zero(),
                    47,
                ),
                awareness: ClaimAwareness::Black,
            },
            claims: [
                Some(ClaimGrant {
                    qualified: QualifiedClaimId::parse(
                        "httpd@kernel-local[cache]:9094=>net.tcp.9094",
                    )
                    .unwrap(),
                    group: None,
                    source: ClaimGrantSource::LocalPolicy,
                    lifetime: ClaimGrantLifetime::Retained,
                    state: ClaimGrantState::Granted,
                    issued_at_unix_seconds: 123,
                    expires_at_unix_seconds: Some(456),
                    seal: LocalAdmissionSeal::new(
                        ImageSealId::new(0xa7f3bc91),
                        ClaimsDigest::zero(),
                        ClaimsDigest::zero(),
                        ClaimsDigest::zero(),
                        47,
                    ),
                }),
                None,
            ],
            bonds: [
                Some(AttachmentBond {
                    id: AttachmentBondId::new(0xf819),
                    boot_epoch: 47,
                    provider: AttachmentBondHalf {
                        bond: AttachmentBondId::new(0xf819),
                        principal: PrincipalId::parse("firewall@net[kernel]").unwrap(),
                        peer: principal,
                        peer_seal: ImageSealId::new(0xa7f3bc91),
                        channel: ClaimName::parse("net.tcp.9094").unwrap(),
                        law: TransportAttachmentLaw::ExclusiveSpsc,
                    },
                    consumer: AttachmentBondHalf {
                        bond: AttachmentBondId::new(0xf819),
                        principal,
                        peer: PrincipalId::parse("firewall@net[kernel]").unwrap(),
                        peer_seal: ImageSealId::new(9),
                        channel: ClaimName::parse("net.tcp.9094").unwrap(),
                        law: TransportAttachmentLaw::ExclusiveSpsc,
                    },
                    issued_at_unix_seconds: 123,
                    expires_at_unix_seconds: None,
                    revocation_epoch: 1,
                }),
                None,
            ],
        };

        let rendered = format!("{}", ClaimContextInspectionDisplay::new(&snapshot));
        assert!(rendered.contains("principal:   httpd@kernel-local[cache]:9094"));
        assert!(rendered.contains("authority:   cache"));
        assert!(rendered.contains("port:        9094"));
        assert!(rendered.contains("net.tcp.9094"));
        assert!(rendered.contains("exclusive:spsc"));
    }
}
