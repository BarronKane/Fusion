use core::array;
use core::fmt;

use fusion_pal::sys::mem::MemBase;
use fusion_pal::sys::mem::system_mem;

use crate::mem::resource::{
    AllocatorLayoutPolicy,
    MemoryResource,
    MemoryResourceHandle,
    ResourceInfo,
    ResourceRange,
    ResourceRequest,
    VirtualMemoryResource,
};

use super::{
    AllocCapabilities,
    AllocError,
    AllocHazards,
    AllocModeSet,
    AllocPolicy,
    AllocResult,
    AllocatorDomainAudit,
    AllocatorDomainId,
    AllocatorDomainInfo,
    AllocatorDomainKind,
    AssignedPoolExtent,
    BoundedArena,
    HeapAllocator,
    Immortal,
    MemoryPool,
    MemoryPoolContributor,
    MemoryPoolExtentRequest,
    MemoryPoolPolicy,
    PoolHandle,
    Slab,
    pool_control_backing_request,
};

#[derive(Debug)]
struct AllocatorResourceBinding {
    domain: AllocatorDomainId,
    handle: MemoryResourceHandle,
}

impl AllocatorResourceBinding {
    const fn new(domain: AllocatorDomainId, handle: MemoryResourceHandle) -> Self {
        Self { domain, handle }
    }
}

#[derive(Debug, Clone, Copy)]
struct AllocatorResourceRecord {
    domain: AllocatorDomainId,
    info: ResourceInfo,
}

impl AllocatorResourceRecord {
    const fn new(domain: AllocatorDomainId, info: ResourceInfo) -> Self {
        Self { domain, info }
    }
}

struct AllocatorDomainRecord<const RESOURCES: usize, const EXTENTS: usize> {
    info: AllocatorDomainInfo,
    pool: Option<PoolHandle>,
}

impl<const RESOURCES: usize, const EXTENTS: usize> AllocatorDomainRecord<RESOURCES, EXTENTS> {
    const fn new(info: AllocatorDomainInfo, pool: Option<PoolHandle>) -> Self {
        Self { info, pool }
    }

    fn assign_extent(
        &self,
        request: &super::MemoryPoolExtentRequest,
    ) -> Result<AssignedPoolExtent, AllocError> {
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(AllocError::capacity_exhausted)?
            .try_clone()?;
        AssignedPoolExtent::assign(pool, request)
    }
}

impl<const RESOURCES: usize, const EXTENTS: usize> fmt::Debug
    for AllocatorDomainRecord<RESOURCES, EXTENTS>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AllocatorDomainRecord")
            .field("info", &self.info)
            .field("pool", &self.pool.as_ref().map(|_| "owned"))
            .finish()
    }
}

/// Root allocation orchestration surface.
///
/// One allocator owns one or more domains. Each domain owns at most one realized pool, and each
/// owned resource may be assigned to exactly one domain.
#[derive(Debug)]
pub struct Allocator<
    const DOMAINS: usize = 4,
    const RESOURCES: usize = 16,
    const EXTENTS: usize = 64,
> {
    policy: AllocPolicy,
    capabilities: AllocCapabilities,
    hazards: AllocHazards,
    domains: [Option<AllocatorDomainRecord<RESOURCES, EXTENTS>>; DOMAINS],
    domain_count: usize,
    resources: [Option<AllocatorResourceRecord>; RESOURCES],
    resource_count: usize,
}

/// Builder for one allocator root.
#[derive(Debug)]
pub struct AllocatorBuilder<
    const DOMAINS: usize = 4,
    const RESOURCES: usize = 16,
    const EXTENTS: usize = 64,
> {
    policy: AllocPolicy,
    domains: [Option<AllocatorDomainInfo>; DOMAINS],
    domain_count: usize,
    resources: [Option<AllocatorResourceBinding>; RESOURCES],
    resource_count: usize,
    default_domain: Option<AllocatorDomainId>,
}

