//! Claim vocabulary for Fusion principals, grants, image seals, and attachment identity.

use core::fmt;

use crate::contract::pal::interconnect::transport::TransportAttachmentLaw;

/// Stable identifier for one live claim context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClaimContextId(u64);

impl ClaimContextId {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Stable identifier for one locally admitted image seal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageSealId(u64);

impl ImageSealId {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Stable identifier for one authority-issued attachment bond.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AttachmentBondId(u64);

impl AttachmentBondId {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Stable identifier for one remote trusted domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RemoteDomainId(u64);

impl RemoteDomainId {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Fixed-size digest used for image, claims, and remote-claims seal binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClaimsDigest([u8; 32]);

impl ClaimsDigest {
    #[must_use]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn zero() -> Self {
        Self([0; 32])
    }

    #[must_use]
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }
}

/// Locally admitted image seal bound to one exact image and claims digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalAdmissionSeal {
    /// Stable local seal identifier.
    pub id: ImageSealId,
    /// Combined image digest for the admitted image bytes.
    pub image_digest: ClaimsDigest,
    /// Digest of `.claims`.
    pub claims_digest: ClaimsDigest,
    /// Digest of `.rclaims`.
    pub remote_claims_digest: ClaimsDigest,
    /// Local boot/admission epoch in which this seal was minted.
    pub boot_epoch: u64,
    /// Count of locally granted claims currently bound to this seal.
    pub granted_claim_count: u32,
}

impl LocalAdmissionSeal {
    /// Creates one zero-grant local admission seal.
    #[must_use]
    pub const fn new(
        id: ImageSealId,
        image_digest: ClaimsDigest,
        claims_digest: ClaimsDigest,
        remote_claims_digest: ClaimsDigest,
        boot_epoch: u64,
    ) -> Self {
        Self {
            id,
            image_digest,
            claims_digest,
            remote_claims_digest,
            boot_epoch,
            granted_claim_count: 0,
        }
    }

    /// Returns one copy of this seal with the granted-claim count updated.
    #[must_use]
    pub const fn with_granted_claim_count(self, granted_claim_count: u32) -> Self {
        Self {
            granted_claim_count,
            ..self
        }
    }
}

/// Reason one locally admitted image seal no longer matches the currently observed image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SealMismatchReason {
    MissingLocalSeal,
    ImageDigestChanged,
    ClaimsDigestChanged,
    RemoteClaimsDigestChanged,
    PublisherIdentityChanged,
}

/// Claim-aware mode surface for couriers and fibers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ClaimAwareness {
    /// Claim mediation is disabled for this execution lane.
    #[default]
    Blind,
    /// Claim mediation is enabled for this execution lane.
    Black,
}

impl ClaimAwareness {
    #[must_use]
    pub const fn blind() -> Self {
        Self::Blind
    }

    #[must_use]
    pub const fn black() -> Self {
        Self::Black
    }

    #[must_use]
    pub const fn is_blind(self) -> bool {
        matches!(self, Self::Blind)
    }

    #[must_use]
    pub const fn is_black(self) -> bool {
        matches!(self, Self::Black)
    }

    #[must_use]
    pub const fn is_claim_enabled(self) -> bool {
        self.is_black()
    }
}

impl fmt::Display for ClaimAwareness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Blind => f.write_str("blind"),
            Self::Black => f.write_str("black"),
        }
    }
}

/// Lifetime model for one granted claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClaimGrantLifetime {
    /// One claim use is consumed exactly once.
    OneShot,
    /// One claim remains active until explicitly revoked or the seal changes.
    Retained,
    /// One claim remains active until the supplied absolute expiry timestamp.
    ExpiresAt { unix_seconds: u64 },
}

/// Current state of one granted claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClaimGrantState {
    Pending,
    Granted,
    Consumed,
    Revoked,
    Expired,
}

/// Source by which one claim grant entered the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClaimGrantSource {
    LocalPolicy,
    RemoteDomain(RemoteDomainId),
    AuthorityIntrinsic,
    AttachmentAttestation,
}

/// Stable claim group name used to aggregate related claim scopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClaimGroupName<'a> {
    raw: &'a str,
}

impl<'a> ClaimGroupName<'a> {
    /// Parses one group name.
    ///
    /// # Errors
    ///
    /// Returns an error when the group name is empty or contains `:`.
    pub fn parse(raw: &'a str) -> Result<Self, ClaimError> {
        if raw.is_empty() || raw.contains(':') {
            return Err(ClaimError::invalid_claim_name());
        }
        Ok(Self { raw })
    }

