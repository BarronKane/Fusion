//! fusion-sys claims vocabulary plus fixed-capacity claim-context composition primitives.

pub use fusion_pal::sys::claims::*;

use core::fmt;

use crate::courier::CourierSupport;
use crate::domain::CourierId;
use crate::transport::TransportAttachmentLaw;

/// Error surfaced while composing or querying live claim state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClaimsError {
    kind: ClaimsErrorKind,
}

impl ClaimsError {
    #[must_use]
    pub const fn kind(self) -> ClaimsErrorKind {
        self.kind
    }

    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: ClaimsErrorKind::Unsupported,
        }
    }

    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: ClaimsErrorKind::Invalid,
        }
    }

    #[must_use]
    pub const fn not_found() -> Self {
        Self {
            kind: ClaimsErrorKind::NotFound,
        }
    }

    #[must_use]
    pub const fn permission_denied() -> Self {
        Self {
            kind: ClaimsErrorKind::PermissionDenied,
        }
    }

    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self {
            kind: ClaimsErrorKind::ResourceExhausted,
        }
    }

    #[must_use]
    pub const fn state_conflict() -> Self {
        Self {
            kind: ClaimsErrorKind::StateConflict,
        }
    }

    #[must_use]
    pub const fn revoked() -> Self {
        Self {
            kind: ClaimsErrorKind::Revoked,
        }
    }

    #[must_use]
    pub const fn expired() -> Self {
        Self {
            kind: ClaimsErrorKind::Expired,
        }
    }
}

/// Classification of live-claims failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClaimsErrorKind {
    Unsupported,
    Invalid,
    NotFound,
    PermissionDenied,
    ResourceExhausted,
    StateConflict,
    Revoked,
    Expired,
}

impl fmt::Display for ClaimsErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("claims operation unsupported"),
            Self::Invalid => f.write_str("invalid claims request"),
            Self::NotFound => f.write_str("claim or context not found"),
            Self::PermissionDenied => f.write_str("claim request denied"),
            Self::ResourceExhausted => f.write_str("claims storage exhausted"),
            Self::StateConflict => f.write_str("claims state conflict"),
            Self::Revoked => f.write_str("claim revoked"),
            Self::Expired => f.write_str("claim expired"),
        }
    }
}

impl fmt::Display for ClaimsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

/// One live claim context carried by one black or claim-blind execution principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClaimContextDescriptor<'a> {
    pub id: ClaimContextId,
    pub principal: PrincipalId<'a>,
    pub image_seal: LocalAdmissionSeal,
    pub awareness: ClaimAwareness,
}

/// One fixed-capacity snapshot of one live claim context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaimContextSnapshot<'a, const MAX_CLAIMS: usize, const MAX_BONDS: usize> {
    pub descriptor: ClaimContextDescriptor<'a>,
    pub claims: [Option<ClaimGrant<'a>>; MAX_CLAIMS],
    pub bonds: [Option<AttachmentBond<'a>>; MAX_BONDS],
}

/// One active claim match returned from a search query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActiveClaimMatch<'a> {
    pub context: ClaimContextId,
    pub principal: PrincipalId<'a>,
    pub qualified: QualifiedClaimId<'a>,
    pub seal: ImageSealId,
}

/// Fixed-capacity search results for active possessed claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaimSearchResults<'a, const MAX_MATCHES: usize> {
    pub matches: [Option<ActiveClaimMatch<'a>>; MAX_MATCHES],
    pub total_matches: usize,
}

// Claims are hierarchical dotted scopes, so each `.`-separated segment becomes one edge in the
// per-context prefix trie. That lets exact and subtree lookups follow the shape of the namespace
// instead of flattening it into one sad string table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClaimTrieNode<'a> {
    segment: &'a str,
    first_child: Option<usize>,
    next_sibling: Option<usize>,
    terminal_claim: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClaimScopeTrie<'a, const MAX_SCOPE_NODES: usize> {
    nodes: [Option<ClaimTrieNode<'a>>; MAX_SCOPE_NODES],
    next_free: usize,
}

impl<'a, const MAX_SCOPE_NODES: usize> ClaimScopeTrie<'a, MAX_SCOPE_NODES> {
    const fn new() -> Self {
        let mut nodes = [None; MAX_SCOPE_NODES];
        let next_free = if MAX_SCOPE_NODES != 0 {
            nodes[0] = Some(ClaimTrieNode {
                segment: "",
                first_child: None,
                next_sibling: None,
                terminal_claim: None,
            });
            1
        } else {
            0
        };
        Self { nodes, next_free }
    }

    fn clear(&mut self) {
        self.nodes = [None; MAX_SCOPE_NODES];
        if MAX_SCOPE_NODES != 0 {
            self.nodes[0] = Some(ClaimTrieNode {
                segment: "",
                first_child: None,
                next_sibling: None,
                terminal_claim: None,
            });
            self.next_free = 1;
        } else {
            self.next_free = 0;
        }
    }

    fn insert(&mut self, claim: ClaimName<'a>, claim_index: usize) -> Result<(), ClaimsError> {
        if self.nodes.first().and_then(|slot| *slot).is_none() {
            return Err(ClaimsError::resource_exhausted());
        }
        let mut current = 0usize;
        for segment in claim.as_str().split('.') {
            let next = self.find_or_insert_child(current, segment)?;
            current = next;
        }
        let node = self
            .nodes
            .get_mut(current)
            .and_then(Option::as_mut)
            .ok_or_else(ClaimsError::state_conflict)?;
        if node.terminal_claim.is_some() {
            return Err(ClaimsError::state_conflict());
        }
        node.terminal_claim = Some(claim_index);
        Ok(())
    }

