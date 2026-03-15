use super::{
    AllocCapabilities, AllocError, AllocHazards, AllocPolicy, AllocRequest, AllocResult, Allocator,
};

/// Fixed-size, bounded allocator planned on top of the internal pool substrate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Slab<const SIZE: usize, const COUNT: usize> {
    policy: AllocPolicy,
}

impl<const SIZE: usize, const COUNT: usize> Slab<SIZE, COUNT> {
    /// Creates a slab allocator description.
    ///
    /// # Errors
    ///
    /// Returns `invalid_request` for zero-sized configurations and `unsupported` until the
    /// concrete slab backing lands.
    pub const fn new(policy: AllocPolicy) -> Result<Self, AllocError> {
        if SIZE == 0 || COUNT == 0 {
            return Err(AllocError::invalid_request());
        }
        let _ = policy;
        Err(AllocError::unsupported())
    }

    /// Returns the capability surface a slab allocator intends to provide.
    #[must_use]
    pub const fn supported_capabilities() -> AllocCapabilities {
        AllocCapabilities::SLAB
            .union(AllocCapabilities::ZEROED_ALLOC)
            .union(AllocCapabilities::DETERMINISTIC)
            .union(AllocCapabilities::BOUNDED)
    }

    /// Returns the expected coarse slab hazards.
    #[must_use]
    pub const fn expected_hazards() -> AllocHazards {
        AllocHazards::empty()
    }

    /// Returns the slab policy.
    #[must_use]
    pub const fn policy(&self) -> AllocPolicy {
        self.policy
    }
}

impl<const SIZE: usize, const COUNT: usize> Allocator for Slab<SIZE, COUNT> {
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
