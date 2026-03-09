use fusion_pal::sys::mem::{
    MemCommit, MemLock, MemPool, MemProtect, PlatformMem, PlatformPoolHandle, PoolError,
    PoolHandle, PoolRequest, Protect, Region, ResolvedPoolConfig, system_mem,
};

pub use fusion_pal::sys::mem::{
    IntegrityConstraints, PoolAccess, PoolBackingKind, PoolBounds, PoolCapabilitySet,
    PoolErrorKind, PoolHazardSet, PoolLatency, PoolPreference, PoolPreferenceSet, PoolProhibition,
    PoolRequirement, PoolSharing,
};

#[derive(Debug)]
pub struct CreatedPool {
    pub pool: Pool,
    pub resolved: ResolvedPoolConfig,
}

#[derive(Debug)]
pub struct Pool {
    provider: PlatformMem,
    backing: Option<PlatformPoolHandle>,
    resolved: ResolvedPoolConfig,
}

impl Pool {
    pub fn create(request: &PoolRequest<'_>) -> Result<CreatedPool, PoolError> {
        let provider = system_mem();
        let (backing, resolved) = provider.create_pool(request)?;
        let pool = Self {
            provider,
            backing: Some(backing),
            resolved,
        };
        Ok(CreatedPool { pool, resolved })
    }

    #[must_use]
    pub fn region(&self) -> Region {
        self.backing_ref().region()
    }

    #[must_use]
    pub fn contains(&self, ptr: *const u8) -> bool {
        self.backing_ref().contains(ptr)
    }

    #[must_use]
    pub fn page_size(&self) -> usize {
        self.backing_ref().page_size()
    }

    #[must_use]
    pub fn page_count(&self) -> usize {
        self.region().len / self.page_size()
    }

    #[must_use]
    pub const fn resolved_config(&self) -> ResolvedPoolConfig {
        self.resolved
    }

    pub fn page_region(&self, first_page: usize, page_count: usize) -> Result<Region, PoolError> {
        if page_count == 0 {
            return Err(PoolError::invalid_range());
        }

        let page_size = self.page_size();
        let offset = first_page
            .checked_mul(page_size)
            .ok_or_else(PoolError::invalid_range)?;
        let len = page_count
            .checked_mul(page_size)
            .ok_or_else(PoolError::invalid_range)?;
        self.region()
            .subrange(offset, len)
            .map_err(|_| PoolError::invalid_range())
    }

    /// # Safety
    /// Caller must ensure the target pages are not actively referenced in ways that would
    /// violate the requested protection change.
    pub unsafe fn protect_pages(
        &self,
        first_page: usize,
        page_count: usize,
        protect: Protect,
    ) -> Result<(), PoolError> {
        let region = self.page_region(first_page, page_count)?;
        unsafe { self.provider.protect(region, protect) }.map_err(Into::into)
    }

    /// # Safety
    /// Caller must ensure committing these pages is valid for the backing strategy and that
    /// subsequent accesses respect the returned protection.
    pub unsafe fn commit_pages(
        &self,
        first_page: usize,
        page_count: usize,
        protect: Protect,
    ) -> Result<(), PoolError> {
        let region = self.page_region(first_page, page_count)?;
        unsafe { self.provider.commit(region, protect) }.map_err(Into::into)
    }

    /// # Safety
    /// Caller must ensure decommitting these pages does not invalidate live references.
    pub unsafe fn decommit_pages(
        &self,
        first_page: usize,
        page_count: usize,
    ) -> Result<(), PoolError> {
        let region = self.page_region(first_page, page_count)?;
        unsafe { self.provider.decommit(region) }.map_err(Into::into)
    }

    /// # Safety
    /// Caller must ensure locking these pages is legal for the target process and backing.
    pub unsafe fn lock_pages(&self, first_page: usize, page_count: usize) -> Result<(), PoolError> {
        let region = self.page_region(first_page, page_count)?;
        unsafe { self.provider.lock(region) }.map_err(Into::into)
    }

    /// # Safety
    /// Caller must ensure the pages were previously locked by a valid operation.
    pub unsafe fn unlock_pages(
        &self,
        first_page: usize,
        page_count: usize,
    ) -> Result<(), PoolError> {
        let region = self.page_region(first_page, page_count)?;
        unsafe { self.provider.unlock(region) }.map_err(Into::into)
    }

    fn backing_ref(&self) -> &PlatformPoolHandle {
        self.backing
            .as_ref()
            .expect("pool backing missing during active use")
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        if let Some(backing) = self.backing.take() {
            let _ = unsafe { self.provider.destroy_pool(backing) };
        }
    }
}

pub fn create(request: &PoolRequest<'_>) -> Result<CreatedPool, PoolError> {
    Pool::create(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_default_pool() {
        let request = PoolRequest::anonymous_private(16 * 1024);
        let created = Pool::create(&request).expect("pool");

        assert_eq!(created.resolved.backing, PoolBackingKind::AnonymousPrivate);
        assert!(
            created
                .resolved
                .granted_capabilities
                .contains(PoolCapabilitySet::PRIVATE_BACKING)
        );
        assert_eq!(created.pool.region().len, 16 * 1024);
    }

    #[test]
    fn page_region_respects_bounds() {
        let request = PoolRequest::anonymous_private(16 * 1024);
        let created = Pool::create(&request).expect("pool");
        let pool = created.pool;
        let region = pool.page_region(1, 2).expect("page region");

        assert_eq!(region.len, pool.page_size() * 2);
        assert!(pool.page_region(pool.page_count(), 1).is_err());
    }

    #[test]
    fn rejects_unsupported_requirement() {
        let requirements = [PoolRequirement::DmaVisible];
        let request = PoolRequest {
            requirements: &requirements,
            ..PoolRequest::anonymous_private(16 * 1024)
        };

        let err = Pool::create(&request).expect_err("dma-visible should fail");
        assert_eq!(err.kind, PoolErrorKind::UnsupportedRequirement);
    }

    #[test]
    fn enforces_executable_prohibition() {
        let prohibitions = [PoolProhibition::Executable];
        let request = PoolRequest {
            access: PoolAccess::ReadWriteExecute,
            prohibitions: &prohibitions,
            ..PoolRequest::anonymous_private(16 * 1024)
        };

        let err = Pool::create(&request).expect_err("executable should be prohibited");
        assert_eq!(err.kind, PoolErrorKind::ProhibitionViolated);
    }
}
