use core::array;

use super::member::{
    MemoryPoolContributor,
    MemoryPoolMember,
};
use super::{
    MemoryPool,
    MemoryPoolError,
    MemoryPoolMemberId,
    MemoryPoolPolicy,
    MemoryPoolProvisioningPolicy,
};

/// Builder for a fixed-capacity `MemoryPool`.
#[derive(Debug)]
pub struct MemoryPoolBuilder<const MEMBERS: usize, const EXTENTS: usize> {
    policy: MemoryPoolPolicy,
    contributors: [Option<MemoryPoolContributor>; MEMBERS],
    contributor_count: usize,
}

impl<const MEMBERS: usize, const EXTENTS: usize> MemoryPoolBuilder<MEMBERS, EXTENTS> {
    /// Creates a new builder with the supplied pool policy.
    #[must_use]
    pub fn new(policy: MemoryPoolPolicy) -> Self {
        Self {
            policy,
            contributors: array::from_fn(|_| None),
            contributor_count: 0,
        }
    }

    /// Adds one realized contributor to the pool build.
    ///
    /// # Errors
    ///
    /// Returns an error when the builder has no remaining contributor capacity.
    pub fn add_contributor(
        &mut self,
        contributor: MemoryPoolContributor,
    ) -> Result<(), MemoryPoolError> {
        if self.contributor_count >= MEMBERS {
            return Err(MemoryPoolError::metadata_exhausted());
        }

        self.contributors[self.contributor_count] = Some(contributor);
        self.contributor_count += 1;
        Ok(())
    }

    /// Builds the pool from the currently staged contributors.
    ///
    /// # Errors
    ///
    /// Returns an error when the staged contributors are empty, incompatible, not ready for
    /// the selected provisioning policy, or exceed fixed pool metadata capacity.
    pub fn build(self) -> Result<MemoryPool<MEMBERS, EXTENTS>, MemoryPoolError> {
        MemoryPool::from_builder(self)
    }

    pub(super) const fn policy(&self) -> MemoryPoolPolicy {
        self.policy
    }

    pub(super) const fn contributor_count(&self) -> usize {
        self.contributor_count
    }

    pub(super) fn into_members(
        self,
    ) -> Result<[Option<MemoryPoolMember>; MEMBERS], MemoryPoolError> {
        let mut members = array::from_fn(|_| None);

        for (index, contributor) in self.contributors.into_iter().enumerate() {
            let Some(contributor) = contributor else {
                continue;
            };

            if !matches!(
                self.policy.provisioning,
                MemoryPoolProvisioningPolicy::ReadyOnly
            ) {
                return Err(MemoryPoolError::unsupported_policy());
            }

            if !contributor.readiness.is_ready_now() {
                return Err(MemoryPoolError::unsupported_policy());
            }

            let member_id = MemoryPoolMemberId(
                u32::try_from(index).map_err(|_| MemoryPoolError::metadata_exhausted())?,
            );
            members[index] = Some(MemoryPoolMember::from_contributor(member_id, contributor)?);
        }

        Ok(members)
    }
}