impl<const DOMAINS: usize, const RESOURCES: usize, const EXTENTS: usize>
    Allocator<DOMAINS, RESOURCES, EXTENTS>
{
    /// Creates a default allocator root builder.
    #[must_use]
    pub fn builder() -> AllocatorBuilder<DOMAINS, RESOURCES, EXTENTS> {
        AllocatorBuilder::new()
    }

    /// Creates a permissive zero-config allocator root backed by anonymous private virtual memory.
    ///
    /// # Errors
    ///
    /// Returns an error when allocator metadata is too small to host the default domain or the
    /// backing virtual resource cannot be acquired.
    pub fn system_default() -> Result<Self, AllocError> {
        let page = system_mem().page_info().alloc_granule.get();
        Self::system_default_with_capacity(page)
    }

    /// Creates a permissive zero-config allocator root backed by anonymous private virtual
    /// memory sized for at least `min_capacity` bytes.
    ///
    /// # Errors
    ///
    /// Returns an error when allocator metadata is too small to host the default domain, the
    /// requested capacity is invalid, or the backing virtual resource cannot be acquired.
    pub fn system_default_with_capacity(min_capacity: usize) -> Result<Self, AllocError> {
        if DOMAINS == 0 || RESOURCES == 0 {
            return Err(AllocError::metadata_exhausted());
        }
        if min_capacity == 0 {
            return Err(AllocError::invalid_request());
        }

        let page = system_mem().page_info().alloc_granule;
        let layout_policy = AllocatorLayoutPolicy::hosted_vm(page);
        let requested_len = Self::resource_request_for_extent_request_with_layout_policy(
            MemoryPoolExtentRequest::new(min_capacity.max(page.get())),
            layout_policy,
        )?
        .provisioning_len()
        .ok_or_else(AllocError::invalid_request)?;

        let mut request = ResourceRequest::anonymous_private(requested_len);
        request.name = Some("fusion-alloc-system-default");
        let resource = VirtualMemoryResource::create(&request)?;

        let mut builder = Self::builder();
        builder.policy(AllocPolicy::general_purpose());
        builder.add_resource(MemoryResourceHandle::from(resource))?;
        builder.build()
    }

    /// Creates one allocator root over one already-realized resource with one explicit policy.
    ///
    /// # Errors
    ///
    /// Returns an error when allocator metadata is exhausted or the supplied resource cannot be
    /// admitted honestly.
    pub fn from_resource_with_policy(
        handle: MemoryResourceHandle,
        policy: AllocPolicy,
    ) -> Result<Self, AllocError> {
        let mut builder = Self::builder();
        builder.policy(policy);
        builder.add_resource(handle)?;
        builder.build()
    }

    /// Creates one allocator root over one already-realized resource using the general-purpose
    /// allocator policy.
    ///
    /// # Errors
    ///
    /// Returns an error when allocator metadata is exhausted or the supplied resource cannot be
    /// admitted honestly.
    pub fn from_resource(handle: MemoryResourceHandle) -> Result<Self, AllocError> {
        Self::from_resource_with_policy(handle, AllocPolicy::general_purpose())
    }

    /// Returns the minimum resource request needed to host one allocator-managed pool extent on
    /// one owned resource, including allocator control metadata stored in-band on that resource.
    ///
    /// # Errors
    ///
    /// Returns an error when the supplied extent request or the allocator control shape cannot be
    /// represented honestly.
    pub fn resource_request_for_extent_request(
        request: MemoryPoolExtentRequest,
    ) -> Result<MemoryPoolExtentRequest, AllocError> {
        Self::resource_request_for_extent_request_with_layout_policy(
            request,
            AllocatorLayoutPolicy::hosted_vm(system_mem().page_info().alloc_granule),
        )
    }

    /// Returns the minimum resource request needed to host one allocator-managed pool extent on
    /// one owned resource under one explicit allocator layout policy.
    pub fn resource_request_for_extent_request_with_layout_policy(
        request: MemoryPoolExtentRequest,
        layout_policy: AllocatorLayoutPolicy,
    ) -> Result<MemoryPoolExtentRequest, AllocError> {
        let control = pool_control_backing_request::<RESOURCES, EXTENTS>()?;
        let bytes = control
            .provisioning_len()
            .ok_or_else(AllocError::invalid_request)?
            .checked_add(
                request
                    .provisioning_len()
                    .ok_or_else(AllocError::invalid_request)?,
            )
            .ok_or_else(AllocError::invalid_request)?;
        Ok(MemoryPoolExtentRequest {
            len: bytes,
            align: control
                .align
                .max(request.align)
                .max(layout_policy.min_extent_align.get()),
        })
    }

    /// Returns the allocator-wide policy.
    #[must_use]
    pub const fn policy(&self) -> AllocPolicy {
        self.policy
    }

    /// Returns the coarse allocator capability surface.
    #[must_use]
    pub const fn capabilities(&self) -> AllocCapabilities {
        self.capabilities
    }

    /// Returns the coarse allocator hazards.
    #[must_use]
    pub const fn hazards(&self) -> AllocHazards {
        self.hazards
    }

    /// Returns the number of configured domains.
    #[must_use]
    pub const fn domain_count(&self) -> usize {
        self.domain_count
    }

    /// Returns the number of owned resources.
    #[must_use]
    pub const fn resource_count(&self) -> usize {
        self.resource_count
    }

    /// Returns the owning domain for one stored resource slot.
    #[must_use]
    pub fn resource_domain(&self, index: usize) -> Option<AllocatorDomainId> {
        self.resources
            .get(index)
            .and_then(Option::as_ref)
            .map(|binding| binding.domain)
    }

    /// Returns immutable descriptive information for one stored resource slot.
    #[must_use]
    pub fn resource_info(&self, index: usize) -> Option<ResourceInfo> {
        self.resources
            .get(index)
            .and_then(Option::as_ref)
            .map(|binding| binding.info)
    }

    /// Returns observable information for one allocator domain.
    #[must_use]
    pub fn domain(&self, id: AllocatorDomainId) -> Option<AllocatorDomainInfo> {
        self.domain_record(id).map(|record| record.info)
    }

    /// Writes known domain identifiers into `out` and returns the number written.
    pub fn write_domain_ids(&self, out: &mut [AllocatorDomainId]) -> usize {
        let mut written = 0;
        for record in self.domains.iter().flatten() {
            let Some(slot) = out.get_mut(written) else {
                break;
            };
            *slot = record.info.id;
            written += 1;
        }
        written
    }

    /// Returns one auditable domain snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error when the domain does not exist or its realized pool cannot synchronize its
    /// live stats honestly.
    pub fn domain_audit(&self, id: AllocatorDomainId) -> Result<AllocatorDomainAudit, AllocError> {
        let record = self
            .domain_record(id)
            .ok_or_else(AllocError::invalid_domain)?;
        let primary_layout_policy = self
            .resources
            .iter()
            .flatten()
            .find(|binding| binding.domain == id)
            .map(|binding| binding.info.layout);
        let pool_stats = record.pool.as_ref().map(PoolHandle::stats).transpose()?;
        Ok(AllocatorDomainAudit {
            info: record.info,
            primary_layout_policy,
            pool_stats,
        })
    }

    /// Writes auditable domain snapshots into `out` and returns the number written.
    ///
    /// # Errors
    ///
    /// Returns an error when one realized pool cannot synchronize its stats honestly.
    pub fn write_domain_audits(
        &self,
        out: &mut [AllocatorDomainAudit],
    ) -> Result<usize, AllocError> {
        let mut written = 0;
        for record in self.domains.iter().flatten() {
            let Some(slot) = out.get_mut(written) else {
                break;
            };
            *slot = self.domain_audit(record.info.id)?;
            written += 1;
        }
        Ok(written)
    }

    /// Returns the implicit default domain when one exists.
    #[must_use]
    pub fn default_domain(&self) -> Option<AllocatorDomainId> {
        self.domains
            .iter()
            .flatten()
            .find(|domain| domain.info.kind == AllocatorDomainKind::Default)
            .map(|domain| domain.info.id)
    }

    /// Returns current pool stats for one domain when that domain owns a realized pool.
    ///
    /// # Errors
    ///
    /// Returns an error when the domain does not exist or the pool cannot synchronize its live
    /// stats honestly.
    pub fn domain_pool_stats(
        &self,
        id: AllocatorDomainId,
    ) -> Result<Option<super::MemoryPoolStats>, AllocError> {
        let record = self
            .domain_record(id)
            .ok_or_else(AllocError::invalid_domain)?;
        record.pool.as_ref().map(PoolHandle::stats).transpose()
    }

    /// Returns one pool-member snapshot for `domain` by stable stream index.
    ///
    /// # Errors
    ///
    /// Returns an error when the domain does not exist or the pool cannot synchronize its live
    /// metadata honestly.
    pub fn domain_pool_member_info_at(
        &self,
        id: AllocatorDomainId,
        index: usize,
    ) -> Result<Option<super::MemoryPoolMemberInfo>, AllocError> {
        let record = self
            .domain_record(id)
            .ok_or_else(AllocError::invalid_domain)?;
        record
            .pool
            .as_ref()
            .map(|pool| pool.member_info_at(index))
            .transpose()
            .map(|maybe| maybe.flatten())
    }

    /// Returns one tracked pool-extent snapshot for `domain` by stable stream index.
    ///
    /// # Errors
    ///
    /// Returns an error when the domain does not exist or the pool cannot synchronize its live
    /// metadata honestly.
    pub fn domain_pool_extent_info_at(
        &self,
        id: AllocatorDomainId,
        index: usize,
    ) -> Result<Option<super::MemoryPoolExtentInfo>, AllocError> {
        let record = self
            .domain_record(id)
            .ok_or_else(AllocError::invalid_domain)?;
        record
            .pool
            .as_ref()
            .map(|pool| pool.extent_info_at(index))
            .transpose()
            .map(|maybe| maybe.flatten())
    }

    /// Returns a slab strategy view for `domain`.
    ///
    /// # Errors
    ///
    /// Returns an error when the domain does not exist, slab allocation is denied by policy, or
    /// the domain owns no realized backing pool.
    pub fn slab<const SIZE: usize, const COUNT: usize>(
        &self,
        domain: AllocatorDomainId,
    ) -> Result<Slab<SIZE, COUNT>, AllocError> {
        let domain = self
            .domain_record(domain)
            .ok_or_else(AllocError::invalid_domain)?;
        if !domain.info.policy.allows(AllocModeSet::SLAB) {
            return Err(AllocError::policy_denied());
        }
        if domain.pool.is_none() {
            return Err(AllocError::capacity_exhausted());
        }
        let layout_policy = self.primary_layout_policy_for_domain(domain.info.id)?;
        let slot_align = Slab::<SIZE, COUNT>::slot_align_for_domain()?;
        let request =
            Slab::<SIZE, COUNT>::extent_request_with_layout_policy(slot_align, layout_policy)?;
        let extent = domain.assign_extent(&request)?;
        Slab::from_assigned_extent_with_layout_policy(
            domain.info.id,
            domain.info.policy,
            extent,
            layout_policy,
        )
    }

    /// Returns an immortal slab strategy view for `domain`.
    ///
    /// Dropping the wrapper intentionally leaves the assigned backing alive until process exit.
    ///
    /// # Errors
    ///
    /// Returns an error when the domain does not exist, slab allocation is denied by policy, or
    /// the domain owns no realized backing pool.
    pub fn immortal_slab<const SIZE: usize, const COUNT: usize>(
        &self,
        domain: AllocatorDomainId,
    ) -> Result<Slab<SIZE, COUNT, Immortal>, AllocError> {
        let domain = self
            .domain_record(domain)
            .ok_or_else(AllocError::invalid_domain)?;
        if !domain.info.policy.allows(AllocModeSet::SLAB) {
            return Err(AllocError::policy_denied());
        }
        if domain.pool.is_none() {
            return Err(AllocError::capacity_exhausted());
        }
        let layout_policy = self.primary_layout_policy_for_domain(domain.info.id)?;
        let slot_align = Slab::<SIZE, COUNT, Immortal>::slot_align_for_domain()?;
        let request = Slab::<SIZE, COUNT, Immortal>::extent_request_with_layout_policy(
            slot_align,
            layout_policy,
        )?;
        let extent = domain.assign_extent(&request)?;
        Slab::<SIZE, COUNT, Immortal>::from_assigned_extent_with_layout_policy(
            domain.info.id,
            domain.info.policy,
            extent,
            layout_policy,
        )
    }

    /// Returns a bounded-arena strategy view for `domain`.
    ///
    /// # Errors
    ///
    /// Returns an error when the domain does not exist, arena allocation is denied by policy, or
    /// the domain owns no realized backing pool.
    pub fn arena(
        &self,
        domain: AllocatorDomainId,
        capacity: usize,
    ) -> Result<BoundedArena, AllocError> {
        let domain = self
            .domain_record(domain)
            .ok_or_else(AllocError::invalid_domain)?;
        if !domain.info.policy.allows(AllocModeSet::ARENA) {
            return Err(AllocError::policy_denied());
        }
        if domain.pool.is_none() {
            return Err(AllocError::capacity_exhausted());
        }
        let max_align = self
            .primary_layout_policy_for_domain(domain.info.id)?
            .default_arena_align
            .get();
        self.arena_with_alignment(domain.info.id, capacity, max_align)
    }

    /// Returns an immortal bounded-arena strategy view for `domain`.
    ///
    /// Immortal arenas keep their backing alive until process exit and therefore do not expose
    /// `reset()`.
    ///
    /// # Errors
    ///
    /// Returns an error when the domain does not exist, arena allocation is denied by policy, or
    /// the domain owns no realized backing pool.
    pub fn immortal_arena(
        &self,
        domain: AllocatorDomainId,
        capacity: usize,
    ) -> Result<BoundedArena<Immortal>, AllocError> {
        let domain = self
            .domain_record(domain)
            .ok_or_else(AllocError::invalid_domain)?;
        if !domain.info.policy.allows(AllocModeSet::ARENA) {
            return Err(AllocError::policy_denied());
        }
        if domain.pool.is_none() {
            return Err(AllocError::capacity_exhausted());
        }
        let max_align = self
            .primary_layout_policy_for_domain(domain.info.id)?
            .default_arena_align
            .get();
        self.immortal_arena_with_alignment(domain.info.id, capacity, max_align)
    }

    /// Returns a bounded-arena strategy view for `domain` with explicit maximum alignment.
    ///
    /// # Errors
    ///
    /// Returns an error when the domain does not exist, arena allocation is denied by policy, or
    /// the domain owns no realized backing pool.
    pub fn arena_with_alignment(
        &self,
        domain: AllocatorDomainId,
        capacity: usize,
        max_align: usize,
    ) -> Result<BoundedArena, AllocError> {
        let domain = self
            .domain_record(domain)
            .ok_or_else(AllocError::invalid_domain)?;
        if !domain.info.policy.allows(AllocModeSet::ARENA) {
            return Err(AllocError::policy_denied());
        }
        if domain.pool.is_none() {
            return Err(AllocError::capacity_exhausted());
        }
        let layout_policy = self.primary_layout_policy_for_domain(domain.info.id)?;
        let request = BoundedArena::<super::Mortal>::extent_request_with_layout_policy(
            capacity,
            max_align,
            layout_policy,
        )?;
        let extent = domain.assign_extent(&request)?;
        BoundedArena::from_assigned_extent_with_layout_policy(
            domain.info.id,
            capacity,
            max_align,
            domain.info.policy,
            extent,
            layout_policy,
        )
    }

    /// Returns an immortal bounded-arena strategy view for `domain` with explicit maximum
    /// alignment.
    ///
    /// # Errors
    ///
    /// Returns an error when the domain does not exist, arena allocation is denied by policy, or
    /// the domain owns no realized backing pool.
    pub fn immortal_arena_with_alignment(
        &self,
        domain: AllocatorDomainId,
        capacity: usize,
        max_align: usize,
    ) -> Result<BoundedArena<Immortal>, AllocError> {
        let domain = self
            .domain_record(domain)
            .ok_or_else(AllocError::invalid_domain)?;
        if !domain.info.policy.allows(AllocModeSet::ARENA) {
            return Err(AllocError::policy_denied());
        }
        if domain.pool.is_none() {
            return Err(AllocError::capacity_exhausted());
        }
        let layout_policy = self.primary_layout_policy_for_domain(domain.info.id)?;
        let request = BoundedArena::<Immortal>::extent_request_with_layout_policy(
            capacity,
            max_align,
            layout_policy,
        )?;
        let extent = domain.assign_extent(&request)?;
        BoundedArena::<Immortal>::from_assigned_extent_with_layout_policy(
            domain.info.id,
            capacity,
            max_align,
            domain.info.policy,
            extent,
            layout_policy,
        )
    }

    /// Returns one allocator-backed shared control block for `value`.
    ///
    /// # Errors
    ///
    /// Returns an error when the domain does not exist, owns no realized pool, or cannot
    /// reserve control backing honestly.
    pub fn control<T>(
        &self,
        domain: AllocatorDomainId,
        value: T,
    ) -> Result<super::ControlLease<T>, AllocError> {
        let domain = self
            .domain_record(domain)
            .ok_or_else(AllocError::invalid_domain)?;
        let request = super::ControlLease::<T>::extent_request()?;
        let extent = domain.assign_extent(&request)?;
        super::ControlLease::new(extent, value)
    }

    /// Returns a general-purpose heap strategy view for `domain`.
    ///
    /// # Errors
    ///
    /// Returns an error when the domain does not exist, heap allocation is denied by policy, or
    /// the domain owns no realized backing pool.
    pub fn heap(&self, domain: AllocatorDomainId) -> Result<HeapAllocator, AllocError> {
        let domain = self
            .domain_record(domain)
            .ok_or_else(AllocError::invalid_domain)?;
        if !domain.info.policy.allows(AllocModeSet::HEAP) {
            return Err(AllocError::policy_denied());
        }
        HeapAllocator::for_domain(domain.info.id, domain.info.policy, domain.pool.as_ref())
    }

    /// Attempts to allocate one heap-routed block from the allocator root.
    ///
    /// # Errors
    ///
    /// Returns an error when no default domain exists, heap allocation is denied by policy, or
    /// heap allocation remains unsupported on the current implementation path.
    pub fn malloc(&self, len: usize) -> Result<AllocResult, AllocError> {
        if len == 0 {
            return Err(AllocError::invalid_request());
        }
        let _ = self.heap(
            self.default_domain()
                .ok_or_else(AllocError::invalid_domain)?,
        )?;
        Err(AllocError::unsupported())
    }

    /// Attempts to allocate one zero-initialized heap-routed block from the allocator root.
    ///
    /// # Errors
    ///
    /// Returns an error when no default domain exists, heap allocation is denied by policy, or
    /// heap allocation remains unsupported on the current implementation path.
    pub fn calloc(&self, len: usize) -> Result<AllocResult, AllocError> {
        if len == 0 {
            return Err(AllocError::invalid_request());
        }
        let _ = self.heap(
            self.default_domain()
                .ok_or_else(AllocError::invalid_domain)?,
        )?;
        Err(AllocError::unsupported())
    }

    /// Attempts to grow or shrink an existing heap-routed allocation.
    ///
    /// # Errors
    ///
    /// Returns an error when no default domain exists, heap allocation is denied by policy, the
    /// requested new length is invalid, or realloc remains unsupported on the current path.
    #[allow(clippy::needless_pass_by_value)]
    pub fn realloc(
        &self,
        allocation: AllocResult,
        new_len: usize,
    ) -> Result<AllocResult, AllocError> {
        let _ = allocation;
        self.heap(
            self.default_domain()
                .ok_or_else(AllocError::invalid_domain)?,
        )?;
        if new_len == 0 {
            return Err(AllocError::invalid_request());
        }
        Err(AllocError::unsupported())
    }

    /// Attempts to release a heap-routed allocation.
    ///
    /// # Errors
    ///
    /// Returns an error when no default domain exists, heap allocation is denied by policy, or
    /// deallocation remains unsupported on the current implementation path.
    #[allow(clippy::needless_pass_by_value)]
    pub fn free(&self, allocation: AllocResult) -> Result<(), AllocError> {
        let _ = allocation;
        let _ = self.heap(
            self.default_domain()
                .ok_or_else(AllocError::invalid_domain)?,
        )?;
        Err(AllocError::unsupported())
    }

    fn domain_record(
        &self,
        id: AllocatorDomainId,
    ) -> Option<&AllocatorDomainRecord<RESOURCES, EXTENTS>> {
        self.domains
            .iter()
            .flatten()
            .find(|domain| domain.info.id == id)
    }

    fn primary_layout_policy_for_domain(
        &self,
        id: AllocatorDomainId,
    ) -> Result<AllocatorLayoutPolicy, AllocError> {
        self.resources
            .iter()
            .flatten()
            .find(|binding| binding.domain == id)
            .map(|binding| binding.info.layout)
            .ok_or_else(AllocError::invalid_domain)
    }
}

