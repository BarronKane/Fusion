use super::{
    AllocCapabilities, AllocError, AllocHazards, AllocModeSet, AllocPolicy, AllocRequest,
    AllocResult, AllocationStrategy, AllocatorDomainId,
};

/// Bounded lifetime allocator intended for bulk-free or reset-driven use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BoundedArena {
    capacity: usize,
    domain: AllocatorDomainId,
    policy: AllocPolicy,
}

impl BoundedArena {
    /// Creates a bounded arena description.
    ///
    /// # Errors
    ///
    /// Returns `invalid_request` for zero capacity and `unsupported` until the concrete arena
    /// backing lands.
    pub const fn new(capacity: usize, policy: AllocPolicy) -> Result<Self, AllocError> {
        Self::for_domain(AllocatorDomainId(0), capacity, policy)
    }

    pub(super) const fn for_domain(
        domain: AllocatorDomainId,
        capacity: usize,
        policy: AllocPolicy,
    ) -> Result<Self, AllocError> {
        if capacity == 0 {
            return Err(AllocError::invalid_request());
        }
        if !policy.allows(AllocModeSet::ARENA) {
            return Err(AllocError::policy_denied());
        }
        let _ = (domain, policy);
        Err(AllocError::unsupported())
    }

    /// Returns the capability surface a bounded arena intends to provide.
    #[must_use]
    pub const fn supported_capabilities() -> AllocCapabilities {
        AllocCapabilities::ARENA
            .union(AllocCapabilities::DETERMINISTIC)
            .union(AllocCapabilities::BOUNDED)
    }

    /// Returns the expected coarse arena hazards.
    #[must_use]
    pub const fn expected_hazards() -> AllocHazards {
        AllocHazards::empty()
    }

    /// Returns the configured bounded capacity in bytes.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the arena policy.
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

impl AllocationStrategy for BoundedArena {
    fn policy(&self) -> AllocPolicy {
        self.policy
    }

    fn capabilities(&self) -> AllocCapabilities {
        Self::supported_capabilities()
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