    fn exact_claim_index(&self, claim: ClaimName<'a>) -> Option<usize> {
        let mut current = 0usize;
        for segment in claim.as_str().split('.') {
            current = self.find_child(current, segment)?;
        }
        self.nodes
            .get(current)
            .and_then(|slot| slot.as_ref())
            .and_then(|node| node.terminal_claim)
    }

    fn collect_prefix_claim_indices<const MAX_MATCHES: usize>(
        &self,
        prefix: &str,
        matches: &mut [Option<usize>; MAX_MATCHES],
        total_matches: &mut usize,
    ) {
        let Some(start) = self.prefix_node(prefix) else {
            return;
        };
        let mut stack = [None; MAX_SCOPE_NODES];
        let mut stack_len = 0usize;
        stack[stack_len] = Some(start);
        stack_len += 1;

        while stack_len != 0 {
            stack_len -= 1;
            let Some(index) = stack[stack_len] else {
                continue;
            };
            let Some(node) = self.nodes.get(index).and_then(|slot| slot.as_ref()) else {
                continue;
            };
            if let Some(claim_index) = node.terminal_claim {
                if *total_matches < MAX_MATCHES {
                    matches[*total_matches] = Some(claim_index);
                }
                *total_matches += 1;
            }

            let mut child = node.first_child;
            while let Some(child_index) = child {
                if stack_len < MAX_SCOPE_NODES {
                    stack[stack_len] = Some(child_index);
                    stack_len += 1;
                }
                child = self
                    .nodes
                    .get(child_index)
                    .and_then(|slot| slot.as_ref())
                    .and_then(|entry| entry.next_sibling);
            }
        }
    }

    fn prefix_node(&self, prefix: &str) -> Option<usize> {
        if prefix.is_empty() || prefix == "*" {
            return Some(0);
        }
        let mut current = 0usize;
        for segment in prefix.split('.') {
            current = self.find_child(current, segment)?;
        }
        Some(current)
    }

    fn find_child(&self, parent: usize, segment: &str) -> Option<usize> {
        let mut current = self
            .nodes
            .get(parent)
            .and_then(|slot| slot.as_ref())
            .and_then(|node| node.first_child);
        while let Some(index) = current {
            let node = self.nodes.get(index).and_then(|slot| slot.as_ref())?;
            if node.segment == segment {
                return Some(index);
            }
            current = node.next_sibling;
        }
        None
    }

