//! Core `fusion-sys::mem::resource` surface for governed contiguous memory ranges.
//!
//! This module defines the common `MemoryResource` contract and the concrete virtual-memory
//! acquisition path used when the selected fusion-pal backend can actively create a CPU-addressable
//! range on behalf of the caller. The important boundary is intentional:
//!
//! - `MemoryResource` is the common result type for a governed contiguous range.
//! - `VirtualMemoryResource` is one concrete acquisition strategy for virtual-memory backends.
//! - `BoundMemoryResource` is a different strategy for ranges that already exist and are
//!   governed externally.
//!
//! `VirtualMemoryResource::create` is therefore not meant to be the universal memory
//! acquisition API for every target. It is specifically the virtual-memory path. Today that
//! means requests shaped like ordinary user-space VM acquisition, such as anonymous or
//! file-backed mappings. If a future target has no useful virtual-memory model, this type
//! should simply not be used there.
//!
//! The intended higher-level architecture is that `MemoryProvider` sits above the concrete
//! resource constructors and selects the appropriate path using two inputs:
//!
//! - fusion-pal capability truth: what operations and acquisition modes the current platform backend
//!   can actually support.
//! - Topology or board truth: what memory physically exists on the machine, board, SoC, or
//!   firmware environment.
//!
//! On Linux or another hosted OS, that may lead to `VirtualMemoryResource::create`. On
//! bare-metal, RTOS, or firmware-driven targets, the provider may instead inventory static
//! SRAM windows, DMA carveouts, physical regions, or MMIO apertures and bind those through a
//! different resource constructor. The provider should expose resource semantics, not leak the
//! acquisition mechanism upward.
//!
//! This module also assumes that a `MemoryResource` names a CPU-addressable contiguous range.
//! The core region model is a `Region { base, len }`, so the resource contract is currently
//! aimed at memory that can be named by an address range in the current execution context.
//! That fits ordinary VM, fixed board-level carveouts, mapped physical ranges, and MMIO
//! windows. If future work needs to model non-addressable device-local heaps or opaque memory
//! objects, that should likely become a sibling abstraction rather than forcing this module to
//! lie about having a meaningful CPU-visible base address.
//!
//! Downstream callers are therefore given borrowed [`RangeView`] values rather than naked fusion-pal
//! [`Region`] descriptors as the primary safe surface. A raw address range is still the truth
//! underneath, but that truth is lifetime-bound to the owning resource and should not be
//! treated like a durable ownership token by higher layers.
//!
//! Live resources are also intended to support concurrent allocator and maintenance code. Their
//! summary state is published atomically, and backing-mutating operations serialize through a
//! small internal guard so background work cannot race resource-wide state transitions into
//! fiction.
//!
//! In short: keep `MemoryResource` as the common governed-range contract, keep
//! `VirtualMemoryResource` strictly VM-shaped, and let future provider layers unify multiple
//! concrete memory domains without pretending they all originate from the same create call.

use fusion_pal::sys::mem::{
    Address,
    Advise,
    Backing,
    CachePolicy,
    MapFlags,
    MapRequest,
    MemAdviceCaps,
    MemAdvise,
    MemBackingCaps,
    MemBase,
    MemCaps,
    MemCommit,
    MemLock,
    MemMap,
    MemPlacementCaps,
    MemProtect,
    MemQuery,
    MemSupport,
    PageInfo,
    Placement,
    Protect,
    Region,
    RegionAttrs,
    RegionInfo,
    system_mem,
};

mod attrs;
mod bound;
mod core;
mod domain;
mod error;
mod geometry;
mod handle;
mod layout;
mod ops;
mod range;
mod request;
mod reservation;
mod resolved;
mod state;
mod support;
mod view;

pub use attrs::ResourceAttrs;
pub use bound::{BoundMemoryResource, BoundResourceSpec};
pub use domain::{MemoryDomain, MemoryDomainSet, ResourceBackingKind};
pub use error::{ResourceError, ResourceErrorKind};
pub use geometry::MemoryGeometry;
pub use handle::MemoryResourceHandle;
pub use layout::{AllocatorLayoutPolicy, AllocatorLayoutRealization};
pub use ops::{ResourceHazardSet, ResourceOpSet, ResourcePreferenceSet};
pub use range::ResourceRange;
pub use request::{
    InitialResidency,
    InitialResourceState,
    IntegrityConstraints,
    OvercommitPolicy,
    PlacementPreference,
    RequiredPlacement,
    ResourceBackingRequest,
    ResourceContract,
    ResourceRequest,
    SharingPolicy,
};
pub use reservation::{
    AddressReservation,
    MaterializedReservation,
    ReservationHazardSet,
    ReservationOpSet,
    ReservationRequest,
    ReservationSupport,
    ResolvedAddressReservation,
};
pub use resolved::{ResolvedResource, ResourceInfo};
pub use state::{ResourceState, ResourceStateProvenance, StateValue};
pub use support::{
    ResourceAcquireSupport,
    ResourceFeatureSupport,
    ResourceResidencySupport,
    ResourceSupport,
};
pub use view::RangeView;

use self::core::ResourceCore;

/// Base contract for all contiguous governed memory resources.
pub trait MemoryResource {
    /// Returns immutable descriptive information for the resource instance.
    fn info(&self) -> &ResourceInfo;

    /// Returns the current resource-wide summary state.
    fn state(&self) -> ResourceState;

    /// Returns a borrowed view of the governed contiguous range.
    ///
    /// Raw address extraction stays tied to this borrow so downstream layers do not casually
    /// treat a copied `Region` descriptor as an ownership-bearing handle.
    fn range(&self) -> RangeView<'_> {
        RangeView::new(self.info().range)
    }

    /// Returns the length of the governed range.
    fn len(&self) -> usize {
        self.info().range.len
    }

    /// Returns `true` when the governed range is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the concrete backing kind represented by the resource.
    fn backing_kind(&self) -> ResourceBackingKind {
        self.info().backing
    }

    /// Returns the domain classification of the resource.
    fn domain(&self) -> MemoryDomain {
        self.info().domain
    }

    /// Returns intrinsic attributes of the resource's backing.
    fn attrs(&self) -> ResourceAttrs {
        self.info().attrs
    }

    /// Returns operation granularity information for the resource.
    fn geometry(&self) -> MemoryGeometry {
        self.info().geometry
    }

    /// Returns allocator-facing metadata and extent layout policy for the resource.
    fn layout(&self) -> AllocatorLayoutPolicy {
        self.info().layout
    }

    /// Returns the immutable lifetime contract of the resource.
    fn contract(&self) -> ResourceContract {
        self.info().contract
    }

    /// Returns the runtime support surface of this live resource instance.
    fn support(&self) -> ResourceSupport {
        self.info().support
    }

    /// Returns the legal operation set for the resource.
    fn ops(&self) -> ResourceOpSet {
        self.info().ops()
    }

    /// Returns inherent hazards associated with the resource.
    fn hazards(&self) -> ResourceHazardSet {
        self.info().hazards
    }

    /// Returns `true` when `ptr` lies within the governed range.
    fn contains(&self, ptr: *const u8) -> bool {
        self.range().contains(ptr)
    }

    /// Returns a checked borrowed subrange of the resource.
    ///
    /// # Errors
    /// Returns an error when the requested range is empty or falls outside the resource.
    fn subrange(&self, range: ResourceRange) -> Result<RangeView<'_>, ResourceError> {
        self.range().subrange(range)
    }
}

/// Extension trait for resources that can answer point queries.
pub trait QueryableResource: MemoryResource {
    /// Returns metadata about the region containing `addr`.
    ///
    /// # Errors
    /// Returns an error when querying is unsupported or `addr` is not valid for the resource.
    fn query(&self, addr: Address) -> Result<RegionInfo, ResourceError>;
}

/// Extension trait for resources that support protection changes.
pub trait ProtectableResource: MemoryResource {
    /// # Safety
    /// Caller must ensure the target range is not actively referenced in ways that would
    /// violate the requested protection change.
    ///
    /// # Errors
    /// Returns an error when the range is invalid, the requested protection is unsupported,
    /// or the change would violate the resource contract.
    unsafe fn protect(&self, range: ResourceRange, protect: Protect) -> Result<(), ResourceError>;
}