impl<const DOMAINS: usize, const RESOURCES: usize, const EXTENTS: usize>
    AllocatorBuilder<DOMAINS, RESOURCES, EXTENTS>
{
    /// Creates a new allocator builder with a critical-safe baseline policy.
    #[must_use]
    pub fn new() -> Self {
        Self {
            policy: AllocPolicy::critical_safe(),
            domains: array::from_fn(|_| None),
            domain_count: 0,
            resources: array::from_fn(|_| None),
            resource_count: 0,
            default_domain: None,
        }
    }

    /// Replaces the root allocator policy.
    pub fn policy(&mut self, policy: AllocPolicy) -> &mut Self {
        self.policy = policy;
        self.sync_default_domain_policy();
        self
    }

    /// Overlays additional critical-safety requirements onto the root policy.
    pub fn critical_safety(&mut self, required: super::CriticalSafetyRequirements) -> &mut Self {
        self.policy.safety |= required;
        self.sync_default_domain_policy();
        self
    }

    /// Adds one explicit domain and returns its stable identifier.
    ///
    /// # Errors
    ///
    /// Returns an error when the fixed domain metadata is exhausted.
    pub fn add_domain(&mut self, policy: AllocPolicy) -> Result<AllocatorDomainId, AllocError> {
        let slot = self
            .domains
            .iter()
            .position(Option::is_none)
            .ok_or_else(AllocError::metadata_exhausted)?;
        let id = AllocatorDomainId(
            u16::try_from(self.domain_count).map_err(|_| AllocError::metadata_exhausted())?,
        );
        self.domains[slot] = Some(AllocatorDomainInfo::new(
            id,
            AllocatorDomainKind::Explicit,
            policy,
        ));
        self.domain_count += 1;
        Ok(id)
    }

    /// Adds one resource to the implicit default domain, creating it if needed.
    ///
    /// # Errors
    ///
    /// Returns an error when builder metadata is exhausted.
    pub fn add_resource(&mut self, handle: MemoryResourceHandle) -> Result<&mut Self, AllocError> {
        let domain = self.ensure_default_domain()?;
        self.add_resource_to_domain(domain, handle)
    }

    /// Adds one resource to an explicit allocator domain.
    ///
    /// # Errors
    ///
    /// Returns an error when the domain does not exist or builder metadata is exhausted.
    pub fn add_resource_to_domain(
        &mut self,
        domain: AllocatorDomainId,
        handle: MemoryResourceHandle,
    ) -> Result<&mut Self, AllocError> {
        let slot = self
            .resources
            .iter()
            .position(Option::is_none)
            .ok_or_else(AllocError::metadata_exhausted)?;
        let domain_slot = self
            .find_domain_slot(domain)
            .ok_or_else(AllocError::invalid_domain)?;
        let resource_info = handle.info();
        let Some(domain_info) = self.domains[domain_slot].as_mut() else {
            return Err(AllocError::invalid_domain());
        };
        domain_info.note_resource(
            resource_info.domain,
            resource_info.attrs,
            resource_info.hazards,
        );
        self.resources[slot] = Some(AllocatorResourceBinding::new(domain, handle));
        self.resource_count += 1;
        Ok(self)
    }

    /// Builds the allocator root from the staged domains and resources.
    ///
    /// # Errors
    ///
    /// Returns an error when the builder contains resources but no domains or otherwise fails
    /// ownership or pool-realization validation.
    pub fn build(mut self) -> Result<Allocator<DOMAINS, RESOURCES, EXTENTS>, AllocError> {
        if self.domain_count == 0 {
            self.ensure_default_domain()?;
        }

        let mut resource_records = array::from_fn(|_| None);
        let mut domain_records = array::from_fn(|_| None);
        let mut staged_resources = self.resources;

        for (slot, domain) in self.domains.into_iter().enumerate() {
            let Some(info) = domain else {
                continue;
            };

            let mut pool_builder =
                MemoryPool::<RESOURCES, EXTENTS>::builder(MemoryPoolPolicy::ready_only());
            let mut contributor_count = 0;
            let mut control_region = None;

            for (resource_slot, binding) in staged_resources.iter_mut().enumerate() {
                let Some(binding_ref) = binding.as_ref() else {
                    continue;
                };
                if binding_ref.domain != info.id {
                    continue;
                }

                let Some(binding) = binding.take() else {
                    return Err(AllocError::metadata_exhausted());
                };
                resource_records[resource_slot] = Some(AllocatorResourceRecord::new(
                    binding.domain,
                    *binding.handle.info(),
                ));
                let mut contributor = MemoryPoolContributor::explicit_ready(binding.handle);
                if control_region.is_none() {
                    if let Some((region, usable_range)) =
                        reserve_pool_control_region::<RESOURCES, EXTENTS>(
                            &contributor.handle,
                            contributor.usable_range,
                        )?
                    {
                        control_region = Some(region);
                        contributor.usable_range = usable_range;
                    }
                }
                pool_builder.add_contributor(contributor)?;
                contributor_count += 1;
            }

            let pool = if contributor_count == 0 {
                None
            } else {
                let control_region = control_region.ok_or_else(AllocError::capacity_exhausted)?;
                Some(PoolHandle::new_in_region(
                    pool_builder.build()?,
                    control_region,
                )?)
            };
            domain_records[slot] = Some(AllocatorDomainRecord::new(info, pool));
        }

        Ok(Allocator {
            policy: self.policy,
            capabilities: allocator_capabilities_for_domains(&domain_records),
            hazards: allocator_hazards_for_domains(&domain_records),
            domains: domain_records,
            domain_count: self.domain_count,
            resources: resource_records,
            resource_count: self.resource_count,
        })
    }

    fn ensure_default_domain(&mut self) -> Result<AllocatorDomainId, AllocError> {
        if let Some(domain) = self.default_domain {
            return Ok(domain);
        }

        let slot = self
            .domains
            .iter()
            .position(Option::is_none)
            .ok_or_else(AllocError::metadata_exhausted)?;
        let id = AllocatorDomainId(
            u16::try_from(self.domain_count).map_err(|_| AllocError::metadata_exhausted())?,
        );
        self.domains[slot] = Some(AllocatorDomainInfo::new(
            id,
            AllocatorDomainKind::Default,
            self.policy,
        ));
        self.domain_count += 1;
        self.default_domain = Some(id);
        Ok(id)
    }

    fn find_domain_slot(&self, id: AllocatorDomainId) -> Option<usize> {
        self.domains
            .iter()
            .position(|domain| domain.is_some_and(|domain| domain.id == id))
    }

    fn sync_default_domain_policy(&mut self) {
        let Some(default_domain) = self.default_domain else {
            return;
        };
        if let Some(slot) = self.find_domain_slot(default_domain)
            && let Some(domain) = &mut self.domains[slot]
        {
            domain.policy = self.policy;
        }
    }
}