    fn find_or_insert_child(
        &mut self,
        parent: usize,
        segment: &'a str,
    ) -> Result<usize, ClaimsError> {
        if let Some(index) = self.find_child(parent, segment) {
            return Ok(index);
        }
        let index = self.next_free;
        if index >= MAX_SCOPE_NODES {
            return Err(ClaimsError::resource_exhausted());
        }
        self.next_free += 1;
        self.nodes[index] = Some(ClaimTrieNode {
            segment,
            first_child: None,
            next_sibling: None,
            terminal_claim: None,
        });
        let parent_node = self
            .nodes
            .get_mut(parent)
            .and_then(Option::as_mut)
            .ok_or_else(ClaimsError::state_conflict)?;
        match parent_node.first_child {
            Some(first_child) => {
                let mut tail = first_child;
                loop {
                    let next = self
                        .nodes
                        .get(tail)
                        .and_then(|slot| slot.as_ref())
                        .and_then(|node| node.next_sibling);
                    if let Some(next_index) = next {
                        tail = next_index;
                    } else {
                        break;
                    }
                }
                let tail_node = self
                    .nodes
                    .get_mut(tail)
                    .and_then(Option::as_mut)
                    .ok_or_else(ClaimsError::state_conflict)?;
                tail_node.next_sibling = Some(index);
            }
            None => parent_node.first_child = Some(index),
        }
        Ok(index)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaimPatternQuery<'a> {
    All,
    Exact(ClaimName<'a>),
    Prefix(&'a str),
    Complex,
}

impl<'a> ClaimPatternQuery<'a> {
    fn classify(pattern: ClaimPattern<'a>) -> Self {
        let raw = pattern.as_str();
        if raw == "*" {
            return Self::All;
        }
        if raw.contains('?') || raw.contains('\\') {
            return Self::Complex;
        }
        if let Some(prefix) = raw.strip_suffix(".*") {
            if !prefix.is_empty() && !prefix.contains('*') {
                return Self::Prefix(prefix);
            }
        }
        if raw.contains('*') {
            return Self::Complex;
        }
        match ClaimName::parse(raw) {
            Ok(exact) => Self::Exact(exact),
            Err(_) => Self::Complex,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClaimContextRecord<
    'a,
    const MAX_CLAIMS: usize,
    const MAX_BONDS: usize,
    const MAX_SCOPE_NODES: usize,
> {
    descriptor: ClaimContextDescriptor<'a>,
    claims: [Option<ClaimGrant<'a>>; MAX_CLAIMS],
    bonds: [Option<AttachmentBond<'a>>; MAX_BONDS],
    claim_trie: ClaimScopeTrie<'a, MAX_SCOPE_NODES>,
}

impl<'a, const MAX_CLAIMS: usize, const MAX_BONDS: usize, const MAX_SCOPE_NODES: usize>
    ClaimContextRecord<'a, MAX_CLAIMS, MAX_BONDS, MAX_SCOPE_NODES>
{
    const fn new(descriptor: ClaimContextDescriptor<'a>) -> Self {
        Self {
            descriptor,
            claims: [None; MAX_CLAIMS],
            bonds: [None; MAX_BONDS],
            claim_trie: ClaimScopeTrie::new(),
        }
    }
}

/// Fixed-capacity claim registry used by couriers, authority surfaces, and inspection tooling.
///
/// This still uses a flat context array at the top level, which is prototype scaffolding rather than
/// the final datastore shape. The important part that is already honest is the per-context claim trie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaimContextRegistry<
    'a,
    const MAX_CONTEXTS: usize,
    const MAX_CLAIMS: usize,
    const MAX_BONDS: usize,
    const MAX_SCOPE_NODES: usize,
> {
    contexts:
        [Option<ClaimContextRecord<'a, MAX_CLAIMS, MAX_BONDS, MAX_SCOPE_NODES>>; MAX_CONTEXTS],
}

impl<
    'a,
    const MAX_CONTEXTS: usize,
    const MAX_CLAIMS: usize,
    const MAX_BONDS: usize,
    const MAX_SCOPE_NODES: usize,
> ClaimContextRegistry<'a, MAX_CONTEXTS, MAX_CLAIMS, MAX_BONDS, MAX_SCOPE_NODES>
{
    /// Creates one empty fixed-capacity claims registry.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            contexts: [None; MAX_CONTEXTS],
        }
    }

    /// Registers one claim context.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the identifier or principal already exists or capacity is
    /// exhausted.
    pub fn register_context(
        &mut self,
        descriptor: ClaimContextDescriptor<'a>,
    ) -> Result<(), ClaimsError> {
        if self.find_context_index(descriptor.id).is_some()
            || self.find_principal_index(descriptor.principal).is_some()
        {
            return Err(ClaimsError::state_conflict());
        }
        let Some(slot) = self.contexts.iter_mut().find(|slot| slot.is_none()) else {
            return Err(ClaimsError::resource_exhausted());
        };
        *slot = Some(ClaimContextRecord::new(descriptor));
        Ok(())
    }

    /// Returns one registered claim-context snapshot.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the claim context does not exist.
    pub fn snapshot(
        &self,
        context: ClaimContextId,
    ) -> Result<ClaimContextSnapshot<'a, MAX_CLAIMS, MAX_BONDS>, ClaimsError> {
        let record = self
            .find_context(context)
            .ok_or_else(ClaimsError::not_found)?;
        Ok(ClaimContextSnapshot {
            descriptor: record.descriptor,
            claims: record.claims,
            bonds: record.bonds,
        })
    }

    /// Returns one snapshot looked up by principal identity.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the principal is not registered.
    pub fn snapshot_by_principal(
        &self,
        principal: PrincipalId<'a>,
    ) -> Result<ClaimContextSnapshot<'a, MAX_CLAIMS, MAX_BONDS>, ClaimsError> {
        let record = self
            .find_principal(principal)
            .ok_or_else(ClaimsError::not_found)?;
        Ok(ClaimContextSnapshot {
            descriptor: record.descriptor,
            claims: record.claims,
            bonds: record.bonds,
        })
    }

    /// Updates the claim-awareness switch for one registered context.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the claim context does not exist.
    pub fn set_context_awareness(
        &mut self,
        context: ClaimContextId,
        awareness: ClaimAwareness,
    ) -> Result<(), ClaimsError> {
        let record = self
            .find_context_mut(context)
            .ok_or_else(ClaimsError::not_found)?;
        record.descriptor.awareness = awareness;
        Ok(())
    }

    /// Rebinds one registered context to a new local seal, resetting grants when the seal changes.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the claim context does not exist.
    pub fn reset_context_seal(
        &mut self,
        context: ClaimContextId,
        image_seal: LocalAdmissionSeal,
    ) -> Result<(), ClaimsError> {
        let record = self
            .find_context_mut(context)
            .ok_or_else(ClaimsError::not_found)?;
        record.descriptor.image_seal = image_seal;
        record.claims = [None; MAX_CLAIMS];
        record.bonds = [None; MAX_BONDS];
        record.claim_trie.clear();
        Ok(())
    }

    /// Grants one claim to one registered context.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the context does not exist, the claim principal does not
    /// match, the claim already exists, or storage is exhausted.
    pub fn grant_claim(
        &mut self,
        context: ClaimContextId,
        mut grant: ClaimGrant<'a>,
    ) -> Result<(), ClaimsError> {
        let record = self
            .find_context_mut(context)
            .ok_or_else(ClaimsError::not_found)?;
        if grant.qualified.principal() != record.descriptor.principal {
            return Err(ClaimsError::invalid());
        }
        grant.seal = record.descriptor.image_seal;
        if let Some(existing_index) = record.claim_trie.exact_claim_index(grant.qualified.claim()) {
            let Some(existing) = record
                .claims
                .get_mut(existing_index)
                .and_then(Option::as_mut)
            else {
                return Err(ClaimsError::state_conflict());
            };
            if existing.qualified != grant.qualified {
                return Err(ClaimsError::state_conflict());
            }
            if matches!(
                existing.state,
                ClaimGrantState::Granted | ClaimGrantState::Pending
            ) {
                return Err(ClaimsError::state_conflict());
            }
            *existing = grant;
        } else {
            let Some(slot_index) = record.claims.iter().position(|slot| slot.is_none()) else {
                return Err(ClaimsError::resource_exhausted());
            };
            record
                .claim_trie
                .insert(grant.qualified.claim(), slot_index)?;
            record.claims[slot_index] = Some(grant);
        }
        let granted_claim_count = record.claims.iter().flatten().count() as u32;
        record.descriptor.image_seal = record
            .descriptor
            .image_seal
            .with_granted_claim_count(granted_claim_count);
        for claim in record.claims.iter_mut().flatten() {
            claim.seal = record.descriptor.image_seal;
        }
        Ok(())
    }

    /// Revokes one granted claim inside one claim context.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the context or claim does not exist.
    pub fn revoke_claim(
        &mut self,
        context: ClaimContextId,
        qualified: QualifiedClaimId<'a>,
    ) -> Result<(), ClaimsError> {
        let record = self
            .find_context_mut(context)
            .ok_or_else(ClaimsError::not_found)?;
        let Some(entry) = record
            .claims
            .iter_mut()
            .flatten()
            .find(|grant| grant.qualified == qualified)
        else {
            return Err(ClaimsError::not_found());
        };
        entry.state = ClaimGrantState::Revoked;
        Ok(())
    }

    /// Expires stale lease-bound claims across every registered context.
    pub fn expire_stale_claims(&mut self, now_unix_seconds: u64) {
        for record in self.contexts.iter_mut().flatten() {
            for grant in record.claims.iter_mut().flatten() {
                if matches!(grant.state, ClaimGrantState::Granted)
                    && grant
                        .expires_at_unix_seconds
                        .is_some_and(|expires| expires <= now_unix_seconds)
                {
                    grant.state = ClaimGrantState::Expired;
                }
            }
        }
    }

    /// Authorizes one claim use through one courier's claim-enabled mediation path.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier is claim-blind, points at the wrong claim
    /// context, or the claim is not active.
    pub fn request_claim_for_courier(
        &mut self,
        courier: CourierSupport,
        qualified: QualifiedClaimId<'a>,
        now_unix_seconds: u64,
    ) -> Result<ClaimGrant<'a>, ClaimsError> {
        if courier.claim_awareness.is_blind() {
            return Err(ClaimsError::permission_denied());
        }
        let Some(context) = courier.claim_context else {
            return Err(ClaimsError::permission_denied());
        };
        self.request_claim_for_context(context, qualified, now_unix_seconds)
    }

    /// Authorizes one claim use for one black fiber mediated by its owning courier.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the fiber or courier claim mode is not black, the claim
    /// context IDs disagree, or the underlying claim is not active.
    pub fn request_claim_for_fiber(
        &mut self,
        courier: CourierSupport,
        fiber_awareness: ClaimAwareness,
        fiber_claim_context: Option<ClaimContextId>,
        qualified: QualifiedClaimId<'a>,
        now_unix_seconds: u64,
    ) -> Result<ClaimGrant<'a>, ClaimsError> {
        if !fiber_awareness.is_black() {
            return Err(ClaimsError::permission_denied());
        }
        if courier.claim_awareness.is_blind() {
            return Err(ClaimsError::permission_denied());
        }
        if courier.claim_context != fiber_claim_context {
            return Err(ClaimsError::permission_denied());
        }
        let Some(context) = fiber_claim_context else {
            return Err(ClaimsError::permission_denied());
        };
        self.request_claim_for_context(context, qualified, now_unix_seconds)
    }

    /// Issues one live bilateral attachment bond between two claim contexts.
    ///
    /// # Errors
    ///
    /// Returns an honest error when either context does not exist, the attachment law is invalid
    /// for the current request, or per-context bond storage is exhausted.
    pub fn issue_attachment_bond(
        &mut self,
        bond: AttachmentBondId,
        provider_context: ClaimContextId,
        consumer_context: ClaimContextId,
        channel: ClaimName<'a>,
        law: TransportAttachmentLaw,
        issued_at_unix_seconds: u64,
        expires_at_unix_seconds: Option<u64>,
        revocation_epoch: u64,
    ) -> Result<AttachmentBond<'a>, ClaimsError> {
        let provider = self
            .find_context(provider_context)
            .ok_or_else(ClaimsError::not_found)?
            .descriptor;
        let consumer = self
            .find_context(consumer_context)
            .ok_or_else(ClaimsError::not_found)?
            .descriptor;

        let issued = AttachmentBond {
            id: bond,
            boot_epoch: provider.image_seal.boot_epoch,
            provider: AttachmentBondHalf {
                bond,
                principal: provider.principal,
                peer: consumer.principal,
                peer_seal: consumer.image_seal.id,
                channel,
                law,
            },
            consumer: AttachmentBondHalf {
                bond,
                principal: consumer.principal,
                peer: provider.principal,
                peer_seal: provider.image_seal.id,
                channel,
                law,
            },
            issued_at_unix_seconds,
            expires_at_unix_seconds,
            revocation_epoch,
        };

        self.attach_bond(provider_context, issued)?;
        if let Err(error) = self.attach_bond(consumer_context, issued) {
            let _ = self.detach_bond(provider_context, issued.id);
            return Err(error);
        }

        Ok(issued)
    }

    /// Returns active possessed claims matching one qualified pattern.
    ///
    /// Exact and prefix claim queries route through the per-context trie first. More complex
    /// wildcard shapes still fall back to filtered traversal until the broader principal/reverse
    /// indexes land.
    #[must_use]
    pub fn search_active_claims<const MAX_MATCHES: usize>(
        &self,
        pattern: QualifiedClaimPattern<'a>,
        now_unix_seconds: u64,
    ) -> ClaimSearchResults<'a, MAX_MATCHES> {
        let mut matches = [None; MAX_MATCHES];
        let mut total_matches = 0usize;
        let claim_query = ClaimPatternQuery::classify(pattern.claim());

        for record in self.contexts.iter().flatten() {
            if !pattern
                .principal()
                .matches_principal(record.descriptor.principal)
            {
                continue;
            }

            match claim_query {
                ClaimPatternQuery::Exact(claim_name) => {
                    let Some(claim_index) = record.claim_trie.exact_claim_index(claim_name) else {
                        continue;
                    };
                    let Some(grant) = record.claims.get(claim_index).and_then(|slot| *slot) else {
                        continue;
                    };
                    if grant.is_active(now_unix_seconds) {
                        if total_matches < MAX_MATCHES {
                            matches[total_matches] = Some(ActiveClaimMatch {
                                context: record.descriptor.id,
                                principal: record.descriptor.principal,
                                qualified: grant.qualified,
                                seal: record.descriptor.image_seal.id,
                            });
                        }
                        total_matches += 1;
                    }
                }
                ClaimPatternQuery::Prefix(prefix) => {
                    let mut claim_indices = [None; MAX_MATCHES];
                    let mut claim_matches = 0usize;
                    record.claim_trie.collect_prefix_claim_indices(
                        prefix,
                        &mut claim_indices,
                        &mut claim_matches,
                    );
                    for claim_index in claim_indices.iter().flatten() {
                        let Some(grant) = record.claims.get(*claim_index).and_then(|slot| *slot)
                        else {
                            continue;
                        };
                        if grant.is_active(now_unix_seconds)
                            && pattern.matches_qualified(grant.qualified)
                        {
                            if total_matches < MAX_MATCHES {
                                matches[total_matches] = Some(ActiveClaimMatch {
                                    context: record.descriptor.id,
                                    principal: record.descriptor.principal,
                                    qualified: grant.qualified,
                                    seal: record.descriptor.image_seal.id,
                                });
                            }
                            total_matches += 1;
                        }
                    }
                }
                ClaimPatternQuery::All | ClaimPatternQuery::Complex => {
                    for grant in record.claims.iter().flatten() {
                        if grant.is_active(now_unix_seconds)
                            && pattern.matches_qualified(grant.qualified)
                        {
                            if total_matches < MAX_MATCHES {
                                matches[total_matches] = Some(ActiveClaimMatch {
                                    context: record.descriptor.id,
                                    principal: record.descriptor.principal,
                                    qualified: grant.qualified,
                                    seal: record.descriptor.image_seal.id,
                                });
                            }
                            total_matches += 1;
                        }
                    }
                }
            }
        }

        ClaimSearchResults {
            matches,
            total_matches,
        }
    }

    fn request_claim_for_context(
        &mut self,
        context: ClaimContextId,
        qualified: QualifiedClaimId<'a>,
        now_unix_seconds: u64,
    ) -> Result<ClaimGrant<'a>, ClaimsError> {
        let record = self
            .find_context_mut(context)
            .ok_or_else(ClaimsError::not_found)?;
        if record.descriptor.awareness.is_blind() {
            return Err(ClaimsError::permission_denied());
        }
        let Some(claim_index) = record.claim_trie.exact_claim_index(qualified.claim()) else {
            return Err(ClaimsError::not_found());
        };
        let Some(grant) = record.claims.get_mut(claim_index).and_then(Option::as_mut) else {
            return Err(ClaimsError::not_found());
        };
        if grant.qualified != qualified {
            return Err(ClaimsError::not_found());
        }
        let granted = *grant;
        match grant.state {
            ClaimGrantState::Revoked => Err(ClaimsError::revoked()),
            ClaimGrantState::Expired => Err(ClaimsError::expired()),
            ClaimGrantState::Pending | ClaimGrantState::Consumed => {
                Err(ClaimsError::permission_denied())
            }
            ClaimGrantState::Granted => {
                if granted.is_active(now_unix_seconds) {
                    if matches!(grant.lifetime, ClaimGrantLifetime::OneShot) {
                        grant.state = ClaimGrantState::Consumed;
                    }
                    Ok(granted)
                } else {
                    grant.state = ClaimGrantState::Expired;
                    Err(ClaimsError::expired())
                }
            }
        }
    }