    #[must_use]
    pub const fn as_str(&self) -> &'a str {
        self.raw
    }
}

impl fmt::Display for ClaimGroupName<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.raw)
    }
}

/// Fully qualified remote domain name against which one remote signed claim is bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RemoteDomainName<'a> {
    raw: &'a str,
}

impl<'a> RemoteDomainName<'a> {
    /// Parses one remote domain name.
    ///
    /// # Errors
    ///
    /// Returns an error when the domain is empty or malformed.
    pub fn parse(raw: &'a str) -> Result<Self, ClaimError> {
        if raw.is_empty() || raw.starts_with('.') || raw.ends_with('.') || raw.contains(':') {
            return Err(ClaimError::invalid_pattern());
        }
        if raw.split('.').any(str::is_empty) {
            return Err(ClaimError::invalid_pattern());
        }
        Ok(Self { raw })
    }

    #[must_use]
    pub const fn as_str(&self) -> &'a str {
        self.raw
    }
}

impl fmt::Display for RemoteDomainName<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.raw)
    }
}

/// Error surfaced while parsing or matching claim identity text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClaimError {
    kind: ClaimErrorKind,
}

impl ClaimError {
    #[must_use]
    pub const fn kind(self) -> ClaimErrorKind {
        self.kind
    }

    #[must_use]
    pub const fn invalid_principal() -> Self {
        Self {
            kind: ClaimErrorKind::InvalidPrincipal,
        }
    }

    #[must_use]
    pub const fn invalid_claim_name() -> Self {
        Self {
            kind: ClaimErrorKind::InvalidClaimName,
        }
    }

    #[must_use]
    pub const fn invalid_qualified_claim() -> Self {
        Self {
            kind: ClaimErrorKind::InvalidQualifiedClaim,
        }
    }

    #[must_use]
    pub const fn invalid_pattern() -> Self {
        Self {
            kind: ClaimErrorKind::InvalidPattern,
        }
    }
}

/// Classification of claim-text failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClaimErrorKind {
    InvalidPrincipal,
    InvalidClaimName,
    InvalidQualifiedClaim,
    InvalidPattern,
}

impl fmt::Display for ClaimErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPrincipal => f.write_str("invalid principal identity"),
            Self::InvalidClaimName => f.write_str("invalid claim name"),
            Self::InvalidQualifiedClaim => f.write_str("invalid qualified claim identity"),
            Self::InvalidPattern => f.write_str("invalid claim pattern"),
        }
    }
}

impl fmt::Display for ClaimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

/// Canonical principal identity: `context[#instance]@courier[authority][:port]`.
///
/// The audit standard writes this as `@courier[[authority]][:port]` to make it explicit that the
/// authority component is always bracketed. The actual stored syntax still contains one literal pair
/// of brackets, because the machine has enough problems already.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PrincipalId<'a> {
    raw: &'a str,
    context: &'a str,
    courier: &'a str,
    authority: &'a str,
    port: Option<&'a str>,
}

impl<'a> PrincipalId<'a> {
    /// Parses one canonical principal identity.
    ///
    /// The authority portion is always bracketed so web-domain and IPv6-style authority text stays
    /// opaque to Fusion core, and any optional `:port` always lives outside those brackets.
    ///
    /// # Errors
    ///
    /// Returns an error when `raw` is not `context[#instance]@courier[authority][:port]`.
    pub fn parse(raw: &'a str) -> Result<Self, ClaimError> {
        if raw.is_empty() {
            return Err(ClaimError::invalid_principal());
        }
        let (context, rest) = raw
            .split_once('@')
            .ok_or_else(ClaimError::invalid_principal)?;
        let Some(authority_start) = rest.find('[') else {
            return Err(ClaimError::invalid_principal());
        };
        let Some(relative_authority_end) = rest[authority_start + 1..].find(']') else {
            return Err(ClaimError::invalid_principal());
        };
        let authority_end = authority_start + 1 + relative_authority_end;
        let courier = &rest[..authority_start];
        let authority = &rest[authority_start + 1..authority_end];
        let trailing = &rest[authority_end + 1..];
        let port = if trailing.is_empty() {
            None
        } else {
            let port = trailing
                .strip_prefix(':')
                .ok_or_else(ClaimError::invalid_principal)?;
            if port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(ClaimError::invalid_principal());
            }
            Some(port)
        };
        if context.is_empty()
            || courier.is_empty()
            || authority.is_empty()
            || context.contains('[')
            || context.contains(']')
            || context.contains(':')
            || context.contains("=>")
            || courier.contains('@')
            || courier.contains('[')
            || courier.contains(']')
            || courier.contains(':')
            || courier.contains("=>")
            || authority.contains('[')
            || authority.contains(']')
            || authority.chars().any(char::is_whitespace)
            || trailing.contains('[')
            || trailing.contains(']')
            || rest[authority_end + 1..].contains('@')
        {
            return Err(ClaimError::invalid_principal());
        }
        Ok(Self {
            raw,
            context,
            courier,
            authority,
            port,
        })
    }

    #[must_use]
    pub const fn as_str(&self) -> &'a str {
        self.raw
    }

    #[must_use]
    pub const fn context(&self) -> &'a str {
        self.context
    }

    #[must_use]
    pub const fn courier(&self) -> &'a str {
        self.courier
    }

    #[must_use]
    pub const fn authority(&self) -> &'a str {
        self.authority
    }

    #[must_use]
    pub const fn port(&self) -> Option<&'a str> {
        self.port
    }
}