impl<const DOMAINS: usize, const RESOURCES: usize, const EXTENTS: usize> Default
    for AllocatorBuilder<DOMAINS, RESOURCES, EXTENTS>
{
    fn default() -> Self {
        Self::new()
    }
}

fn reserve_pool_control_region<const RESOURCES: usize, const EXTENTS: usize>(
    handle: &MemoryResourceHandle,
    usable_range: ResourceRange,
) -> Result<Option<(fusion_pal::sys::mem::Region, ResourceRange)>, AllocError> {
    let request = pool_control_backing_request::<RESOURCES, EXTENTS>()?;
    let reserved_len = request
        .provisioning_len()
        .ok_or_else(AllocError::invalid_request)?;
    if usable_range.len <= reserved_len {
        return Ok(None);
    }

    let view = handle
        .subview(usable_range)
        .map_err(|_| AllocError::invalid_request())?;
    let start = view.base_addr().get();
    let aligned = super::align_up(start, request.align)?;
    let padding = aligned
        .checked_sub(start)
        .ok_or_else(AllocError::invalid_request)?;
    let control_range = ResourceRange::new(
        usable_range
            .offset
            .checked_add(padding)
            .ok_or_else(AllocError::invalid_request)?,
        request.len,
    );
    let control_region = handle
        .subview(control_range)
        .map_err(|_| AllocError::invalid_request())
        .and_then(|view| {
            // SAFETY: the control block lives inside the contributor resource, which remains owned
            // by the pool for at least as long as the control block itself.
            Ok(unsafe { view.raw_region() })
        })?;
    let remaining = ResourceRange::new(
        usable_range
            .offset
            .checked_add(reserved_len)
            .ok_or_else(AllocError::invalid_request)?,
        usable_range
            .len
            .checked_sub(reserved_len)
            .ok_or_else(AllocError::invalid_request)?,
    );
    Ok(Some((control_region, remaining)))
}