    // Claims are indexed by their dotted scope inside the context-local trie so exact scope checks
    // can stay cheap even before the larger principal/bond datastore replacement lands.
    fn has_active_claim_name(
        &self,
        context: ClaimContextId,
        claim: ClaimName<'a>,
        now_unix_seconds: u64,
    ) -> Result<bool, ClaimsError> {
        let record = self
            .find_context(context)
            .ok_or_else(ClaimsError::not_found)?;
        let Some(claim_index) = record.claim_trie.exact_claim_index(claim) else {
            return Ok(false);
        };
        let Some(grant) = record.claims.get(claim_index).and_then(|slot| *slot) else {
            return Ok(false);
        };
        Ok(grant.is_active(now_unix_seconds))
    }

    fn attach_bond(
        &mut self,
        context: ClaimContextId,
        bond: AttachmentBond<'a>,
    ) -> Result<(), ClaimsError> {
        let record = self
            .find_context_mut(context)
            .ok_or_else(ClaimsError::not_found)?;
        if record
            .bonds
            .iter()
            .flatten()
            .any(|existing| existing.id == bond.id)
        {
            return Err(ClaimsError::state_conflict());
        }
        let Some(slot) = record.bonds.iter_mut().find(|slot| slot.is_none()) else {
            return Err(ClaimsError::resource_exhausted());
        };
        *slot = Some(bond);
        Ok(())
    }

