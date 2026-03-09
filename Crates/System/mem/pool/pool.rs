use fusion_pal::sys::mem::{
    Backing, CachePolicy, MapFlags, MapRequest, MemBase, MemCaps, MemErrorKind, MemLock, MemMap,
    MemProtect, Placement, Protect, Region, RegionAttrs, system_mem,
};

#[path = "capability.rs"]
mod capability;
#[path = "error.rs"]
mod error;
#[path = "request.rs"]
mod request;

pub use capability::{
    PoolBackingKind, PoolCapabilitySet, PoolHazardSet, ResolvedPoolConfig, UnmetPoolPreferenceSet,
};
pub use error::{PoolError, PoolErrorKind};
pub use request::{
    IntegrityConstraints, PoolAccess, PoolBounds, PoolLatency, PoolPreference, PoolProhibition,
    PoolRequest, PoolRequirement, PoolSharing,
};

#[derive(Debug)]
pub struct CreatedPool {
    pub pool: Pool,
    pub resolved: ResolvedPoolConfig,
}

#[derive(Debug)]
pub struct Pool {
    provider: fusion_pal::sys::mem::PlatformMem,
    region: Option<Region>,
    page_size: usize,
    resolved: ResolvedPoolConfig,
}

impl Pool {
    pub fn create(request: &PoolRequest<'_>) -> Result<CreatedPool, PoolError> {
        let provider = system_mem();
        let page_size = provider.page_info().alloc_granule.get();
        let capacity = aligned_capacity(request.bounds, page_size)?;
        let mut bounds = request.bounds;
        bounds.initial_capacity = capacity;
        bounds.max_capacity = Some(capacity);
        bounds.growable = false;

        if matches!(request.sharing, PoolSharing::Shared) {
            return Err(PoolError::unsupported_requirement());
        }

        let mem_caps = provider.caps();
        let mut granted = pool_caps_from_mem(mem_caps);
        let mut unmet = UnmetPoolPreferenceSet::empty();
        let emulated = PoolCapabilitySet::empty();
        let mut hazards = PoolHazardSet::empty();

        let protect = match request.access {
            PoolAccess::ReadWrite => Protect::READ | Protect::WRITE,
            PoolAccess::ReadWriteExecute => {
                if !mem_caps.contains(MemCaps::EXECUTE_MAP) {
                    return Err(PoolError::unsupported_requirement());
                }
                hazards |= PoolHazardSet::EXECUTABLE;
                granted |= PoolCapabilitySet::EXECUTABLE;
                Protect::READ | Protect::WRITE | Protect::EXEC
            }
        };

        for prohibition in request.prohibitions {
            match prohibition {
                PoolProhibition::Executable => {
                    if protect.contains(Protect::EXEC) {
                        return Err(PoolError::prohibition_violated());
                    }
                }
                PoolProhibition::Overcommit => {
                    return Err(PoolError::unsupported_requirement());
                }
                PoolProhibition::Shared => {
                    if matches!(request.sharing, PoolSharing::Shared) {
                        return Err(PoolError::prohibition_violated());
                    }
                }
                PoolProhibition::ReplaceMapping
                | PoolProhibition::DeviceLocal
                | PoolProhibition::Physical
                | PoolProhibition::Emulation => {}
            }
        }

        let mut required_placement = None;
        let mut preferred_placement = None;
        let mut map_flags = MapFlags::PRIVATE;
        let mut prefer_populate = false;
        let mut prefer_lock = false;

        match request.latency {
            PoolLatency::BestEffort => {}
            PoolLatency::Prefault => {
                map_flags |= MapFlags::POPULATE;
                granted |= PoolCapabilitySet::POPULATE;
            }
            PoolLatency::Locked => {
                if !mem_caps.contains(MemCaps::LOCK) {
                    return Err(PoolError::unsupported_requirement());
                }
                map_flags |= MapFlags::LOCKED;
                granted |= PoolCapabilitySet::LOCKABLE;
            }
        }

        for requirement in request.requirements {
            match *requirement {
                PoolRequirement::Placement(placement) => {
                    validate_required_placement(placement, mem_caps)?;
                    required_placement = Some(placement);
                }
                PoolRequirement::Query => {
                    if !mem_caps.contains(MemCaps::QUERY) {
                        return Err(PoolError::unsupported_requirement());
                    }
                    granted |= PoolCapabilitySet::QUERY;
                }
                PoolRequirement::Locked => {
                    if !mem_caps.contains(MemCaps::LOCK) {
                        return Err(PoolError::unsupported_requirement());
                    }
                    map_flags |= MapFlags::LOCKED;
                    granted |= PoolCapabilitySet::LOCKABLE;
                }
                PoolRequirement::NoOvercommit => {
                    return Err(PoolError::unsupported_requirement());
                }
                PoolRequirement::CachePolicy(policy) => {
                    if policy != CachePolicy::Default {
                        return Err(PoolError::unsupported_requirement());
                    }
                }
                PoolRequirement::Integrity(_)
                | PoolRequirement::DmaVisible
                | PoolRequirement::PhysicalContiguous
                | PoolRequirement::DeviceLocal
                | PoolRequirement::Shared
                | PoolRequirement::ZeroOnFree => {
                    return Err(PoolError::unsupported_requirement());
                }
            }
        }

        for preference in request.preferences {
            match *preference {
                PoolPreference::Placement(placement) => {
                    if required_placement.is_none()
                        && validate_required_placement(placement, mem_caps).is_ok()
                    {
                        preferred_placement = Some(placement);
                    } else {
                        unmet |= UnmetPoolPreferenceSet::PLACEMENT;
                    }
                }
                PoolPreference::Populate => {
                    if !map_flags.contains(MapFlags::POPULATE) {
                        prefer_populate = true;
                    }
                }
                PoolPreference::Lock => {
                    if !map_flags.contains(MapFlags::LOCKED) && mem_caps.contains(MemCaps::LOCK) {
                        prefer_lock = true;
                    } else if !mem_caps.contains(MemCaps::LOCK) {
                        unmet |= UnmetPoolPreferenceSet::LOCK;
                    }
                }
                PoolPreference::HugePages => {
                    unmet |= UnmetPoolPreferenceSet::HUGE_PAGES;
                }
                PoolPreference::ZeroOnFree => {
                    unmet |= UnmetPoolPreferenceSet::ZERO_ON_FREE;
                }
            }
        }

        let required_request = base_pool_request(capacity, protect, map_flags, required_placement);
        let mut preferred_flags = map_flags;
        if prefer_lock {
            preferred_flags |= MapFlags::LOCKED;
        }
        if prefer_populate {
            preferred_flags |= MapFlags::POPULATE;
        }
        let preferred_request = base_pool_request(
            capacity,
            protect,
            preferred_flags,
            preferred_placement.or(required_placement),
        );

        let (region, final_placement, final_flags) =
            match unsafe { provider.map(&preferred_request) } {
                Ok(region) => (region, preferred_request.placement, preferred_request.flags),
                Err(error) => {
                    if preferred_request.flags == required_request.flags
                        && preferred_request.placement == required_request.placement
                    {
                        return Err(error.into());
                    }

                    if prefer_lock {
                        unmet |= UnmetPoolPreferenceSet::LOCK;
                    }
                    if prefer_populate {
                        unmet |= UnmetPoolPreferenceSet::POPULATE;
                    }
                    if preferred_placement.is_some() {
                        unmet |= UnmetPoolPreferenceSet::PLACEMENT;
                    }

                    let region = unsafe { provider.map(&required_request) }?;
                    (region, required_request.placement, required_request.flags)
                }
            };

        if final_flags.contains(MapFlags::LOCKED) {
            granted |= PoolCapabilitySet::LOCKABLE;
        }
        if final_flags.contains(MapFlags::POPULATE) {
            granted |= PoolCapabilitySet::POPULATE;
        }
        if let Placement::FixedNoReplace(_) = final_placement {
            granted |= PoolCapabilitySet::FIXED_NOREPLACE;
        }

        let resolved = ResolvedPoolConfig {
            backing: PoolBackingKind::AnonymousPrivate,
            bounds,
            granted_capabilities: granted,
            unmet_preferences: unmet,
            emulated_capabilities: emulated,
            residual_hazards: hazards,
        };

        let pool = Self {
            provider,
            region: Some(region),
            page_size,
            resolved,
        };

        Ok(CreatedPool { pool, resolved })
    }

