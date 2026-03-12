use crate::mem::provider::{
    MemoryCompatibilityEnvelope, MemoryPoolClassId, MemoryResourceDescriptor, MemoryResourceId,
    MemoryResourceReadiness, MemoryStrategyId, MemoryTopologyNodeId,
};
use crate::mem::resource::{MemoryResource, MemoryResourceHandle, ResourceRange};

use super::MemoryPoolError;

/// Stable identifier for a member resource owned by a `MemoryPool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryPoolMemberId(pub u32);

/// Provenance of a contributor admitted to a `MemoryPool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryPoolContributorOrigin {
    /// Contributor was supplied directly by the caller.
    Explicit,
    /// Contributor corresponds to a provider-known present resource.
    PresentResource(MemoryResourceId),
    /// Contributor was created or materialized through a provider strategy.
    CreatedResource(MemoryStrategyId),
}

/// Explicit realized contributor admitted to a `MemoryPool`.
#[derive(Debug)]
pub struct MemoryPoolContributor {
    /// Owned realized resource.
    pub handle: MemoryResourceHandle,
    /// Pool-usable range inside the owned resource.
    pub usable_range: ResourceRange,
    /// Current readiness classification for pool use.
    pub readiness: MemoryResourceReadiness,
    /// Optional provider-authored topology node.
    pub topology_node: Option<MemoryTopologyNodeId>,
    /// Optional provider-authored pool class.
    pub pool_class: Option<MemoryPoolClassId>,
    /// Provenance of the contributor.
    pub origin: MemoryPoolContributorOrigin,
}

impl MemoryPoolContributor {
    /// Creates an explicit ready contributor spanning the whole resource.
    #[must_use]
    pub fn explicit_ready(handle: MemoryResourceHandle) -> Self {
        let len = handle.len();
        Self {
            handle,
            usable_range: ResourceRange::whole(len),
            readiness: MemoryResourceReadiness::ReadyNow,
            topology_node: None,
            pool_class: None,
            origin: MemoryPoolContributorOrigin::Explicit,
        }
    }

    /// Creates a contributor that mirrors a provider-known present resource record.
    ///
    /// # Errors
    ///
    /// Returns an error when the supplied handle does not match the descriptor or when
    /// `usable_range` falls outside the handle's governed range.
    pub fn from_resource_descriptor(
        handle: MemoryResourceHandle,
        descriptor: &MemoryResourceDescriptor,
        usable_range: ResourceRange,
    ) -> Result<Self, MemoryPoolError> {
        if handle.resolved().info != descriptor.info {
            return Err(MemoryPoolError::incompatible_contributor());
        }

        handle
            .subview(usable_range)
            .map_err(|error| MemoryPoolError::resource(error.kind))?;

        if usable_range.len > descriptor.usable_max_len {
            return Err(MemoryPoolError::incompatible_contributor());
        }

        Ok(Self {
            handle,
            usable_range,
            readiness: descriptor.readiness,
            topology_node: descriptor.topology_node,
            pool_class: descriptor.pool_class,
            origin: MemoryPoolContributorOrigin::PresentResource(descriptor.id),
        })
    }

    /// Returns the pool-visible compatibility envelope of this contributor.
    #[must_use]
    pub const fn compatibility(&self) -> MemoryCompatibilityEnvelope {
        MemoryCompatibilityEnvelope::from_resource_info(self.handle.resolved().info)
    }
}

#[derive(Debug)]
pub(super) struct MemoryPoolMember {
    pub id: MemoryPoolMemberId,
    pub handle: MemoryResourceHandle,
    pub usable_range: ResourceRange,
    pub compatibility: MemoryCompatibilityEnvelope,
    pub topology_node: Option<MemoryTopologyNodeId>,
    pub pool_class: Option<MemoryPoolClassId>,
    pub origin: MemoryPoolContributorOrigin,
}

impl MemoryPoolMember {
    pub(super) fn from_contributor(
        id: MemoryPoolMemberId,
        contributor: MemoryPoolContributor,
    ) -> Result<Self, MemoryPoolError> {
        contributor
            .handle
            .subview(contributor.usable_range)
            .map_err(|error| MemoryPoolError::resource(error.kind))?;

        Ok(Self {
            id,
            compatibility: contributor.compatibility(),
            topology_node: contributor.topology_node,
            pool_class: contributor.pool_class,
            origin: contributor.origin,
            usable_range: contributor.usable_range,
            handle: contributor.handle,
        })
    }
}

/// Public summary of one member inside a `MemoryPool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryPoolMemberInfo {
    /// Stable member identifier.
    pub id: MemoryPoolMemberId,
    /// Provenance of the member.
    pub origin: MemoryPoolContributorOrigin,
    /// Pool-usable range inside the member resource.
    pub usable_range: ResourceRange,
    /// Canonical compatibility envelope shared across the pool.
    pub compatibility: MemoryCompatibilityEnvelope,
    /// Optional provider-authored topology node.
    pub topology_node: Option<MemoryTopologyNodeId>,
    /// Optional provider-authored pool class.
    pub pool_class: Option<MemoryPoolClassId>,
    /// Currently free bytes in this member.
    pub free_bytes: usize,
    /// Currently leased bytes in this member.
    pub leased_bytes: usize,
}

impl MemoryPoolMemberInfo {
    pub(super) const fn from_member(member: &MemoryPoolMember, stats: MemberUsageStats) -> Self {
        Self {
            id: member.id,
            origin: member.origin,
            usable_range: member.usable_range,
            compatibility: member.compatibility,
            topology_node: member.topology_node,
            pool_class: member.pool_class,
            free_bytes: stats.free_bytes,
            leased_bytes: stats.leased_bytes,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct MemberUsageStats {
    pub free_bytes: usize,
    pub leased_bytes: usize,
}

impl MemberUsageStats {
    pub(super) const fn new(free_bytes: usize, leased_bytes: usize) -> Self {
        Self {
            free_bytes,
            leased_bytes,
        }
    }
}
