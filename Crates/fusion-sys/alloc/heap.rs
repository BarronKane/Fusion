use core::fmt;

use super::{
    AllocCapabilities,
    AllocError,
    AllocHazards,
    AllocModeSet,
    AllocPolicy,
    AllocRequest,
    AllocResult,
    AllocationStrategy,
    AllocatorDomainId,
    PoolHandle,
};

/// General-purpose allocator surface for non-critical-safe heap behavior.
pub struct HeapAllocator {
    domain: AllocatorDomainId,
    policy: AllocPolicy,
    pool: PoolHandle,
}

impl fmt::Debug for HeapAllocator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeapAllocator")
            .field("domain", &self.domain)
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl HeapAllocator {
    pub(super) fn for_domain(
        domain: AllocatorDomainId,
        policy: AllocPolicy,
        pool: Option<&PoolHandle>,
    ) -> Result<Self, AllocError> {
        if !policy.allows(AllocModeSet::HEAP) {
            return Err(AllocError::policy_denied());
        }
        let pool = pool
            .ok_or_else(AllocError::capacity_exhausted)?
            .try_clone()?;
        Ok(Self {
            domain,
            policy,
            pool,
        })
    }

    /// Returns the capability surface currently available for the heap path.
    ///
    /// Heap remains intentionally unimplemented on the active allocator model, so this surface
    /// stays empty until the backing implementation lands.
    #[must_use]
    pub const fn supported_capabilities(policy: AllocPolicy) -> AllocCapabilities {
        let _ = policy;
        AllocCapabilities::empty()
    }

    /// Returns the expected coarse heap hazards.
    ///
    /// Heap remains stubbed, so this surface also stays empty rather than advertising hazards for
    /// behavior that does not yet exist.
    #[must_use]
    pub const fn expected_hazards() -> AllocHazards {
        AllocHazards::empty()
    }

    /// Returns the heap policy.
    #[must_use]
    pub const fn policy(&self) -> AllocPolicy {
        self.policy
    }

    /// Returns the owning allocator domain.
    #[must_use]
    pub const fn domain(&self) -> AllocatorDomainId {
        self.domain
    }
}

impl AllocationStrategy for HeapAllocator {
    fn policy(&self) -> AllocPolicy {
        self.policy
    }

    fn capabilities(&self) -> AllocCapabilities {
        Self::supported_capabilities(self.policy)
    }

    fn hazards(&self) -> AllocHazards {
        Self::expected_hazards()
    }

    fn allocate(&self, request: &AllocRequest) -> Result<AllocResult, AllocError> {
        if request.len == 0 || request.align == 0 || !request.align.is_power_of_two() {
            return Err(AllocError::invalid_request());
        }
        let _ = self.pool;
        Err(AllocError::unsupported())
    }

    fn deallocate(&self, allocation: AllocResult) -> Result<(), AllocError> {
        let _ = (&self.pool, allocation);
        Err(AllocError::unsupported())
    }
}