/// Extension trait for resources that accept advisory usage hints.
pub trait AdvisableResource: MemoryResource {
    /// # Safety
    /// Caller must ensure the target range is valid for the requested advisory update.
    ///
    /// # Errors
    /// Returns an error when the range is invalid or the advisory hint is unsupported.
    unsafe fn advise(&self, range: ResourceRange, advice: Advise) -> Result<(), ResourceError>;
}

/// Extension trait for resources that can semantically discard their contents.
pub trait DiscardableResource: MemoryResource {
    /// # Safety
    /// Caller must ensure discarding the target range is valid for the resource's backing
    /// and does not violate higher-level lifetime assumptions.
    ///
    /// # Errors
    /// Returns an error when discard is unsupported or the range is invalid.
    unsafe fn discard(&self, range: ResourceRange) -> Result<(), ResourceError>;
}

/// Extension trait for resources that support explicit flush-style operations.
pub trait FlushableResource: MemoryResource {
    /// # Safety
    /// Caller must ensure flushing the target range is meaningful for the resource's
    /// backing and ordering model.
    ///
    /// # Errors
    /// Returns an error when flush is unsupported or the range is invalid.
    unsafe fn flush(&self, range: ResourceRange) -> Result<(), ResourceError>;
}

/// Extension trait for resources that expose reserve/commit style backing control.
pub trait CommitControlledResource: MemoryResource {
    /// # Safety
    /// Caller must ensure committing the target range is valid for this resource's backing
    /// and contract.
    ///
    /// # Errors
    /// Returns an error when commit is unsupported, the range is invalid, or the request
    /// would violate the resource contract.
    unsafe fn commit(&self, range: ResourceRange, protect: Protect) -> Result<(), ResourceError>;

    /// # Safety
    /// Caller must ensure decommitting the target range does not invalidate live references.
    ///
    /// # Errors
    /// Returns an error when decommit is unsupported or the range is invalid.
    unsafe fn decommit(&self, range: ResourceRange) -> Result<(), ResourceError>;
}

/// Extension trait for resources that support residency locking.
pub trait LockableResource: MemoryResource {
    /// # Safety
    /// Caller must ensure locking the target range is legal in the current execution context.
    ///
    /// # Errors
    /// Returns an error when locking is unsupported or the range is invalid.
    unsafe fn lock(&self, range: ResourceRange) -> Result<(), ResourceError>;

    /// # Safety
    /// Caller must ensure the target range was previously locked in a compatible way.
    ///
    /// # Errors
    /// Returns an error when unlocking is unsupported or the range is invalid.
    unsafe fn unlock(&self, range: ResourceRange) -> Result<(), ResourceError>;
}

/// Concrete virtual-memory resource acquired from the current fusion-pal backend.
#[derive(Debug)]
pub struct VirtualMemoryResource {
    provider: fusion_pal::sys::mem::PlatformMem,
    region: Option<Region>,
    page_info: PageInfo,
    core: ResourceCore,
}

impl VirtualMemoryResource {
    /// Returns system-wide acquisition support for virtual memory resources on this backend.
    #[must_use]
    pub fn system_acquire_support() -> ResourceAcquireSupport {
        let provider = system_mem();
        resource_acquire_support_from_mem_support(provider.support())
    }

    /// Creates a new virtual memory resource from a request.
    ///
    /// # Errors
    /// Returns an error when the request is invalid, unsupported, violates its contract, or
    /// the backend cannot create the resource.
    #[allow(clippy::too_many_lines)]
    pub fn create(request: &ResourceRequest<'_>) -> Result<Self, ResourceError> {
        let provider = system_mem();
        let page_info = provider.page_info();
        let mem_support = provider.support();
        let acquire_support = resource_acquire_support_from_mem_support(mem_support);
        let resource_support = acquire_support.instance;
        let len = normalize_len(request.len, page_info.alloc_granule.get())?;

        validate_request(request, acquire_support)?;

        let base_flags = initial_map_flags(request, resource_support)?;
        let required_placement = required_placement_to_mem(
            request.required_placement,
            page_info.alloc_granule.get(),
            acquire_support.placements,
        )?;
        let (preferred_placement, preferred_unmet) = preferred_placement_to_mem(
            request.initial.placement,
            page_info.alloc_granule.get(),
            acquire_support.placements,
        )?;

        if required_placement.is_some()
            && !matches!(request.initial.placement, PlacementPreference::Anywhere)
        {
            return Err(ResourceError::invalid_request());
        }

        let backing = request_backing_to_mem(request.backing);
        let backing_kind = backing_kind_from_request(request.backing, request.contract.sharing);
        let required_request = base_map_request(
            len,
            request.initial.protect,
            base_flags,
            required_placement.unwrap_or(Placement::Anywhere),
            resource_attrs_for_request(request.backing),
            request.contract.cache_policy,
            backing,
        );

        let mut preferred_flags = base_flags;
        let mut unmet = ResourcePreferenceSet::empty();

        if preferred_unmet {
            unmet |= ResourcePreferenceSet::PLACEMENT;
        }

        if request
            .preferences
            .contains(ResourcePreferenceSet::PREFAULT)
            && !preferred_flags.contains(MapFlags::POPULATE)
        {
            if resource_support
                .residency
                .contains(ResourceResidencySupport::PREFAULT)
            {
                preferred_flags |= MapFlags::POPULATE;
            } else {
                unmet |= ResourcePreferenceSet::PREFAULT;
            }
        }

        if request
            .preferences
            .contains(ResourcePreferenceSet::HUGE_PAGES)
        {
            if supports_map_time_huge_pages(mem_support) {
                preferred_flags |= MapFlags::HUGE_PAGE;
            } else if !supports_huge_page_advice(mem_support) {
                unmet |= ResourcePreferenceSet::HUGE_PAGES;
            }
        }

        let preferred_request = base_map_request(
            len,
            request.initial.protect,
            preferred_flags,
            preferred_placement.unwrap_or(required_request.placement),
            resource_attrs_for_request(request.backing),
            request.contract.cache_policy,
            backing,
        );

        let has_preferred_attempt = preferred_request.flags != required_request.flags
            || preferred_request.placement != required_request.placement;

        let (region, actual_flags) = if has_preferred_attempt {
            if let Ok(region) = unsafe { provider.map(&preferred_request) } {
                if !placement_preference_honored(provider, request.initial.placement, region) {
                    unmet |= ResourcePreferenceSet::PLACEMENT;
                }
                (region, preferred_flags)
            } else {
                if preferred_flags.contains(MapFlags::POPULATE)
                    && !base_flags.contains(MapFlags::POPULATE)
                {
                    unmet |= ResourcePreferenceSet::PREFAULT;
                }
                if preferred_flags.contains(MapFlags::HUGE_PAGE)
                    && !base_flags.contains(MapFlags::HUGE_PAGE)
                {
                    unmet |= ResourcePreferenceSet::HUGE_PAGES;
                }
                if preferred_placement.is_some() {
                    unmet |= ResourcePreferenceSet::PLACEMENT;
                }

                (
                    unsafe { provider.map(&required_request) }
                        .map_err(ResourceError::from_request_error)?,
                    base_flags,
                )
            }
        } else {
            (
                unsafe { provider.map(&required_request) }
                    .map_err(ResourceError::from_request_error)?,
                base_flags,
            )
        };

        verify_required_placement(provider, region, request.required_placement)?;
        let actual_locked = match finalize_post_map_state(
            provider,
            region,
            request,
            mem_support,
            actual_flags,
            &mut unmet,
        ) {
            Ok(actual_locked) => actual_locked,
            Err(err) => {
                let _ = unsafe { provider.unmap(region) };
                return Err(err);
            }
        };
        let initial_state = ResourceState::tracked(request.initial.protect, actual_locked, true);

        Ok(Self::from_parts(
            provider,
            region,
            page_info,
            build_resolved_resource(
                region,
                request,
                resource_support,
                backing_kind,
                resource_attrs_from_request(request),
                geometry_from_page_info(page_info, resource_support.ops),
                allocator_layout_from_page_info(page_info),
                unmet,
                initial_state,
            ),
            initial_state,
        ))
    }

