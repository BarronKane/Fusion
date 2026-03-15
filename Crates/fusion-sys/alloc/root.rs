use core::array;

use crate::mem::resource::{MemoryResource, MemoryResourceHandle, ResourceInfo};

use super::{
    AllocCapabilities, AllocError, AllocHazards, AllocModeSet, AllocPolicy, AllocRequest,
    AllocResult, AllocationStrategy, AllocatorDomainId, AllocatorDomainInfo, AllocatorDomainKind,
    BoundedArena, CriticalSafetyRequirements, HeapAllocator, Slab,
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

/// Root allocation orchestration surface.
///
/// One allocator owns one or more domains. Each domain corresponds to one internal pool-owned
/// allocation stack, and each owned resource may be assigned to exactly one domain.
#[derive(Debug)]
pub struct Allocator<const DOMAINS: usize = 4, const RESOURCES: usize = 16> {
    policy: AllocPolicy,
    capabilities: AllocCapabilities,
    hazards: AllocHazards,
    domains: [Option<AllocatorDomainInfo>; DOMAINS],
    domain_count: usize,
    resources: [Option<AllocatorResourceBinding>; RESOURCES],
    resource_count: usize,
}

/// Builder for one allocator root.
#[derive(Debug)]
pub struct AllocatorBuilder<const DOMAINS: usize = 4, const RESOURCES: usize = 16> {
    policy: AllocPolicy,
    domains: [Option<AllocatorDomainInfo>; DOMAINS],
    domain_count: usize,
    resources: [Option<AllocatorResourceBinding>; RESOURCES],
    resource_count: usize,
    default_domain: Option<AllocatorDomainId>,
}

impl<const DOMAINS: usize, const RESOURCES: usize> Allocator<DOMAINS, RESOURCES> {
    /// Creates a default allocator root builder.
    #[must_use]
    pub fn builder() -> AllocatorBuilder<DOMAINS, RESOURCES> {
        AllocatorBuilder::new()
    }

    /// Creates a permissive zero-config allocator root.
    ///
    /// This currently shapes the allocator honestly but does not yet realize provider discovery
    /// or backing pools. Runtime allocation entry points therefore remain capability-gated.
    #[must_use]
    pub fn system_default() -> Self {
        let mut domains = array::from_fn(|_| None);
        if DOMAINS != 0 {
            domains[0] = Some(AllocatorDomainInfo::new(
                AllocatorDomainId(0),
                AllocatorDomainKind::Default,
                AllocPolicy::general_purpose(),
            ));
        }

        let policy = AllocPolicy::general_purpose();
        Self {
            policy,
            capabilities: allocator_capabilities_for_domains(&domains),
            hazards: allocator_hazards_for_domains(&domains),
            domains,
            domain_count: usize::from(DOMAINS != 0),
            resources: array::from_fn(|_| None),
            resource_count: 0,
        }
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
            .map(|binding| *binding.handle.info())
    }

    /// Returns observable information for one allocator domain.
    #[must_use]
    pub fn domain(&self, id: AllocatorDomainId) -> Option<AllocatorDomainInfo> {
        self.domains
            .iter()
            .flatten()
            .copied()
            .find(|domain| domain.id == id)
    }

    /// Returns the implicit default domain when one exists.
    #[must_use]
    pub fn default_domain(&self) -> Option<AllocatorDomainId> {
        self.domains
            .iter()
            .flatten()
            .find(|domain| domain.kind == AllocatorDomainKind::Default)
            .map(|domain| domain.id)
    }

    /// Returns a slab strategy view for `domain`.
    ///
    /// # Errors
    ///
    /// Returns an error when the domain does not exist, slab allocation is denied by policy, or
    /// the slab strategy remains unsupported on the current implementation path.
    pub fn slab<const SIZE: usize, const COUNT: usize>(
        &self,
        domain: AllocatorDomainId,
    ) -> Result<Slab<SIZE, COUNT>, AllocError> {
        let domain = self.domain(domain).ok_or_else(AllocError::invalid_domain)?;
        if !domain.policy.allows(AllocModeSet::SLAB) {
            return Err(AllocError::policy_denied());
        }
        Slab::for_domain(domain.id, domain.policy)
    }

    /// Returns a bounded-arena strategy view for `domain`.
    ///
    /// # Errors
    ///
    /// Returns an error when the domain does not exist, arena allocation is denied by policy, or
    /// the arena strategy remains unsupported on the current implementation path.
    pub fn arena(
        &self,
        domain: AllocatorDomainId,
        capacity: usize,
    ) -> Result<BoundedArena, AllocError> {
        let domain = self.domain(domain).ok_or_else(AllocError::invalid_domain)?;
        if !domain.policy.allows(AllocModeSet::ARENA) {
            return Err(AllocError::policy_denied());
        }
        BoundedArena::for_domain(domain.id, capacity, domain.policy)
    }

    /// Returns a general-purpose heap strategy view for `domain`.
    ///
    /// # Errors
    ///
    /// Returns an error when the domain does not exist, heap allocation is denied by policy, or
    /// the heap strategy remains unsupported on the current implementation path.
    pub fn heap(&self, domain: AllocatorDomainId) -> Result<HeapAllocator, AllocError> {
        let domain = self.domain(domain).ok_or_else(AllocError::invalid_domain)?;
        if !domain.policy.allows(AllocModeSet::HEAP) {
            return Err(AllocError::policy_denied());
        }
        HeapAllocator::for_domain(domain.id, domain.policy)
    }

    /// Attempts to allocate one heap-routed block from the allocator root.
    ///
    /// # Errors
    ///
    /// Returns an error when no default domain exists, heap allocation is denied by policy, or
    /// heap allocation remains unsupported on the current implementation path.
    pub fn malloc(&self, len: usize) -> Result<AllocResult, AllocError> {
        self.heap(
            self.default_domain()
                .ok_or_else(AllocError::invalid_domain)?,
        )?
        .allocate(&AllocRequest::new(len))
    }

    /// Attempts to allocate one zero-initialized heap-routed block from the allocator root.
    ///
    /// # Errors
    ///
    /// Returns an error when no default domain exists, heap allocation is denied by policy, or
    /// heap allocation remains unsupported on the current implementation path.
    pub fn calloc(&self, len: usize) -> Result<AllocResult, AllocError> {
        self.heap(
            self.default_domain()
                .ok_or_else(AllocError::invalid_domain)?,
        )?
        .allocate(&AllocRequest::zeroed(len))
    }

    /// Attempts to grow or shrink an existing heap-routed allocation.
    ///
    /// # Errors
    ///
    /// Returns an error when no default domain exists, heap allocation is denied by policy, the
    /// requested new length is invalid, or realloc remains unsupported on the current path.
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
    pub fn free(&self, allocation: AllocResult) -> Result<(), AllocError> {
        self.heap(
            self.default_domain()
                .ok_or_else(AllocError::invalid_domain)?,
        )?
        .deallocate(allocation)
    }
}

impl<const DOMAINS: usize, const RESOURCES: usize> Default for Allocator<DOMAINS, RESOURCES> {
    fn default() -> Self {
        Self::system_default()
    }
}

impl<const DOMAINS: usize, const RESOURCES: usize> AllocatorBuilder<DOMAINS, RESOURCES> {
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
    pub fn critical_safety(&mut self, required: CriticalSafetyRequirements) -> &mut Self {
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
    /// basic ownership validation.
    pub fn build(mut self) -> Result<Allocator<DOMAINS, RESOURCES>, AllocError> {
        if self.domain_count == 0 {
            self.ensure_default_domain()?;
        }

        Ok(Allocator {
            policy: self.policy,
            capabilities: allocator_capabilities_for_domains(&self.domains),
            hazards: allocator_hazards_for_domains(&self.domains),
            domains: self.domains,
            domain_count: self.domain_count,
            resources: self.resources,
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

impl<const DOMAINS: usize, const RESOURCES: usize> Default
    for AllocatorBuilder<DOMAINS, RESOURCES>
{
    fn default() -> Self {
        Self::new()
    }
}

fn allocator_capabilities_for_domains<const DOMAINS: usize>(
    domains: &[Option<AllocatorDomainInfo>; DOMAINS],
) -> AllocCapabilities {
    let mut capabilities = AllocCapabilities::empty();
    for domain in domains.iter().flatten() {
        if domain.policy.allows(AllocModeSet::SLAB) {
            capabilities = capabilities.union(AllocCapabilities::SLAB);
        }
        if domain.policy.allows(AllocModeSet::ARENA) {
            capabilities = capabilities.union(AllocCapabilities::ARENA);
        }
        if domain.policy.allows(AllocModeSet::HEAP) {
            capabilities = capabilities
                .union(AllocCapabilities::HEAP)
                .union(AllocCapabilities::ZEROED_ALLOC)
                .union(AllocCapabilities::REALLOC);
        }
        if domain.policy.allows(AllocModeSet::GLOBAL_ALLOC) {
            capabilities = capabilities.union(AllocCapabilities::GLOBAL_ALLOC);
        }
    }
    if !capabilities.contains(AllocCapabilities::HEAP) {
        capabilities = capabilities
            .union(AllocCapabilities::DETERMINISTIC)
            .union(AllocCapabilities::BOUNDED);
    }
    capabilities
}

fn allocator_hazards_for_domains<const DOMAINS: usize>(
    domains: &[Option<AllocatorDomainInfo>; DOMAINS],
) -> AllocHazards {
    for domain in domains.iter().flatten() {
        if domain.policy.allows(AllocModeSet::HEAP) {
            return HeapAllocator::expected_hazards();
        }
    }
    AllocHazards::empty()
}