    fn detach_bond(
        &mut self,
        context: ClaimContextId,
        bond: AttachmentBondId,
    ) -> Result<(), ClaimsError> {
        let record = self
            .find_context_mut(context)
            .ok_or_else(ClaimsError::not_found)?;
        let Some(slot) = record
            .bonds
            .iter_mut()
            .find(|slot| slot.is_some_and(|existing| existing.id == bond))
        else {
            return Err(ClaimsError::not_found());
        };
        *slot = None;
        Ok(())
    }

    fn find_context(
        &self,
        context: ClaimContextId,
    ) -> Option<&ClaimContextRecord<'a, MAX_CLAIMS, MAX_BONDS, MAX_SCOPE_NODES>> {
        self.contexts
            .iter()
            .flatten()
            .find(|record| record.descriptor.id == context)
    }

    fn find_context_mut(
        &mut self,
        context: ClaimContextId,
    ) -> Option<&mut ClaimContextRecord<'a, MAX_CLAIMS, MAX_BONDS, MAX_SCOPE_NODES>> {
        self.contexts
            .iter_mut()
            .flatten()
            .find(|record| record.descriptor.id == context)
    }

    fn find_context_index(&self, context: ClaimContextId) -> Option<usize> {
        self.contexts
            .iter()
            .position(|slot| slot.is_some_and(|record| record.descriptor.id == context))
    }

    fn find_principal(
        &self,
        principal: PrincipalId<'a>,
    ) -> Option<&ClaimContextRecord<'a, MAX_CLAIMS, MAX_BONDS, MAX_SCOPE_NODES>> {
        self.contexts
            .iter()
            .flatten()
            .find(|record| record.descriptor.principal == principal)
    }

    fn find_principal_index(&self, principal: PrincipalId<'a>) -> Option<usize> {
        self.contexts
            .iter()
            .position(|slot| slot.is_some_and(|record| record.descriptor.principal == principal))
    }
}