    /// Creates a virtual memory resource from already-resolved parts.
    pub(super) const fn from_parts(
        provider: fusion_pal::sys::mem::PlatformMem,
        region: Region,
        page_info: PageInfo,
        resolved: ResolvedResource,
        state: ResourceState,
    ) -> Self {
        Self {
            provider,
            region: Some(region),
            page_info,
            core: ResourceCore::new(resolved, state),
        }
    }

    /// Returns the page and granule information of the creating backend.
    #[must_use]
    pub const fn page_info(&self) -> PageInfo {
        self.page_info
    }

    /// Returns creation-time resolution metadata for the resource.
    #[must_use]
    pub const fn resolved(&self) -> ResolvedResource {
        self.core.resolved()
    }

    /// Returns a borrowed view of the resource's governed range.
    #[must_use]
    pub fn view(&self) -> RangeView<'_> {
        self.range()
    }

    /// Returns a checked borrowed subrange of the resource.
    ///
    /// # Errors
    /// Returns an error when the requested range is empty or falls outside the resource.
    pub fn subview(&self, range: ResourceRange) -> Result<RangeView<'_>, ResourceError> {
        self.subrange(range)
    }

    const fn raw_region(&self) -> Region {
        self.core.info().range
    }

    fn operation_region(&self, range: ResourceRange) -> Result<Region, ResourceError> {
        if range.len == 0 {
            return Err(ResourceError::invalid_range());
        }

        let page = self.page_info.base_page.get();
        if !range.offset.is_multiple_of(page) || !range.len.is_multiple_of(page) {
            return Err(ResourceError::invalid_range());
        }

        self.raw_region()
            .subrange(range.offset, range.len)
            .map_err(|_| ResourceError::invalid_range())
    }

    fn validate_protect_contract(&self, protect: Protect) -> Result<(), ResourceError> {
        if !supports_protect(self.support().protect, protect) {
            return Err(ResourceError::unsupported_operation());
        }

        if !self.contract().allowed_protect.contains(protect) {
            return Err(ResourceError::contract_violation());
        }

        if self.contract().write_xor_execute
            && protect.contains(Protect::WRITE)
            && protect.contains(Protect::EXEC)
        {
            return Err(ResourceError::contract_violation());
        }

        Ok(())
    }

    fn is_full_resource_range(&self, range: ResourceRange) -> bool {
        range.offset == 0 && range.len == self.len()
    }
}

impl MemoryResource for VirtualMemoryResource {
    fn info(&self) -> &ResourceInfo {
        self.core.info()
    }

    fn state(&self) -> ResourceState {
        self.core.state()
    }
}

impl ProtectableResource for VirtualMemoryResource {
    unsafe fn protect(&self, range: ResourceRange, protect: Protect) -> Result<(), ResourceError> {
        if !self.ops().contains(ResourceOpSet::PROTECT) {
            return Err(ResourceError::unsupported_operation());
        }

        self.validate_protect_contract(protect)?;
        let region = self.operation_region(range)?;
        let mutation = self
            .core
            .begin_mutation()
            .map_err(|error| ResourceError::from_sync_error(error.kind))?;
        unsafe { self.provider.protect(region, protect) }
            .map_err(ResourceError::from_operation_error)?;
        if self.is_full_resource_range(range) {
            mutation.set_current_protect(protect);
        } else {
            mutation.mark_protect_asymmetric();
        }
        Ok(())
    }
}

impl AdvisableResource for VirtualMemoryResource {
    unsafe fn advise(&self, range: ResourceRange, advice: Advise) -> Result<(), ResourceError> {
        if !self.ops().contains(ResourceOpSet::ADVISE)
            || !supports_advice(self.support().advice, advice)
        {
            return Err(ResourceError::unsupported_operation());
        }

        let region = self.operation_region(range)?;
        let _mutation = self
            .core
            .begin_mutation()
            .map_err(|error| ResourceError::from_sync_error(error.kind))?;
        unsafe { self.provider.advise(region, advice) }.map_err(ResourceError::from_operation_error)
    }
}

impl DiscardableResource for VirtualMemoryResource {
    unsafe fn discard(&self, range: ResourceRange) -> Result<(), ResourceError> {
        if !self.ops().contains(ResourceOpSet::DISCARD) {
            return Err(ResourceError::unsupported_operation());
        }

        let advice = preferred_discard_advice(self.support().advice)
            .ok_or_else(ResourceError::unsupported_operation)?;
        let region = self.operation_region(range)?;
        let _mutation = self
            .core
            .begin_mutation()
            .map_err(|error| ResourceError::from_sync_error(error.kind))?;
        unsafe { self.provider.advise(region, advice) }.map_err(ResourceError::from_operation_error)
    }
}

impl CommitControlledResource for VirtualMemoryResource {
    unsafe fn commit(&self, range: ResourceRange, protect: Protect) -> Result<(), ResourceError> {
        if !self.ops().contains(ResourceOpSet::COMMIT) {
            return Err(ResourceError::unsupported_operation());
        }

        self.validate_protect_contract(protect)?;
        let region = self.operation_region(range)?;
        let mutation = self
            .core
            .begin_mutation()
            .map_err(|error| ResourceError::from_sync_error(error.kind))?;
        unsafe { self.provider.commit(region, protect) }
            .map_err(ResourceError::from_operation_error)?;
        if self.is_full_resource_range(range) {
            mutation.set_current_protect(protect);
            mutation.set_committed_state(true);
        } else {
            mutation.mark_protect_asymmetric();
            mutation.mark_committed_asymmetric();
        }
        Ok(())
    }

    unsafe fn decommit(&self, range: ResourceRange) -> Result<(), ResourceError> {
        if !self.ops().contains(ResourceOpSet::DECOMMIT) {
            return Err(ResourceError::unsupported_operation());
        }

        let region = self.operation_region(range)?;
        let mutation = self
            .core
            .begin_mutation()
            .map_err(|error| ResourceError::from_sync_error(error.kind))?;
        unsafe { self.provider.decommit(region) }.map_err(ResourceError::from_operation_error)?;
        if self.is_full_resource_range(range) {
            mutation.set_committed_state(false);
        } else {
            mutation.mark_committed_asymmetric();
        }
        Ok(())
    }
}

impl LockableResource for VirtualMemoryResource {
    unsafe fn lock(&self, range: ResourceRange) -> Result<(), ResourceError> {
        if !self.ops().contains(ResourceOpSet::LOCK) {
            return Err(ResourceError::unsupported_operation());
        }

        let region = self.operation_region(range)?;
        let mutation = self
            .core
            .begin_mutation()
            .map_err(|error| ResourceError::from_sync_error(error.kind))?;
        unsafe { self.provider.lock(region) }.map_err(ResourceError::from_operation_error)?;
        if self.is_full_resource_range(range) {
            mutation.set_locked_state(true);
        } else {
            mutation.mark_locked_asymmetric();
        }
        Ok(())
    }

    unsafe fn unlock(&self, range: ResourceRange) -> Result<(), ResourceError> {
        if !self.ops().contains(ResourceOpSet::LOCK) {
            return Err(ResourceError::unsupported_operation());
        }

        let region = self.operation_region(range)?;
        let mutation = self
            .core
            .begin_mutation()
            .map_err(|error| ResourceError::from_sync_error(error.kind))?;
        unsafe { self.provider.unlock(region) }.map_err(ResourceError::from_operation_error)?;
        if self.is_full_resource_range(range) {
            mutation.set_locked_state(false);
        } else {
            mutation.mark_locked_asymmetric();
        }
        Ok(())
    }
}

impl QueryableResource for VirtualMemoryResource {
    fn query(&self, addr: Address) -> Result<RegionInfo, ResourceError> {
        if !self.ops().contains(ResourceOpSet::QUERY) {
            return Err(ResourceError::unsupported_operation());
        }

        if !self.range().contains_addr(addr) {
            return Err(ResourceError::invalid_range());
        }

        self.provider
            .query(addr)
            .map_err(ResourceError::from_operation_error)
    }
}

impl Drop for VirtualMemoryResource {
    fn drop(&mut self) {
        if let Some(region) = self.region.take() {
            let _ = unsafe { self.provider.unmap(region) };
        }
    }
}