impl fmt::Display for PrincipalId<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.raw)
    }
}

/// Canonical claim name relative to one principal: `claim.scope`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClaimName<'a> {
    raw: &'a str,
}

impl<'a> ClaimName<'a> {
    /// Parses one claim name.
    ///
    /// # Errors
    ///
    /// Returns an error when `raw` is empty or contains the qualified-claim separator.
    pub fn parse(raw: &'a str) -> Result<Self, ClaimError> {
        if raw.is_empty() || raw.contains("=>") {
            return Err(ClaimError::invalid_claim_name());
        }
        Ok(Self { raw })
    }

    #[must_use]
    pub const fn as_str(&self) -> &'a str {
        self.raw
    }
}

impl fmt::Display for ClaimName<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.raw)
    }
}

/// Declarative local claim recorded in `.claims`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClaimDeclaration<'a> {
    pub claim: ClaimName<'a>,
    pub group: Option<ClaimGroupName<'a>>,
}

/// Declarative remote or delegated claim recorded in `.rclaims`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RemoteClaimDeclaration<'a> {
    pub claim: ClaimName<'a>,
    pub group: Option<ClaimGroupName<'a>>,
    pub remote_domain: RemoteDomainName<'a>,
}

/// One concrete granted claim bound to one locally admitted seal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClaimGrant<'a> {
    pub qualified: QualifiedClaimId<'a>,
    pub group: Option<ClaimGroupName<'a>>,
    pub source: ClaimGrantSource,
    pub lifetime: ClaimGrantLifetime,
    pub state: ClaimGrantState,
    pub issued_at_unix_seconds: u64,
    pub expires_at_unix_seconds: Option<u64>,
    pub seal: LocalAdmissionSeal,
}

impl ClaimGrant<'_> {
    #[must_use]
    pub const fn is_active(self, now_unix_seconds: u64) -> bool {
        match self.state {
            ClaimGrantState::Granted => match self.expires_at_unix_seconds {
                Some(expires) => expires > now_unix_seconds,
                None => true,
            },
            ClaimGrantState::Pending
            | ClaimGrantState::Consumed
            | ClaimGrantState::Revoked
            | ClaimGrantState::Expired => false,
        }
    }
}

/// One half of one authority-attested attachment bond.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AttachmentBondHalf<'a> {
    pub bond: AttachmentBondId,
    pub principal: PrincipalId<'a>,
    pub peer: PrincipalId<'a>,
    pub peer_seal: ImageSealId,
    pub channel: ClaimName<'a>,
    pub law: TransportAttachmentLaw,
}

/// One live authority-attested attachment bond binding two principals to one channel relationship.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AttachmentBond<'a> {
    pub id: AttachmentBondId,
    pub boot_epoch: u64,
    pub provider: AttachmentBondHalf<'a>,
    pub consumer: AttachmentBondHalf<'a>,
    pub issued_at_unix_seconds: u64,
    pub expires_at_unix_seconds: Option<u64>,
    pub revocation_epoch: u64,
}

impl AttachmentBond<'_> {
    #[must_use]
    pub const fn is_active(self, now_unix_seconds: u64, current_revocation_epoch: u64) -> bool {
        self.revocation_epoch >= current_revocation_epoch
            && match self.expires_at_unix_seconds {
                Some(expires) => expires > now_unix_seconds,
                None => true,
            }
    }
}