impl<
    'a,
    const MAX_CONTEXTS: usize,
    const MAX_CLAIMS: usize,
    const MAX_BONDS: usize,
    const MAX_SCOPE_NODES: usize,
> Default for ClaimContextRegistry<'a, MAX_CONTEXTS, MAX_CLAIMS, MAX_BONDS, MAX_SCOPE_NODES>
{
    fn default() -> Self {
        Self::new()
    }
}

/// One courier authority node with one claim context and an optional parent courier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CourierAuthorityDescriptor<'a> {
    pub courier: CourierId,
    pub principal: PrincipalId<'a>,
    pub parent: Option<CourierId>,
    pub awareness: ClaimAwareness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CourierAuthorityRecord<'a> {
    descriptor: CourierAuthorityDescriptor<'a>,
    claim_context: ClaimContextId,
}

/// Courier-rooted authority registry for claims, seals, and attachment bonds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CourierAuthorityRegistry<
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_CLAIMS: usize,
    const MAX_BONDS: usize,
    const MAX_SCOPE_NODES: usize,
> {
    boot_epoch: u64,
    current_revocation_epoch: u64,
    next_context: u64,
    next_seal: u64,
    next_bond: u64,
    couriers: [Option<CourierAuthorityRecord<'a>>; MAX_COURIERS],
    contexts: ClaimContextRegistry<'a, MAX_CONTEXTS, MAX_CLAIMS, MAX_BONDS, MAX_SCOPE_NODES>,
}

impl<
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_CLAIMS: usize,
    const MAX_BONDS: usize,
    const MAX_SCOPE_NODES: usize,
> CourierAuthorityRegistry<'a, MAX_COURIERS, MAX_CONTEXTS, MAX_CLAIMS, MAX_BONDS, MAX_SCOPE_NODES>
{
    /// Creates one empty courier-rooted authority registry.
    #[must_use]
    pub const fn new(boot_epoch: u64) -> Self {
        Self {
            boot_epoch,
            current_revocation_epoch: 1,
            next_context: 1,
            next_seal: 1,
            next_bond: 1,
            couriers: [None; MAX_COURIERS],
            contexts: ClaimContextRegistry::new(),
        }
    }

    /// Registers one root courier with one admitted local seal and claim context.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier already exists or storage is exhausted.
    pub fn register_root_courier(
        &mut self,
        descriptor: CourierAuthorityDescriptor<'a>,
        image_digest: ClaimsDigest,
        claims_digest: ClaimsDigest,
        remote_claims_digest: ClaimsDigest,
    ) -> Result<ClaimContextId, ClaimsError> {
        if descriptor.parent.is_some() {
            return Err(ClaimsError::invalid());
        }
        self.register_courier(
            descriptor,
            image_digest,
            claims_digest,
            remote_claims_digest,
        )
    }

    /// Registers one child courier under one existing parent courier.
    ///
    /// Child couriers get their own claim context and seal identity, but they do not get to outrun
    /// the authority chain above them; actual claim use and grant still has to fit under the parent
    /// chain carried by the issuing authority root.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the parent does not exist, the descriptor parent mismatches,
    /// or storage is exhausted.
    pub fn register_child_courier(
        &mut self,
        parent: CourierId,
        descriptor: CourierAuthorityDescriptor<'a>,
        image_digest: ClaimsDigest,
        claims_digest: ClaimsDigest,
        remote_claims_digest: ClaimsDigest,
    ) -> Result<ClaimContextId, ClaimsError> {
        if descriptor.parent != Some(parent) || self.find_courier(parent).is_none() {
            return Err(ClaimsError::invalid());
        }
        self.register_courier(
            descriptor,
            image_digest,
            claims_digest,
            remote_claims_digest,
        )
    }

    /// Revalidates one courier admission seal against the currently observed digests.
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
        let context = self
            .find_courier(courier)
            .ok_or_else(ClaimsError::not_found)?
            .claim_context;
        let snapshot = self.contexts.snapshot(context)?;
        let reason = if snapshot.descriptor.image_seal.image_digest != image_digest {
            Some(SealMismatchReason::ImageDigestChanged)
        } else if snapshot.descriptor.image_seal.claims_digest != claims_digest {
            Some(SealMismatchReason::ClaimsDigestChanged)
        } else if snapshot.descriptor.image_seal.remote_claims_digest != remote_claims_digest {
            Some(SealMismatchReason::RemoteClaimsDigestChanged)
        } else {
            None
        };
        if reason.is_some() {
            let seal = self.next_seal(image_digest, claims_digest, remote_claims_digest);
            self.contexts.reset_context_seal(context, seal)?;
        }
        Ok(reason)
    }

    /// Grants one claim to the supplied courier authority.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier is unknown or the grant cannot be stored.
    pub fn grant_claim(
        &mut self,
        courier: CourierId,
        grant: ClaimGrant<'a>,
    ) -> Result<(), ClaimsError> {
        let record = *self
            .find_courier(courier)
            .ok_or_else(ClaimsError::not_found)?;
        // Child couriers do not mint authority out of thin air; a parent chain must already carry
        // the same claim scope before the child can hold it locally.
        self.authorize_parent_chain(
            record.descriptor.parent,
            grant.qualified.claim(),
            grant.issued_at_unix_seconds,
        )?;
        self.contexts.grant_claim(record.claim_context, grant)
    }

    /// Revokes one claim previously granted to one courier.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier or claim is unknown.
    pub fn revoke_claim(
        &mut self,
        courier: CourierId,
        qualified: QualifiedClaimId<'a>,
    ) -> Result<(), ClaimsError> {
        let context = self
            .find_courier(courier)
            .ok_or_else(ClaimsError::not_found)?
            .claim_context;
        self.contexts.revoke_claim(context, qualified)
    }

    /// Authorizes one claim use through one courier authority.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier is unknown or the claim is not active.
    pub fn request_claim_for_courier(
        &mut self,
        courier: CourierId,
        qualified: QualifiedClaimId<'a>,
        now_unix_seconds: u64,
    ) -> Result<ClaimGrant<'a>, ClaimsError> {
        let record = *self
            .find_courier(courier)
            .ok_or_else(ClaimsError::not_found)?;
        self.authorize_parent_chain(
            record.descriptor.parent,
            qualified.claim(),
            now_unix_seconds,
        )?;
        self.contexts
            .request_claim_for_context(record.claim_context, qualified, now_unix_seconds)
    }

    /// Authorizes one claim use for one fiber mediated by its owning courier.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier is unknown, the fiber is not black, or the
    /// supplied fiber claim context does not match the owning courier.
    pub fn request_claim_for_fiber(
        &mut self,
        courier: CourierId,
        fiber_awareness: ClaimAwareness,
        fiber_claim_context: Option<ClaimContextId>,
        qualified: QualifiedClaimId<'a>,
        now_unix_seconds: u64,
    ) -> Result<ClaimGrant<'a>, ClaimsError> {
        let record = *self
            .find_courier(courier)
            .ok_or_else(ClaimsError::not_found)?;
        if !fiber_awareness.is_black() || fiber_claim_context != Some(record.claim_context) {
            return Err(ClaimsError::permission_denied());
        }
        self.authorize_parent_chain(
            record.descriptor.parent,
            qualified.claim(),
            now_unix_seconds,
        )?;
        self.contexts
            .request_claim_for_context(record.claim_context, qualified, now_unix_seconds)
    }

    /// Issues one authority-attested bond between two courier boundaries.
    ///
    /// # Errors
    ///
    /// Returns an honest error when either courier is unknown or bond storage is exhausted.
    pub fn issue_attachment_bond(
        &mut self,
        provider: CourierId,
        consumer: CourierId,
        channel: ClaimName<'a>,
        law: TransportAttachmentLaw,
        issued_at_unix_seconds: u64,
        expires_at_unix_seconds: Option<u64>,
    ) -> Result<AttachmentBond<'a>, ClaimsError> {
        let provider_context = self
            .find_courier(provider)
            .ok_or_else(ClaimsError::not_found)?
            .claim_context;
        let consumer_context = self
            .find_courier(consumer)
            .ok_or_else(ClaimsError::not_found)?
            .claim_context;
        let bond = AttachmentBondId::new(self.next_bond);
        self.next_bond += 1;
        self.contexts.issue_attachment_bond(
            bond,
            provider_context,
            consumer_context,
            channel,
            law,
            issued_at_unix_seconds,
            expires_at_unix_seconds,
            self.current_revocation_epoch,
        )
    }

    /// Returns the current authority-wide bond revocation epoch.
    #[must_use]
    pub const fn current_revocation_epoch(&self) -> u64 {
        self.current_revocation_epoch
    }

    /// Bumps the bond revocation epoch, invalidating outstanding bonds from older epochs.
    #[must_use]
    pub fn bump_revocation_epoch(&mut self) -> u64 {
        self.current_revocation_epoch += 1;
        self.current_revocation_epoch
    }

    /// Expires stale claims through the composed claim-context registry.
    pub fn expire_stale_claims(&mut self, now_unix_seconds: u64) {
        self.contexts.expire_stale_claims(now_unix_seconds);
    }

    /// Searches currently active possessed claims matching one pattern.
    #[must_use]
    pub fn search_active_claims<const MAX_MATCHES: usize>(
        &self,
        pattern: QualifiedClaimPattern<'a>,
        now_unix_seconds: u64,
    ) -> ClaimSearchResults<'a, MAX_MATCHES> {
        self.contexts
            .search_active_claims::<MAX_MATCHES>(pattern, now_unix_seconds)
    }

    /// Returns one current claim-context snapshot for the supplied principal.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the principal is unknown.
    pub fn inspect_principal(
        &self,
        principal: PrincipalId<'a>,
    ) -> Result<ClaimContextSnapshot<'a, MAX_CLAIMS, MAX_BONDS>, ClaimsError> {
        self.contexts.snapshot_by_principal(principal)
    }

    /// Returns one current claim-context snapshot for the supplied context.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the context is unknown.
    pub fn inspect_context(
        &self,
        context: ClaimContextId,
    ) -> Result<ClaimContextSnapshot<'a, MAX_CLAIMS, MAX_BONDS>, ClaimsError> {
        self.contexts.snapshot(context)
    }

    /// Returns one shared view of the underlying claims registry.
    #[must_use]
    pub const fn contexts(
        &self,
    ) -> &ClaimContextRegistry<'a, MAX_CONTEXTS, MAX_CLAIMS, MAX_BONDS, MAX_SCOPE_NODES> {
        &self.contexts
    }

    fn register_courier(
        &mut self,
        descriptor: CourierAuthorityDescriptor<'a>,
        image_digest: ClaimsDigest,
        claims_digest: ClaimsDigest,
        remote_claims_digest: ClaimsDigest,
    ) -> Result<ClaimContextId, ClaimsError> {
        if self.find_courier(descriptor.courier).is_some() {
            return Err(ClaimsError::state_conflict());
        }
        let seal = self.next_seal(image_digest, claims_digest, remote_claims_digest);
        let Some(slot) = self.couriers.iter_mut().find(|slot| slot.is_none()) else {
            return Err(ClaimsError::resource_exhausted());
        };
        let claim_context = ClaimContextId::new(self.next_context);
        self.next_context += 1;
        self.contexts.register_context(ClaimContextDescriptor {
            id: claim_context,
            principal: descriptor.principal,
            image_seal: seal,
            awareness: descriptor.awareness,
        })?;
        *slot = Some(CourierAuthorityRecord {
            descriptor,
            claim_context,
        });
        Ok(claim_context)
    }

    fn next_seal(
        &mut self,
        image_digest: ClaimsDigest,
        claims_digest: ClaimsDigest,
        remote_claims_digest: ClaimsDigest,
    ) -> LocalAdmissionSeal {
        let seal = LocalAdmissionSeal::new(
            ImageSealId::new(self.next_seal),
            image_digest,
            claims_digest,
            remote_claims_digest,
            self.boot_epoch,
        );
        self.next_seal += 1;
        seal
    }

    fn find_courier(&self, courier: CourierId) -> Option<&CourierAuthorityRecord<'a>> {
        self.couriers
            .iter()
            .flatten()
            .find(|record| record.descriptor.courier == courier)
    }

    // Parent couriers are the authority root for child couriers beneath them, so every mediated
    // child claim use/grant must still be covered by an active ancestor scope in the chain.
    fn authorize_parent_chain(
        &self,
        mut parent: Option<CourierId>,
        claim: ClaimName<'a>,
        now_unix_seconds: u64,
    ) -> Result<(), ClaimsError> {
        while let Some(parent_courier) = parent {
            let record = self
                .find_courier(parent_courier)
                .ok_or_else(ClaimsError::not_found)?;
            if !self.contexts.has_active_claim_name(
                record.claim_context,
                claim,
                now_unix_seconds,
            )? {
                return Err(ClaimsError::permission_denied());
            }
            parent = record.descriptor.parent;
        }
        Ok(())
    }
}

#[cfg(all(test, feature = "std", not(target_os = "none")))]
mod tests {
    use super::*;

    use crate::courier::{CourierCaps, CourierImplementationKind, CourierVisibility};
    use crate::domain::DomainId;

    type DemoRegistry<'a> = ClaimContextRegistry<'a, 4, 4, 4, 16>;
    type DemoAuthority<'a> = CourierAuthorityRegistry<'a, 4, 4, 4, 4, 16>;

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

        let search = registry
            .search_active_claims::<4>(QualifiedClaimPattern::parse("*=>net.*").unwrap(), 150);
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
                    qualified: QualifiedClaimId::parse("firewall@net[kernel]=>net.tcp.443")
                        .unwrap(),
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
}