pub(super) fn validate_request(
    request: &ResourceRequest<'_>,
    support: ResourceAcquireSupport,
) -> Result<(), ResourceError> {
    if request.len == 0 {
        return Err(ResourceError::invalid_request());
    }

    if !supports_backing(request.backing, request.contract.sharing, support.backings) {
        return Err(ResourceError::unsupported_request());
    }

    if !supports_protect(support.instance.protect, request.initial.protect)
        || !supports_protect(support.instance.protect, request.contract.allowed_protect)
    {
        return Err(ResourceError::unsupported_request());
    }

    if !request
        .contract
        .allowed_protect
        .contains(request.initial.protect)
    {
        return Err(ResourceError::contract_violation());
    }

    if request.contract.write_xor_execute
        && request.initial.protect.contains(Protect::WRITE)
        && request.initial.protect.contains(Protect::EXEC)
    {
        return Err(ResourceError::contract_violation());
    }

    if request.contract.allowed_protect != request.initial.protect
        && !support.instance.ops.contains(ResourceOpSet::PROTECT)
    {
        return Err(ResourceError::unsupported_request());
    }

    if matches!(request.contract.overcommit, OvercommitPolicy::Disallow)
        && !support
            .features
            .contains(ResourceFeatureSupport::OVERCOMMIT_DISALLOW)
    {
        return Err(ResourceError::unsupported_request());
    }

    if request.contract.cache_policy != CachePolicy::Default
        && !support
            .features
            .contains(ResourceFeatureSupport::CACHE_POLICY)
    {
        return Err(ResourceError::unsupported_request());
    }

    if request.contract.integrity.is_some()
        && !support.features.contains(ResourceFeatureSupport::INTEGRITY)
    {
        return Err(ResourceError::unsupported_request());
    }

    match request.initial.residency {
        InitialResidency::BestEffort => {}
        InitialResidency::Prefault
            if support
                .instance
                .residency
                .contains(ResourceResidencySupport::PREFAULT) => {}
        InitialResidency::Locked
            if support
                .instance
                .residency
                .contains(ResourceResidencySupport::LOCKED) => {}
        InitialResidency::Prefault | InitialResidency::Locked => {
            return Err(ResourceError::unsupported_request());
        }
    }

    Ok(())
}

pub(super) fn normalize_len(len: usize, granule: usize) -> Result<usize, ResourceError> {
    if len == 0 {
        return Err(ResourceError::invalid_request());
    }

    align_up(len, granule).ok_or_else(ResourceError::invalid_request)
}

const fn supports_backing(
    backing: ResourceBackingRequest<'_>,
    sharing: SharingPolicy,
    support: MemBackingCaps,
) -> bool {
    match (backing, sharing) {
        (ResourceBackingRequest::Anonymous, SharingPolicy::Private) => {
            support.contains(MemBackingCaps::ANON_PRIVATE)
        }
        (ResourceBackingRequest::Anonymous, SharingPolicy::Shared) => {
            support.contains(MemBackingCaps::ANON_SHARED)
        }
        (ResourceBackingRequest::File { .. }, SharingPolicy::Private) => {
            support.contains(MemBackingCaps::FILE_PRIVATE)
        }
        (ResourceBackingRequest::File { .. }, SharingPolicy::Shared) => {
            support.contains(MemBackingCaps::FILE_SHARED)
        }
    }
}

fn supports_protect(supported: Protect, requested: Protect) -> bool {
    if requested.contains(Protect::GUARD) && !supported.contains(Protect::GUARD) {
        return false;
    }

    supported.contains(requested - Protect::GUARD)
}

const fn supports_advice(supported: MemAdviceCaps, advice: Advise) -> bool {
    match advice {
        Advise::Normal => supported.contains(MemAdviceCaps::NORMAL),
        Advise::Sequential => supported.contains(MemAdviceCaps::SEQUENTIAL),
        Advise::Random => supported.contains(MemAdviceCaps::RANDOM),
        Advise::WillNeed => supported.contains(MemAdviceCaps::WILL_NEED),
        Advise::DontNeed => supported.contains(MemAdviceCaps::DONT_NEED),
        Advise::Free => supported.contains(MemAdviceCaps::FREE),
        Advise::NoHugePage => supported.contains(MemAdviceCaps::NO_HUGE_PAGE),
        Advise::HugePage => supported.contains(MemAdviceCaps::HUGE_PAGE),
    }
}

const fn preferred_discard_advice(supported: MemAdviceCaps) -> Option<Advise> {
    if supported.contains(MemAdviceCaps::FREE) {
        Some(Advise::Free)
    } else if supported.contains(MemAdviceCaps::DONT_NEED) {
        Some(Advise::DontNeed)
    } else {
        None
    }
}

pub(super) fn initial_map_flags(
    request: &ResourceRequest<'_>,
    support: ResourceSupport,
) -> Result<MapFlags, ResourceError> {
    let mut flags = match request.contract.sharing {
        SharingPolicy::Private => MapFlags::PRIVATE,
        SharingPolicy::Shared => MapFlags::SHARED,
    };

    match request.initial.residency {
        InitialResidency::BestEffort => {}
        InitialResidency::Prefault => {
            if !support
                .residency
                .contains(ResourceResidencySupport::PREFAULT)
            {
                return Err(ResourceError::unsupported_request());
            }
            flags |= MapFlags::POPULATE;
        }
        InitialResidency::Locked => {
            if !support.residency.contains(ResourceResidencySupport::LOCKED) {
                return Err(ResourceError::unsupported_request());
            }
        }
    }

    Ok(flags)
}

pub(super) const fn request_backing_to_mem(backing: ResourceBackingRequest<'_>) -> Backing<'_> {
    match backing {
        ResourceBackingRequest::Anonymous => Backing::Anonymous,
        ResourceBackingRequest::File { fd, offset } => Backing::File { fd, offset },
    }
}

pub(super) const fn backing_kind_from_request(
    backing: ResourceBackingRequest<'_>,
    sharing: SharingPolicy,
) -> ResourceBackingKind {
    match (backing, sharing) {
        (ResourceBackingRequest::Anonymous, SharingPolicy::Private) => {
            ResourceBackingKind::AnonymousPrivate
        }
        (ResourceBackingRequest::Anonymous, SharingPolicy::Shared) => {
            ResourceBackingKind::AnonymousShared
        }
        (ResourceBackingRequest::File { .. }, SharingPolicy::Private) => {
            ResourceBackingKind::FilePrivate
        }
        (ResourceBackingRequest::File { .. }, SharingPolicy::Shared) => {
            ResourceBackingKind::FileShared
        }
    }
}

pub(super) const fn resource_attrs_for_request(
    _backing: ResourceBackingRequest<'_>,
) -> RegionAttrs {
    RegionAttrs::VIRTUAL_ONLY
}

pub(super) fn resource_attrs_from_request(request: &ResourceRequest<'_>) -> ResourceAttrs {
    let mut attrs = ResourceAttrs::ALLOCATABLE | ResourceAttrs::CACHEABLE | ResourceAttrs::COHERENT;

    if request.contract.integrity.is_some() {
        attrs |= ResourceAttrs::INTEGRITY_MANAGED;
    }
    if request
        .contract
        .integrity
        .is_some_and(|constraints| constraints.tag.is_some())
    {
        attrs |= ResourceAttrs::TAGGED;
    }

    attrs
}

pub(super) fn resource_region_attrs_from_attrs(attrs: ResourceAttrs) -> RegionAttrs {
    let mut region_attrs = RegionAttrs::empty();

    if attrs.contains(ResourceAttrs::DMA_VISIBLE) {
        region_attrs |= RegionAttrs::DMA_VISIBLE;
    }
    if attrs.contains(ResourceAttrs::DEVICE_LOCAL) {
        region_attrs |= RegionAttrs::DEVICE_LOCAL;
    }
    if attrs.contains(ResourceAttrs::CACHEABLE) {
        region_attrs |= RegionAttrs::CACHEABLE;
    }
    if attrs.contains(ResourceAttrs::COHERENT) {
        region_attrs |= RegionAttrs::COHERENT;
    }
    if attrs.contains(ResourceAttrs::PHYS_CONTIGUOUS) {
        region_attrs |= RegionAttrs::PHYS_CONTIGUOUS;
    }
    if attrs.contains(ResourceAttrs::TAGGED) {
        region_attrs |= RegionAttrs::TAGGED;
    }
    if attrs.contains(ResourceAttrs::INTEGRITY_MANAGED) {
        region_attrs |= RegionAttrs::INTEGRITY_MANAGED;
    }
    if attrs.contains(ResourceAttrs::STATIC_REGION) {
        region_attrs |= RegionAttrs::STATIC_REGION;
    }

    region_attrs
}

pub(super) const fn required_placement_to_mem(
    placement: Option<RequiredPlacement>,
    granule: usize,
    supported: MemPlacementCaps,
) -> Result<Option<Placement>, ResourceError> {
    match placement {
        None => Ok(None),
        Some(RequiredPlacement::FixedNoReplace(addr)) => {
            if !addr.is_multiple_of(granule) {
                return Err(ResourceError::invalid_request());
            }
            if !supported.contains(MemPlacementCaps::FIXED_NOREPLACE) {
                return Err(ResourceError::unsupported_request());
            }
            Ok(Some(Placement::FixedNoReplace(addr)))
        }
        Some(RequiredPlacement::RequiredNode(node)) => {
            if !supported.contains(MemPlacementCaps::REQUIRED_NODE) {
                return Err(ResourceError::unsupported_request());
            }
            Ok(Some(Placement::RequiredNode(node)))
        }
        Some(RequiredPlacement::RegionId(region_id)) => {
            if !supported.contains(MemPlacementCaps::REGION_ID) {
                return Err(ResourceError::unsupported_request());
            }
            Ok(Some(Placement::RegionId(region_id)))
        }
    }
}

pub(super) const fn preferred_placement_to_mem(
    placement: PlacementPreference,
    granule: usize,
    supported: MemPlacementCaps,
) -> Result<(Option<Placement>, bool), ResourceError> {
    match placement {
        PlacementPreference::Anywhere => Ok((None, false)),
        PlacementPreference::Hint(addr) => {
            if !addr.is_multiple_of(granule) {
                return Err(ResourceError::invalid_request());
            }
            if supported.contains(MemPlacementCaps::HINT) {
                Ok((Some(Placement::Hint(addr)), false))
            } else {
                Ok((None, true))
            }
        }
        PlacementPreference::PreferredNode(node) => {
            if supported.contains(MemPlacementCaps::PREFERRED_NODE) {
                Ok((Some(Placement::PreferredNode(node)), false))
            } else {
                Ok((None, true))
            }
        }
    }
}

const fn base_map_request(
    len: usize,
    protect: Protect,
    flags: MapFlags,
    placement: Placement,
    attrs: RegionAttrs,
    cache: CachePolicy,
    backing: Backing<'_>,
) -> MapRequest<'_> {
    MapRequest {
        len,
        align: 0,
        protect,
        flags,
        attrs,
        cache,
        placement,
        backing,
    }
}