    #[must_use]
    pub fn region(&self) -> Region {
        self.region.expect("pool region missing during active use")
    }

    #[must_use]
    pub fn contains(&self, ptr: *const u8) -> bool {
        self.region().contains(ptr as usize)
    }

    #[must_use]
    pub const fn page_size(&self) -> usize {
        self.page_size
    }

    #[must_use]
    pub fn page_count(&self) -> usize {
        self.region().len / self.page_size
    }

    #[must_use]
    pub const fn resolved_config(&self) -> ResolvedPoolConfig {
        self.resolved
    }

    pub fn page_region(&self, first_page: usize, page_count: usize) -> Result<Region, PoolError> {
        if page_count == 0 {
            return Err(PoolError::invalid_range());
        }

        let offset = first_page
            .checked_mul(self.page_size)
            .ok_or_else(PoolError::invalid_range)?;
        let len = page_count
            .checked_mul(self.page_size)
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
        let _region = self.page_region(first_page, page_count)?;
        let _protect = protect;
        Err(PoolError::platform(MemErrorKind::Unsupported))
    }

    /// # Safety
    /// Caller must ensure decommitting these pages does not invalidate live references.
    pub unsafe fn decommit_pages(
        &self,
        first_page: usize,
        page_count: usize,
    ) -> Result<(), PoolError> {
        let _region = self.page_region(first_page, page_count)?;
        Err(PoolError::platform(MemErrorKind::Unsupported))
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
}

impl Drop for Pool {
    fn drop(&mut self) {
        if let Some(region) = self.region.take() {
            let _ = unsafe { self.provider.unmap(region) };
        }
    }
}

pub fn create(request: &PoolRequest<'_>) -> Result<CreatedPool, PoolError> {
    Pool::create(request)
}

fn pool_caps_from_mem(mem_caps: MemCaps) -> PoolCapabilitySet {
    let mut out = PoolCapabilitySet::PRIVATE_BACKING;

    if mem_caps.contains(MemCaps::LOCK) {
        out |= PoolCapabilitySet::LOCKABLE;
    }
    if mem_caps.contains(MemCaps::MAP_FIXED_NOREPLACE) {
        out |= PoolCapabilitySet::FIXED_NOREPLACE;
    }
    if mem_caps.contains(MemCaps::ADVISE) {
        out |= PoolCapabilitySet::ADVISE;
    }
    if mem_caps.contains(MemCaps::QUERY) {
        out |= PoolCapabilitySet::QUERY;
    }
    if mem_caps.contains(MemCaps::PHYSICAL_MAP) {
        out |= PoolCapabilitySet::PHYSICAL;
    }
    if mem_caps.contains(MemCaps::DEVICE_MAP) {
        out |= PoolCapabilitySet::DEVICE_LOCAL;
    }
    if mem_caps.contains(MemCaps::INTEGRITY_CONTROL) {
        out |= PoolCapabilitySet::INTEGRITY;
    }
    if mem_caps.contains(MemCaps::CACHE_POLICY) {
        out |= PoolCapabilitySet::CACHE_POLICY;
    }

    out
}

fn validate_required_placement(placement: Placement, mem_caps: MemCaps) -> Result<(), PoolError> {
    match placement {
        Placement::Anywhere | Placement::Hint(_) => Ok(()),
        Placement::FixedNoReplace(_) if mem_caps.contains(MemCaps::MAP_FIXED_NOREPLACE) => Ok(()),
        Placement::FixedNoReplace(_) => Err(PoolError::unsupported_requirement()),
        Placement::PreferredNode(_) | Placement::RequiredNode(_) | Placement::RegionId(_) => {
            Err(PoolError::unsupported_requirement())
        }
    }
}

fn aligned_capacity(bounds: PoolBounds, granule: usize) -> Result<usize, PoolError> {
    if bounds.initial_capacity == 0 {
        return Err(PoolError::invalid_request());
    }

    if bounds.growable {
        return Err(PoolError::unsupported_requirement());
    }

    if let Some(max) = bounds.max_capacity {
        if max < bounds.initial_capacity {
            return Err(PoolError::invalid_request());
        }
    }

    align_up(bounds.initial_capacity, granule).ok_or_else(PoolError::invalid_request)
}

fn base_pool_request(
    capacity: usize,
    protect: Protect,
    mut flags: MapFlags,
    placement: Option<Placement>,
) -> MapRequest<'static> {
    let placement = placement.unwrap_or(Placement::Anywhere);
    if matches!(
        placement,
        Placement::Anywhere | Placement::Hint(_) | Placement::FixedNoReplace(_)
    ) {
        flags |= MapFlags::PRIVATE;
    }

    MapRequest {
        len: capacity,
        align: 0,
        protect,
        flags,
        attrs: RegionAttrs::VIRTUAL_ONLY,
        cache: CachePolicy::Default,
        placement,
        backing: Backing::Anonymous,
    }
}

fn align_up(value: usize, align: usize) -> Option<usize> {
    if align == 0 {
        return Some(value);
    }

    let mask = align.checked_sub(1)?;
    value.checked_add(mask).map(|v| v & !mask)
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