fn allocator_capabilities_for_domains<
    const DOMAINS: usize,
    const RESOURCES: usize,
    const EXTENTS: usize,
>(
    domains: &[Option<AllocatorDomainRecord<RESOURCES, EXTENTS>>; DOMAINS],
) -> AllocCapabilities {
    let mut capabilities = AllocCapabilities::empty();
    for domain in domains.iter().flatten() {
        if domain.pool.is_none() {
            continue;
        }
        if domain.info.policy.allows(AllocModeSet::SLAB) {
            capabilities = capabilities
                .union(AllocCapabilities::SLAB)
                .union(AllocCapabilities::ZEROED_ALLOC);
        }
        if domain.info.policy.allows(AllocModeSet::ARENA) {
            capabilities = capabilities.union(AllocCapabilities::ARENA);
        }
    }
    if !capabilities.is_empty() && !capabilities.contains(AllocCapabilities::HEAP) {
        capabilities = capabilities
            .union(AllocCapabilities::DETERMINISTIC)
            .union(AllocCapabilities::BOUNDED);
    }
    capabilities
}

fn allocator_hazards_for_domains<
    const DOMAINS: usize,
    const RESOURCES: usize,
    const EXTENTS: usize,
>(
    _domains: &[Option<AllocatorDomainRecord<RESOURCES, EXTENTS>>; DOMAINS],
) -> AllocHazards {
    AllocHazards::empty()
}