/// Canonical qualified claim identity: `context[#instance]@courier[authority][:port]=>claim.scope`.
///
/// Grouped forms like `principal=>[claim.one, claim.two]` are operator/query sugar only. Core
/// substrate code still admits one canonical qualified claim at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct QualifiedClaimId<'a> {
    raw: &'a str,
    principal: PrincipalId<'a>,
    claim: ClaimName<'a>,
}

impl<'a> QualifiedClaimId<'a> {
    /// Parses one fully qualified claim identity.
    ///
    /// `=>` is the only principal-to-claim delimiter so authority text can use `:port` without
    /// colliding with claim qualification.
    ///
    /// # Errors
    ///
    /// Returns an error when `raw` is not `context[#instance]@courier[authority][:port]=>claim.scope`.
    pub fn parse(raw: &'a str) -> Result<Self, ClaimError> {
        let (principal, claim) = raw
            .split_once("=>")
            .ok_or_else(ClaimError::invalid_qualified_claim)?;
        let principal =
            PrincipalId::parse(principal).map_err(|_| ClaimError::invalid_qualified_claim())?;
        let claim = ClaimName::parse(claim).map_err(|_| ClaimError::invalid_qualified_claim())?;
        Ok(Self {
            raw,
            principal,
            claim,
        })
    }

    #[must_use]
    pub const fn as_str(&self) -> &'a str {
        self.raw
    }

    #[must_use]
    pub const fn principal(&self) -> PrincipalId<'a> {
        self.principal
    }

    #[must_use]
    pub const fn claim(&self) -> ClaimName<'a> {
        self.claim
    }
}

impl fmt::Display for QualifiedClaimId<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.raw)
    }
}

/// Glob-style pattern matched against canonical principal identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PrincipalPattern<'a> {
    raw: &'a str,
}

impl<'a> PrincipalPattern<'a> {
    /// Parses one principal pattern.
    ///
    /// # Errors
    ///
    /// Returns an error when `raw` is empty or contains the qualified-claim separator.
    pub fn parse(raw: &'a str) -> Result<Self, ClaimError> {
        if raw.is_empty() || raw.contains("=>") || ends_with_unescaped_escape(raw) {
            return Err(ClaimError::invalid_pattern());
        }
        Ok(Self { raw })
    }

    #[must_use]
    pub const fn as_str(&self) -> &'a str {
        self.raw
    }

    #[must_use]
    pub fn matches_principal(&self, principal: PrincipalId<'_>) -> bool {
        glob_matches(self.raw, principal.as_str())
    }
}

/// Glob-style pattern matched against canonical claim names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClaimPattern<'a> {
    raw: &'a str,
}

impl<'a> ClaimPattern<'a> {
    /// Parses one claim pattern.
    ///
    /// # Errors
    ///
    /// Returns an error when `raw` is empty or contains the qualified-claim separator.
    pub fn parse(raw: &'a str) -> Result<Self, ClaimError> {
        if raw.is_empty() || raw.contains("=>") || ends_with_unescaped_escape(raw) {
            return Err(ClaimError::invalid_pattern());
        }
        Ok(Self { raw })
    }

    #[must_use]
    pub const fn as_str(&self) -> &'a str {
        self.raw
    }

    #[must_use]
    pub fn matches_claim(&self, claim: ClaimName<'_>) -> bool {
        glob_matches(self.raw, claim.as_str())
    }
}

/// Glob-style pattern matched against fully qualified claim identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct QualifiedClaimPattern<'a> {
    raw: &'a str,
    principal: PrincipalPattern<'a>,
    claim: ClaimPattern<'a>,
}

impl<'a> QualifiedClaimPattern<'a> {
    /// Parses one fully qualified claim pattern.
    ///
    /// The pattern syntax follows the same exact `principal=>claim` boundary as the canonical
    /// identifier grammar; only the principal/claim bodies become glob-aware.
    ///
    /// # Errors
    ///
    /// Returns an error when `raw` is not `<principal-pattern>=><claim-pattern>`.
    pub fn parse(raw: &'a str) -> Result<Self, ClaimError> {
        let (principal, claim) = raw
            .split_once("=>")
            .ok_or_else(ClaimError::invalid_pattern)?;
        let principal = PrincipalPattern::parse(principal)?;
        let claim = ClaimPattern::parse(claim)?;
        Ok(Self {
            raw,
            principal,
            claim,
        })
    }