pub(super) fn placement_preference_honored(
    provider: fusion_pal::sys::mem::PlatformMem,
    preference: PlacementPreference,
    region: Region,
) -> bool {
    match preference {
        PlacementPreference::Anywhere => true,
        PlacementPreference::Hint(addr) => region.base.get() == addr,
        PlacementPreference::PreferredNode(node) => provider
            .query(region.base)
            .is_ok_and(|info| placement_matches_node(info.placement, node)),
    }
}

pub(super) fn verify_required_placement(
    provider: fusion_pal::sys::mem::PlatformMem,
    region: Region,
    placement: Option<RequiredPlacement>,
) -> Result<(), ResourceError> {
    match placement {
        None => Ok(()),
        Some(RequiredPlacement::FixedNoReplace(addr)) if region.base.get() == addr => Ok(()),
        Some(RequiredPlacement::FixedNoReplace(_)) => Err(ResourceError::unsupported_request()),
        Some(RequiredPlacement::RequiredNode(node)) => {
            provider.query(region.base).map_or(Ok(()), |info| {
                if placement_contradicts_node(info.placement, node) {
                    Err(ResourceError::unsupported_request())
                } else {
                    Ok(())
                }
            })
        }
        Some(RequiredPlacement::RegionId(region_id)) => {
            provider.query(region.base).map_or(Ok(()), |info| {
                if placement_contradicts_region_id(info.placement, region_id) {
                    Err(ResourceError::unsupported_request())
                } else {
                    Ok(())
                }
            })
        }
    }
}

pub(super) fn resource_instance_support_from_mem_support(
    mem_support: MemSupport,
) -> ResourceSupport {
    let mut ops = ResourceOpSet::empty();
    if mem_support.caps.contains(MemCaps::PROTECT) {
        ops |= ResourceOpSet::PROTECT;
    }
    if mem_support.caps.contains(MemCaps::ADVISE) && !mem_support.advice.is_empty() {
        ops |= ResourceOpSet::ADVISE;
    }
    if mem_support.caps.contains(MemCaps::LOCK) {
        ops |= ResourceOpSet::LOCK;
    }
    if mem_support.caps.contains(MemCaps::QUERY) {
        ops |= ResourceOpSet::QUERY;
    }
    if mem_support.caps.contains(MemCaps::COMMIT_CONTROL) {
        ops |= ResourceOpSet::COMMIT;
    }
    if mem_support.caps.contains(MemCaps::DECOMMIT_CONTROL) {
        ops |= ResourceOpSet::DECOMMIT;
    }
    if preferred_discard_advice(mem_support.advice).is_some() {
        ops |= ResourceOpSet::DISCARD;
    }

    let mut residency = ResourceResidencySupport::BEST_EFFORT;
    if mem_support.map_flags.contains(MapFlags::POPULATE) {
        residency |= ResourceResidencySupport::PREFAULT;
    }
    if mem_support.caps.contains(MemCaps::LOCK) {
        residency |= ResourceResidencySupport::LOCKED;
    }

    ResourceSupport {
        protect: mem_support.protect,
        ops,
        advice: mem_support.advice,
        residency,
    }
}

pub(super) fn resource_acquire_support_from_mem_support(
    mem_support: MemSupport,
) -> ResourceAcquireSupport {
    let mut features = ResourceFeatureSupport::empty();
    if mem_support.caps.contains(MemCaps::CACHE_POLICY) {
        features |= ResourceFeatureSupport::CACHE_POLICY;
    }
    if mem_support.caps.contains(MemCaps::INTEGRITY_CONTROL) {
        features |= ResourceFeatureSupport::INTEGRITY;
    }

    let mut preferences = ResourcePreferenceSet::empty();
    if mem_support.placements.contains(MemPlacementCaps::HINT)
        || mem_support
            .placements
            .contains(MemPlacementCaps::PREFERRED_NODE)
    {
        preferences |= ResourcePreferenceSet::PLACEMENT;
    }
    if mem_support.map_flags.contains(MapFlags::POPULATE) {
        preferences |= ResourcePreferenceSet::PREFAULT;
    }
    if mem_support.caps.contains(MemCaps::LOCK) {
        preferences |= ResourcePreferenceSet::LOCK;
    }
    if mem_support.map_flags.contains(MapFlags::HUGE_PAGE)
        || mem_support.advice.contains(MemAdviceCaps::HUGE_PAGE)
    {
        preferences |= ResourcePreferenceSet::HUGE_PAGES;
    }

    ResourceAcquireSupport {
        domains: resource_domains_from_mem_support(mem_support),
        backings: mem_support.backings,
        placements: mem_support.placements,
        instance: resource_instance_support_from_mem_support(mem_support),
        features,
        preferences,
    }
}

