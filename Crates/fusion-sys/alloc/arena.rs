use super::{
    AllocCapabilities, AllocError, AllocHazards, AllocPolicy, AllocRequest, AllocResult, Allocator,
};

/// Bounded lifetime allocator intended for bulk-free or reset-driven use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BoundedArena {
    capacity: usize,
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
        if capacity == 0 {
            return Err(AllocError::invalid_request());
        }
        let _ = policy;
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
}

impl Allocator for BoundedArena {
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
