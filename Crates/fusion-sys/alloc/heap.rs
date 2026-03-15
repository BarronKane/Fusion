use super::{
    AllocCapabilities, AllocError, AllocHazards, AllocModeSet, AllocPolicy, AllocRequest,
    AllocResult, AllocationStrategy, AllocatorDomainId,
};

/// General-purpose allocator surface for non-critical-safe heap behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HeapAllocator {
    domain: AllocatorDomainId,
    policy: AllocPolicy,
}

impl HeapAllocator {
    /// Creates a heap allocator description.
    ///
    /// # Errors
    ///
    /// Returns `policy_denied` when heap behavior is disabled and `unsupported` until the
    /// concrete heap implementation lands.
    pub const fn new(policy: AllocPolicy) -> Result<Self, AllocError> {
        Self::for_domain(AllocatorDomainId(0), policy)
    }

    pub(super) const fn for_domain(
        domain: AllocatorDomainId,
        policy: AllocPolicy,
    ) -> Result<Self, AllocError> {
        if !policy.allows(AllocModeSet::HEAP) {
            return Err(AllocError::policy_denied());
        }
        let _ = (domain, policy);
        Err(AllocError::unsupported())
    }

    /// Returns the capability surface a general-purpose heap intends to provide.
    #[must_use]
    pub const fn supported_capabilities(policy: AllocPolicy) -> AllocCapabilities {
        if !policy.allows(AllocModeSet::HEAP) {
            return AllocCapabilities::empty();
        }

        let capabilities = AllocCapabilities::HEAP
            .union(AllocCapabilities::ZEROED_ALLOC)
            .union(AllocCapabilities::REALLOC);
        if policy.allows(AllocModeSet::GLOBAL_ALLOC) {
            capabilities.union(AllocCapabilities::GLOBAL_ALLOC)
        } else {
            capabilities
        }
    }

    /// Returns the expected coarse heap hazards.
    #[must_use]
    pub const fn expected_hazards() -> AllocHazards {
        AllocHazards::FRAGMENTATION
            .union(AllocHazards::VARIABLE_LATENCY)
            .union(AllocHazards::EXTERNAL_GROWTH)
            .union(AllocHazards::MAY_BLOCK)
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
        let _ = self;
        Err(AllocError::unsupported())
    }

    fn deallocate(&self, allocation: AllocResult) -> Result<(), AllocError> {
        let _ = (self, allocation);
        Err(AllocError::unsupported())
    }
}