pub(super) fn finalize_post_map_state(
    provider: fusion_pal::sys::mem::PlatformMem,
    region: Region,
    request: &ResourceRequest<'_>,
    mem_support: MemSupport,
    actual_flags: MapFlags,
    unmet: &mut ResourcePreferenceSet,
) -> Result<bool, ResourceError> {
    let mut actual_locked = false;

    if matches!(request.initial.residency, InitialResidency::Locked) {
        unsafe { provider.lock(region) }.map_err(ResourceError::from_operation_error)?;
        actual_locked = true;
    } else if request.preferences.contains(ResourcePreferenceSet::LOCK) {
        if mem_support.caps.contains(MemCaps::LOCK) {
            if unsafe { provider.lock(region) }.is_ok() {
                actual_locked = true;
            } else {
                *unmet |= ResourcePreferenceSet::LOCK;
            }
        } else {
            *unmet |= ResourcePreferenceSet::LOCK;
        }
    }

    if request
        .preferences
        .contains(ResourcePreferenceSet::HUGE_PAGES)
        && !actual_flags.contains(MapFlags::HUGE_PAGE)
    {
        if supports_huge_page_advice(mem_support) {
            if unsafe { provider.advise(region, Advise::HugePage) }.is_err() {
                *unmet |= ResourcePreferenceSet::HUGE_PAGES;
            }
        } else {
            *unmet |= ResourcePreferenceSet::HUGE_PAGES;
        }
    }

    Ok(actual_locked)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_resolved_resource(
    range: Region,
    request: &ResourceRequest<'_>,
    support: ResourceSupport,
    backing: ResourceBackingKind,
    attrs: ResourceAttrs,
    geometry: MemoryGeometry,
    layout: AllocatorLayoutPolicy,
    unmet_preferences: ResourcePreferenceSet,
    initial_state: ResourceState,
) -> ResolvedResource {
    ResolvedResource {
        info: ResourceInfo {
            range,
            domain: MemoryDomain::VirtualAddressSpace,
            backing,
            attrs,
            geometry,
            layout,
            contract: request.contract,
            support,
            hazards: infer_resource_hazards(request.contract, attrs),
        },
        initial_state,
        unmet_preferences,
    }
}

pub(super) const fn allocator_layout_from_page_info(page_info: PageInfo) -> AllocatorLayoutPolicy {
    AllocatorLayoutPolicy::hosted_vm(page_info.alloc_granule)
}

pub(super) fn infer_resource_hazards(
    contract: ResourceContract,
    attrs: ResourceAttrs,
) -> ResourceHazardSet {
    let mut hazards = ResourceHazardSet::empty();
    if contract.allowed_protect.contains(Protect::EXEC) {
        hazards |= ResourceHazardSet::EXECUTABLE;
    }
    if matches!(contract.sharing, SharingPolicy::Shared) {
        hazards |= ResourceHazardSet::SHARED_ALIASING;
    }
    if matches!(contract.overcommit, OvercommitPolicy::Allow) {
        hazards |= ResourceHazardSet::OVERCOMMIT;
    }
    if !attrs.contains(ResourceAttrs::COHERENT) {
        hazards |= ResourceHazardSet::NON_COHERENT;
    }
    if attrs.contains(ResourceAttrs::HAZARDOUS_IO) {
        hazards |= ResourceHazardSet::MMIO_SIDE_EFFECTS;
    }
    if attrs.contains(ResourceAttrs::PERSISTENT) {
        hazards |= ResourceHazardSet::PERSISTENCE_REQUIRES_FLUSH;
    }

    hazards
}

fn resource_domains_from_mem_support(mem_support: MemSupport) -> MemoryDomainSet {
    let mut domains = MemoryDomainSet::empty();
    if !mem_support.backings.is_empty() {
        domains |= MemoryDomainSet::VIRTUAL_ADDRESS_SPACE;
    }
    if mem_support.backings.contains(MemBackingCaps::DEVICE) {
        domains |= MemoryDomainSet::DEVICE_LOCAL;
    }
    if mem_support.backings.contains(MemBackingCaps::PHYSICAL) {
        domains |= MemoryDomainSet::PHYSICAL;
    }
    if mem_support.backings.contains(MemBackingCaps::BORROWED) {
        domains |= MemoryDomainSet::STATIC_REGION;
    }
    domains
}

pub(super) fn geometry_from_page_info(page_info: PageInfo, ops: ResourceOpSet) -> MemoryGeometry {
    MemoryGeometry {
        base_granule: page_info.base_page,
        alloc_granule: page_info.alloc_granule,
        protect_granule: ops
            .contains(ResourceOpSet::PROTECT)
            .then_some(page_info.base_page),
        commit_granule: (ops.contains(ResourceOpSet::COMMIT)
            || ops.contains(ResourceOpSet::DECOMMIT))
        .then_some(page_info.base_page),
        lock_granule: ops
            .contains(ResourceOpSet::LOCK)
            .then_some(page_info.base_page),
        large_granule: page_info.huge_page,
    }
}

fn align_up(value: usize, align: usize) -> Option<usize> {
    if align == 0 {
        return Some(value);
    }
    if !align.is_power_of_two() {
        return None;
    }

    let mask = align.checked_sub(1)?;
    value.checked_add(mask).map(|rounded| rounded & !mask)
}

pub(super) const fn supports_map_time_huge_pages(mem_support: MemSupport) -> bool {
    mem_support.caps.contains(MemCaps::HUGE_PAGES)
        && mem_support.map_flags.contains(MapFlags::HUGE_PAGE)
}

pub(super) const fn supports_huge_page_advice(mem_support: MemSupport) -> bool {
    mem_support.caps.contains(MemCaps::ADVISE)
        && mem_support.advice.contains(MemAdviceCaps::HUGE_PAGE)
}

const fn placement_matches_node(placement: Placement, node: u32) -> bool {
    match placement {
        Placement::PreferredNode(actual) | Placement::RequiredNode(actual) => actual == node,
        Placement::Anywhere
        | Placement::Hint(_)
        | Placement::FixedNoReplace(_)
        | Placement::RegionId(_) => false,
    }
}

const fn placement_contradicts_node(placement: Placement, node: u32) -> bool {
    match placement {
        Placement::PreferredNode(actual) | Placement::RequiredNode(actual) => actual != node,
        Placement::Anywhere
        | Placement::Hint(_)
        | Placement::FixedNoReplace(_)
        | Placement::RegionId(_) => false,
    }
}

const fn placement_contradicts_region_id(placement: Placement, region_id: u64) -> bool {
    match placement {
        Placement::RegionId(actual) => actual != region_id,
        Placement::Anywhere
        | Placement::Hint(_)
        | Placement::FixedNoReplace(_)
        | Placement::PreferredNode(_)
        | Placement::RequiredNode(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_linux_virtual_support_honestly() {
        let support = VirtualMemoryResource::system_acquire_support();

        assert!(support.backings.contains(MemBackingCaps::ANON_PRIVATE));
        assert!(support.backings.contains(MemBackingCaps::ANON_SHARED));
        assert!(support.backings.contains(MemBackingCaps::FILE_PRIVATE));
        assert!(support.backings.contains(MemBackingCaps::FILE_SHARED));
        assert!(support.placements.contains(MemPlacementCaps::HINT));
        assert!(support.instance.ops.contains(ResourceOpSet::ADVISE));
        assert_eq!(
            support.instance.ops.contains(ResourceOpSet::QUERY),
            system_mem().support().caps.contains(MemCaps::QUERY)
        );
    }

    #[test]
    fn creates_default_virtual_resource() {
        let request = ResourceRequest::anonymous_private(16 * 1024);
        let resource = VirtualMemoryResource::create(&request).expect("resource");

        assert_eq!(resource.domain(), MemoryDomain::VirtualAddressSpace);
        assert_eq!(
            resource.backing_kind(),
            ResourceBackingKind::AnonymousPrivate
        );
        assert_eq!(
            resource.resolved().info.backing,
            ResourceBackingKind::AnonymousPrivate
        );
        assert_eq!(resource.len(), 16 * 1024);
        assert!(resource.attrs().contains(
            ResourceAttrs::ALLOCATABLE | ResourceAttrs::CACHEABLE | ResourceAttrs::COHERENT
        ));
        assert!(resource.ops().contains(ResourceOpSet::PROTECT));
        assert!(resource.ops().contains(ResourceOpSet::ADVISE));
        assert!(resource.ops().contains(ResourceOpSet::DISCARD));
        assert!(resource.ops().contains(ResourceOpSet::LOCK));
        assert_eq!(
            resource.geometry().base_granule,
            resource.page_info().base_page
        );
        assert_eq!(
            resource.state(),
            ResourceState::tracked(Protect::READ | Protect::WRITE, false, true)
        );
    }

    #[test]
    fn creates_shared_virtual_resource() {
        let request = ResourceRequest::anonymous_shared(16 * 1024);
        let resource = VirtualMemoryResource::create(&request).expect("resource");

        assert_eq!(
            resource.resolved().info.backing,
            ResourceBackingKind::AnonymousShared
        );
        assert!(resource.hazards().contains(ResourceHazardSet::SHARED));
    }

    #[test]
    fn queries_resource_region_snapshot() {
        let request = ResourceRequest::anonymous_private(16 * 1024);
        let resource = VirtualMemoryResource::create(&request).expect("resource");
        let base = resource.view().base_addr();
        if resource.ops().contains(ResourceOpSet::QUERY) {
            let info = resource.query(base).expect("query");
            assert!(info.region.contains(base.get()));
            assert!(info.region.len >= resource.len());
            assert!(info.protect.contains(Protect::READ));
            assert!(info.protect.contains(Protect::WRITE));
        } else {
            let err = resource.query(base).expect_err("query should be unsupported");
            assert_eq!(err.kind, ResourceErrorKind::UnsupportedOperation);
        }
    }

    #[test]
    fn query_rejects_foreign_resource_address() {
        let request = ResourceRequest::anonymous_private(16 * 1024);
        let first = VirtualMemoryResource::create(&request).expect("first resource");
        let second = VirtualMemoryResource::create(&request).expect("second resource");
        let second_base = second.view().base_addr();

        let err = first
            .query(second_base)
            .expect_err("foreign address should be rejected");

        if first.ops().contains(ResourceOpSet::QUERY) {
            assert_eq!(err.kind, ResourceErrorKind::InvalidRange);
        } else {
            assert_eq!(err.kind, ResourceErrorKind::UnsupportedOperation);
        }
    }

    #[test]
    fn creates_address_reservation_and_materializes_private_resource() {
        let reservation =
            AddressReservation::create(&ReservationRequest::new(16 * 1024)).expect("reservation");
        let request = ResourceRequest::anonymous_private(16 * 1024);
        let resource = reservation.into_resource(&request).expect("resource");

        assert_eq!(
            resource.resolved().info.backing,
            ResourceBackingKind::AnonymousPrivate
        );
        assert_eq!(resource.len(), 16 * 1024);
    }

    #[test]
    fn reservation_materializes_shared_resource_by_replacement() {
        let reservation =
            AddressReservation::create(&ReservationRequest::new(16 * 1024)).expect("reservation");
        let request = ResourceRequest::anonymous_shared(16 * 1024);
        let resource = reservation.into_resource(&request).expect("resource");

        assert_eq!(
            resource.resolved().info.backing,
            ResourceBackingKind::AnonymousShared
        );
        assert!(resource.hazards().contains(ResourceHazardSet::SHARED));
    }

    #[test]
    fn reservation_materializes_subrange_and_returns_remainders() {
        let probe = AddressReservation::create(&ReservationRequest::new(16 * 1024))
            .expect("probe reservation");
        let page = probe.page_info().base_page.get();
        drop(probe);

        let reservation =
            AddressReservation::create(&ReservationRequest::new(page * 3)).expect("reservation");
        let request = ResourceRequest::anonymous_private(page);
        let materialized = reservation
            .materialize_range(ResourceRange::new(page, page), &request)
            .expect("materialized");

        let leading = materialized.leading.expect("leading reservation");
        let trailing = materialized.trailing.expect("trailing reservation");
        let resource = materialized.resource;

        assert_eq!(leading.view().len(), page);
        assert_eq!(resource.len(), page);
        assert_eq!(trailing.view().len(), page);
        assert_eq!(
            leading.view().checked_end_addr(),
            Some(resource.view().base_addr().get())
        );
        assert_eq!(
            resource.view().checked_end_addr(),
            Some(trailing.view().base_addr().get())
        );
    }

    #[test]
    fn reservation_query_rejects_foreign_address() {
        let first = AddressReservation::create(&ReservationRequest::new(16 * 1024)).expect("first");
        let second =
            AddressReservation::create(&ReservationRequest::new(16 * 1024)).expect("second");
        let second_base = second.view().base_addr();

        let err = first
            .query(second_base)
            .expect_err("foreign address should be rejected");

        if first.support().ops.contains(ReservationOpSet::QUERY) {
            assert_eq!(err.kind, ResourceErrorKind::InvalidRange);
        } else {
            assert_eq!(err.kind, ResourceErrorKind::UnsupportedOperation);
        }
    }

    #[test]
    fn bound_memory_resource_represents_non_vm_region_honestly() {
        let request = ResourceRequest::anonymous_private(16 * 1024);
        let resource = VirtualMemoryResource::create(&request).expect("resource");
        let support = ResourceSupport {
            protect: Protect::READ | Protect::WRITE,
            ops: ResourceOpSet::QUERY,
            advice: MemAdviceCaps::empty(),
            residency: ResourceResidencySupport::empty(),
        };
        let contract = ResourceContract {
            allowed_protect: Protect::READ | Protect::WRITE,
            write_xor_execute: true,
            sharing: SharingPolicy::Private,
            overcommit: OvercommitPolicy::Disallow,
            cache_policy: CachePolicy::Default,
            integrity: None,
        };
        let mut spec = BoundResourceSpec::new(
            unsafe { resource.view().raw_region() },
            MemoryDomain::StaticRegion,
            ResourceBackingKind::StaticRegion,
            ResourceAttrs::ALLOCATABLE | ResourceAttrs::STATIC_REGION | ResourceAttrs::DMA_VISIBLE,
            resource.geometry(),
            resource.layout(),
            contract,
            support,
            ResourceState::static_state(
                StateValue::Uniform(Protect::READ | Protect::WRITE),
                StateValue::Unknown,
                StateValue::Uniform(true),
            ),
        );
        spec.additional_hazards = ResourceHazardSet::EXTERNAL_MUTATION;

        let bound = BoundMemoryResource::new(spec).expect("bound resource");
        let info = bound.query(bound.range().base_addr()).expect("query");

        assert_eq!(bound.domain(), MemoryDomain::StaticRegion);
        assert_eq!(bound.backing_kind(), ResourceBackingKind::StaticRegion);
        assert!(bound.attrs().contains(ResourceAttrs::STATIC_REGION));
        assert!(bound.attrs().contains(ResourceAttrs::DMA_VISIBLE));
        assert!(bound.ops().contains(ResourceOpSet::QUERY));
        assert!(
            bound
                .hazards()
                .contains(ResourceHazardSet::EXTERNAL_MUTATION)
        );
        assert!(info.attrs.contains(RegionAttrs::DMA_VISIBLE));
        assert!(info.attrs.contains(RegionAttrs::STATIC_REGION));
        assert_eq!(info.protect, Protect::READ | Protect::WRITE);
    }

    #[test]
    fn bound_query_requires_precise_summary_state() {
        let request = ResourceRequest::anonymous_private(16 * 1024);
        let resource = VirtualMemoryResource::create(&request).expect("resource");
        let support = ResourceSupport {
            protect: Protect::READ | Protect::WRITE,
            ops: ResourceOpSet::QUERY,
            advice: MemAdviceCaps::empty(),
            residency: ResourceResidencySupport::empty(),
        };
        let contract = ResourceContract {
            allowed_protect: Protect::READ | Protect::WRITE,
            write_xor_execute: true,
            sharing: SharingPolicy::Private,
            overcommit: OvercommitPolicy::Disallow,
            cache_policy: CachePolicy::Default,
            integrity: None,
        };
        let spec = BoundResourceSpec::new(
            unsafe { resource.view().raw_region() },
            MemoryDomain::StaticRegion,
            ResourceBackingKind::StaticRegion,
            ResourceAttrs::ALLOCATABLE | ResourceAttrs::STATIC_REGION,
            resource.geometry(),
            resource.layout(),
            contract,
            support,
            ResourceState::static_state(
                StateValue::Unknown,
                StateValue::Unknown,
                StateValue::Uniform(true),
            ),
        );

        let err = BoundMemoryResource::new(spec).expect_err("imprecise query state should fail");
        assert_eq!(err.kind, ResourceErrorKind::InvalidRequest);
    }

    #[test]
    fn bound_spec_rejects_unsupported_live_ops() {
        let request = ResourceRequest::anonymous_private(16 * 1024);
        let resource = VirtualMemoryResource::create(&request).expect("resource");
        let support = ResourceSupport {
            protect: Protect::READ | Protect::WRITE,
            ops: ResourceOpSet::QUERY | ResourceOpSet::PROTECT,
            advice: MemAdviceCaps::empty(),
            residency: ResourceResidencySupport::empty(),
        };
        let contract = ResourceContract {
            allowed_protect: Protect::READ | Protect::WRITE,
            write_xor_execute: true,
            sharing: SharingPolicy::Private,
            overcommit: OvercommitPolicy::Disallow,
            cache_policy: CachePolicy::Default,
            integrity: None,
        };
        let spec = BoundResourceSpec::new(
            unsafe { resource.view().raw_region() },
            MemoryDomain::StaticRegion,
            ResourceBackingKind::StaticRegion,
            ResourceAttrs::ALLOCATABLE | ResourceAttrs::STATIC_REGION,
            resource.geometry(),
            resource.layout(),
            contract,
            support,
            ResourceState::static_state(
                StateValue::Uniform(Protect::READ | Protect::WRITE),
                StateValue::Unknown,
                StateValue::Uniform(true),
            ),
        );

        let err = BoundMemoryResource::new(spec).expect_err("unsupported ops should fail");
        assert_eq!(err.kind, ResourceErrorKind::InvalidRequest);
    }

    #[test]
    fn bound_spec_rejects_domain_backing_mismatch() {
        let request = ResourceRequest::anonymous_private(16 * 1024);
        let resource = VirtualMemoryResource::create(&request).expect("resource");
        let support = ResourceSupport {
            protect: Protect::READ | Protect::WRITE,
            ops: ResourceOpSet::QUERY,
            advice: MemAdviceCaps::empty(),
            residency: ResourceResidencySupport::empty(),
        };
        let contract = ResourceContract {
            allowed_protect: Protect::READ | Protect::WRITE,
            write_xor_execute: true,
            sharing: SharingPolicy::Private,
            overcommit: OvercommitPolicy::Disallow,
            cache_policy: CachePolicy::Default,
            integrity: None,
        };
        let spec = BoundResourceSpec::new(
            unsafe { resource.view().raw_region() },
            MemoryDomain::StaticRegion,
            ResourceBackingKind::AnonymousPrivate,
            ResourceAttrs::ALLOCATABLE | ResourceAttrs::STATIC_REGION,
            resource.geometry(),
            resource.layout(),
            contract,
            support,
            ResourceState::static_state(
                StateValue::Uniform(Protect::READ | Protect::WRITE),
                StateValue::Unknown,
                StateValue::Uniform(true),
            ),
        );

        let err =
            BoundMemoryResource::new(spec).expect_err("mismatched domain/backing should fail");
        assert_eq!(err.kind, ResourceErrorKind::InvalidRequest);
    }

    #[test]
    fn initial_locked_residency_tracks_verified_lock_state() {
        let page = system_mem().page_info().base_page.get();
        let mut request = ResourceRequest::anonymous_private(page);
        request.initial.residency = InitialResidency::Locked;

        let resource = VirtualMemoryResource::create(&request).expect("resource");

        assert_eq!(resource.state().locked, StateValue::Uniform(true));
    }

    #[test]
    fn state_tracks_runtime_protect_and_lock_transitions() {
        let page = system_mem().page_info().base_page.get();
        let request = ResourceRequest::anonymous_private(page * 2);
        let resource = VirtualMemoryResource::create(&request).expect("resource");

        unsafe { resource.protect(ResourceRange::new(0, page), Protect::READ) }.expect("protect");
        assert_eq!(resource.state().current_protect, StateValue::Asymmetric);

        unsafe { resource.lock(ResourceRange::new(0, page)) }.expect("lock");
        assert_eq!(resource.state().locked, StateValue::Asymmetric);

        unsafe { resource.unlock(ResourceRange::new(0, page)) }.expect("unlock");
        assert_eq!(resource.state().locked, StateValue::Asymmetric);
    }

    #[test]
    fn full_range_mutation_preserves_uniform_state() {
        let request = ResourceRequest::anonymous_private(16 * 1024);
        let resource = VirtualMemoryResource::create(&request).expect("resource");
        let whole = ResourceRange::whole(resource.len());

        unsafe { resource.protect(whole, Protect::READ) }.expect("protect");
        assert_eq!(
            resource.state().current_protect,
            StateValue::Uniform(Protect::READ)
        );

        unsafe { resource.lock(whole) }.expect("lock");
        assert_eq!(resource.state().locked, StateValue::Uniform(true));

        unsafe { resource.unlock(whole) }.expect("unlock");
        assert_eq!(resource.state().locked, StateValue::Uniform(false));
    }

    #[test]
    fn discard_is_legal_when_reported() {
        let request = ResourceRequest::anonymous_private(16 * 1024);
        let resource = VirtualMemoryResource::create(&request).expect("resource");
        let page = resource.page_info().base_page.get();

        unsafe { resource.discard(ResourceRange::new(0, page)) }.expect("discard");
    }

    #[test]
    fn subregion_respects_bounds() {
        let page = system_mem().page_info().base_page.get();
        let request = ResourceRequest::anonymous_private(page * 4);
        let resource = VirtualMemoryResource::create(&request).expect("resource");
        let page = resource.page_info().base_page.get();
        let region = resource
            .subview(ResourceRange::new(page, page * 2))
            .expect("subview");

        assert_eq!(region.len(), page * 2);
        assert!(
            resource
                .subview(ResourceRange::new(resource.len(), page))
                .is_err()
        );
    }

    #[test]
    fn contract_blocks_executable_upgrade() {
        let request = ResourceRequest::anonymous_private(16 * 1024);
        let resource = VirtualMemoryResource::create(&request).expect("resource");
        let page = resource.page_info().base_page.get();

        let err =
            unsafe { resource.protect(ResourceRange::new(0, page), Protect::READ | Protect::EXEC) }
                .expect_err("exec upgrade should fail");

        assert_eq!(err.kind, ResourceErrorKind::ContractViolation);
    }

    #[test]
    fn contract_blocks_write_execute_mapping() {
        let mut request = ResourceRequest::anonymous_private(16 * 1024);
        request.contract.allowed_protect = Protect::READ | Protect::WRITE | Protect::EXEC;
        let resource = VirtualMemoryResource::create(&request).expect("resource");
        let page = resource.page_info().base_page.get();

        let err = unsafe {
            resource.protect(
                ResourceRange::new(0, page),
                Protect::READ | Protect::WRITE | Protect::EXEC,
            )
        }
        .expect_err("w+x should fail");

        assert_eq!(err.kind, ResourceErrorKind::ContractViolation);
    }

    #[test]
    fn huge_page_advice_is_supported_when_reported() {
        let request = ResourceRequest::anonymous_private(16 * 1024);
        let resource = VirtualMemoryResource::create(&request).expect("resource");
        let page = resource.page_info().base_page.get();
        if resource.support().advice.contains(MemAdviceCaps::HUGE_PAGE) {
            unsafe { resource.advise(ResourceRange::new(0, page), Advise::HugePage) }
                .expect("huge page advice should succeed");
        } else {
            let err = unsafe { resource.advise(ResourceRange::new(0, page), Advise::HugePage) }
                .expect_err("huge page advice should be unsupported");
            assert_eq!(err.kind, ResourceErrorKind::UnsupportedOperation);
        }
    }

    #[test]
    fn required_node_and_region_id_translate_when_supported() {
        assert_eq!(
            required_placement_to_mem(
                Some(RequiredPlacement::RequiredNode(7)),
                4096,
                MemPlacementCaps::REQUIRED_NODE,
            )
            .expect("required node"),
            Some(Placement::RequiredNode(7))
        );
        assert_eq!(
            required_placement_to_mem(
                Some(RequiredPlacement::RegionId(42)),
                4096,
                MemPlacementCaps::REGION_ID,
            )
            .expect("region id"),
            Some(Placement::RegionId(42))
        );
    }

    #[test]
    fn live_resource_types_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}

        assert_send_sync::<VirtualMemoryResource>();
        assert_send_sync::<AddressReservation>();
        assert_send_sync::<BoundMemoryResource>();
    }

    #[test]
    fn normalize_len_rejects_non_power_of_two_granule() {
        let error = normalize_len(4096, 24).expect_err("non-power-of-two granule should reject");
        assert_eq!(error.kind, ResourceErrorKind::InvalidRequest);
    }
}