    #[must_use]
    pub const fn as_str(&self) -> &'a str {
        self.raw
    }

    #[must_use]
    pub const fn principal(&self) -> PrincipalPattern<'a> {
        self.principal
    }

    #[must_use]
    pub const fn claim(&self) -> ClaimPattern<'a> {
        self.claim
    }

    #[must_use]
    pub fn matches_qualified(&self, qualified: QualifiedClaimId<'_>) -> bool {
        self.principal.matches_principal(qualified.principal())
            && self.claim.matches_claim(qualified.claim())
    }
}

fn ends_with_unescaped_escape(raw: &str) -> bool {
    let mut escaped = false;
    for byte in raw.bytes() {
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        }
    }
    escaped
}

fn glob_matches(pattern: &str, candidate: &str) -> bool {
    let pattern = pattern.as_bytes();
    let candidate = candidate.as_bytes();
    let mut pattern_index = 0usize;
    let mut candidate_index = 0usize;
    let mut star_index: Option<usize> = None;
    let mut fallback_candidate = 0usize;

    while candidate_index < candidate.len() {
        if pattern_index < pattern.len() && pattern[pattern_index] == b'\\' {
            if pattern_index + 1 >= pattern.len() {
                return false;
            }
            pattern_index += 1;
            if pattern[pattern_index] == candidate[candidate_index] {
                pattern_index += 1;
                candidate_index += 1;
                continue;
            }
        } else if pattern_index < pattern.len()
            && (pattern[pattern_index] == candidate[candidate_index]
                || pattern[pattern_index] == b'?')
        {
            pattern_index += 1;
            candidate_index += 1;
            continue;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star_index = Some(pattern_index);
            pattern_index += 1;
            fallback_candidate = candidate_index;
            continue;
        }

        if let Some(star) = star_index {
            pattern_index = star + 1;
            fallback_candidate += 1;
            candidate_index = fallback_candidate;
        } else {
            return false;
        }
    }

    while pattern_index < pattern.len() {
        if pattern[pattern_index] == b'*' {
            pattern_index += 1;
            continue;
        }
        if pattern[pattern_index] == b'\\' {
            return false;
        }
        break;
    }

    pattern_index == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::{
        AttachmentBond,
        AttachmentBondHalf,
        AttachmentBondId,
        ClaimAwareness,
        ClaimDeclaration,
        ClaimGrant,
        ClaimGrantLifetime,
        ClaimGrantSource,
        ClaimGrantState,
        ClaimGroupName,
        ClaimName,
        ClaimPattern,
        ClaimsDigest,
        ImageSealId,
        LocalAdmissionSeal,
        PrincipalId,
        PrincipalPattern,
        QualifiedClaimId,
        QualifiedClaimPattern,
        RemoteClaimDeclaration,
        RemoteDomainId,
        RemoteDomainName,
        SealMismatchReason,
    };
    use crate::contract::pal::interconnect::transport::TransportAttachmentLaw;

    #[test]
    fn parse_principal_identity() {
        let principal =
            PrincipalId::parse("shell#03@walance[pvas-local]:22").expect("principal should parse");
        assert_eq!(principal.context(), "shell#03");
        assert_eq!(principal.courier(), "walance");
        assert_eq!(principal.authority(), "pvas-local");
        assert_eq!(principal.port(), Some("22"));
        assert_eq!(principal.as_str(), "shell#03@walance[pvas-local]:22");
    }

    #[test]
    fn parse_qualified_claim_identity() {
        let claim = QualifiedClaimId::parse("shell#03@walance[pvas-local]:22=>net.tcp.443")
            .expect("qualified claim should parse");
        assert_eq!(
            claim.principal().as_str(),
            "shell#03@walance[pvas-local]:22"
        );
        assert_eq!(claim.claim().as_str(), "net.tcp.443");
    }

    #[test]
    fn middle_wildcards_match() {
        let pattern =
            QualifiedClaimPattern::parse("*=>n*").expect("qualified pattern should parse");
        let net = QualifiedClaimId::parse("httpd@web[server]=>net.tcp.443").unwrap();
        let nic = QualifiedClaimId::parse("driver@kernel[local]=>nic.rx").unwrap();
        let fs = QualifiedClaimId::parse("cache@kernel[local]=>fs.read./var/cache").unwrap();
        assert!(pattern.matches_qualified(net));
        assert!(pattern.matches_qualified(nic));
        assert!(!pattern.matches_qualified(fs));
    }

    #[test]
    fn principal_and_claim_patterns_match_exact_examples() {
        let principal =
            PrincipalId::parse("httpd@web[cache.server]").expect("principal should parse");
        let claim = ClaimName::parse("hw.nic.rx").expect("claim should parse");
        assert!(
            PrincipalPattern::parse("httpd@web[cache.server]")
                .unwrap()
                .matches_principal(principal)
        );
        assert!(
            PrincipalPattern::parse("*@*[*.server]")
                .unwrap()
                .matches_principal(principal)
        );
        assert!(
            ClaimPattern::parse("hw.nic.*")
                .unwrap()
                .matches_claim(claim)
        );
    }

    #[test]
    fn escaped_metacharacters_match_literally() {
        let pattern = ClaimPattern::parse(r"fs.read.\*").expect("pattern should parse");
        let claim = ClaimName::parse("fs.read.*").expect("claim should parse");
        assert!(pattern.matches_claim(claim));
    }

    #[test]
    fn claim_awareness_reports_black_switch_state() {
        assert!(ClaimAwareness::black().is_black());
        assert!(ClaimAwareness::black().is_claim_enabled());
        assert!(ClaimAwareness::blind().is_blind());
    }

    #[test]
    fn local_admission_seal_starts_ungranted() {
        let seal = LocalAdmissionSeal::new(
            ImageSealId::new(7),
            ClaimsDigest::zero(),
            ClaimsDigest::zero(),
            ClaimsDigest::zero(),
            47,
        );
        assert_eq!(seal.granted_claim_count, 0);
        assert_eq!(seal.boot_epoch, 47);
    }

    #[test]
    fn claim_grant_reports_active_only_while_granted_and_unexpired() {
        let grant = ClaimGrant {
            qualified: QualifiedClaimId::parse("httpd@web[server]=>net.tcp.443").unwrap(),
            group: Some(ClaimGroupName::parse("net.listen").unwrap()),
            source: ClaimGrantSource::RemoteDomain(RemoteDomainId::new(3)),
            lifetime: ClaimGrantLifetime::ExpiresAt { unix_seconds: 200 },
            state: ClaimGrantState::Granted,
            issued_at_unix_seconds: 100,
            expires_at_unix_seconds: Some(200),
            seal: LocalAdmissionSeal::new(
                ImageSealId::new(1),
                ClaimsDigest::zero(),
                ClaimsDigest::zero(),
                ClaimsDigest::zero(),
                47,
            ),
        };
        assert!(grant.is_active(150));
        assert!(!grant.is_active(200));
    }

    #[test]
    fn remote_claim_declaration_binds_to_remote_domain() {
        let remote = RemoteClaimDeclaration {
            claim: ClaimName::parse("net.tcp.443").unwrap(),
            group: Some(ClaimGroupName::parse("net.listen").unwrap()),
            remote_domain: RemoteDomainName::parse("mobile.device").unwrap(),
        };
        let local = ClaimDeclaration {
            claim: ClaimName::parse("fs.read./var/cache").unwrap(),
            group: None,
        };
        assert_eq!(remote.remote_domain.as_str(), "mobile.device");
        assert_eq!(local.claim.as_str(), "fs.read./var/cache");
    }

    #[test]
    fn attachment_bond_tracks_exclusive_law_and_peers() {
        let provider = PrincipalId::parse("firewall@net[kernel]").unwrap();
        let consumer = PrincipalId::parse("httpd@cache[server]").unwrap();
        let bond = AttachmentBond {
            id: AttachmentBondId::new(11),
            boot_epoch: 47,
            provider: AttachmentBondHalf {
                bond: AttachmentBondId::new(11),
                principal: provider,
                peer: consumer,
                peer_seal: ImageSealId::new(9),
                channel: ClaimName::parse("net.tcp.443").unwrap(),
                law: TransportAttachmentLaw::ExclusiveSpsc,
            },
            consumer: AttachmentBondHalf {
                bond: AttachmentBondId::new(11),
                principal: consumer,
                peer: provider,
                peer_seal: ImageSealId::new(8),
                channel: ClaimName::parse("net.tcp.443").unwrap(),
                law: TransportAttachmentLaw::ExclusiveSpsc,
            },
            issued_at_unix_seconds: 100,
            expires_at_unix_seconds: Some(200),
            revocation_epoch: 1,
        };
        assert!(bond.is_active(150, 1));
        assert!(!bond.is_active(200, 1));
    }

    #[test]
    fn seal_mismatch_reason_is_copyable_vocabulary() {
        let reason = SealMismatchReason::ClaimsDigestChanged;
        assert_eq!(reason, SealMismatchReason::ClaimsDigestChanged);
    }
}
