use ::core::ptr::NonNull;

use fusion_pal::sys::mem::{
    Advise, Backing, CachePolicy, MapFlags, MapRequest, MemAdviceCaps, MemAdvise, MemBackingCaps,
    MemBase, MemCaps, MemCommit, MemLock, MemMap, MemPlacementCaps, MemProtect, MemQuery,
    MemSupport, PageInfo, Placement, Protect, Region, RegionAttrs, RegionInfo, system_mem,
};

#[path = "attrs.rs"]
mod attrs;
#[path = "bound.rs"]
mod bound;
#[path = "core.rs"]
mod core;
#[path = "domain.rs"]
mod domain;
#[path = "error.rs"]
mod error;
#[path = "geometry.rs"]
mod geometry;
#[path = "ops.rs"]
mod ops;
#[path = "range.rs"]
mod range;
#[path = "request.rs"]
mod request;
#[path = "reservation.rs"]
mod reservation;
#[path = "resolved.rs"]
mod resolved;
#[path = "state.rs"]
mod state;
#[path = "support.rs"]
mod support;

pub use attrs::ResourceAttrs;
pub use bound::{BoundMemoryResource, BoundResourceSpec};
pub use domain::{MemoryDomain, MemoryDomainSet, ResourceBackingKind};
pub use error::{ResourceError, ResourceErrorKind};
pub use geometry::MemoryGeometry;
pub use ops::{ResourceHazardSet, ResourceOpSet, ResourcePreferenceSet};
pub use range::ResourceRange;
pub use request::{
    InitialResidency, InitialResourceState, IntegrityConstraints, OvercommitPolicy,
    PlacementPreference, RequiredPlacement, ResourceBackingRequest, ResourceContract,
    ResourceRequest, SharingPolicy,
};
pub use reservation::{
    AddressReservation, MaterializedReservation, ReservationHazardSet, ReservationOpSet,
    ReservationRequest, ReservationSupport, ResolvedAddressReservation,
};
pub use resolved::{ResolvedResource, ResourceInfo};
pub use state::{ResourceState, ResourceStateProvenance, StateValue};
pub use support::{
    ResourceAcquireSupport, ResourceFeatureSupport, ResourceResidencySupport, ResourceSupport,
};

use self::core::ResourceCore;

pub trait MemoryResource {
    fn info(&self) -> &ResourceInfo;

    fn state(&self) -> ResourceState;

    fn range(&self) -> Region {
        self.info().range
    }

    fn backing_kind(&self) -> ResourceBackingKind {
        self.info().backing
    }

    fn domain(&self) -> MemoryDomain {
        self.info().domain
    }

    fn attrs(&self) -> ResourceAttrs {
        self.info().attrs
    }

    fn geometry(&self) -> MemoryGeometry {
        self.info().geometry
    }

    fn contract(&self) -> ResourceContract {
        self.info().contract
    }

    fn support(&self) -> ResourceSupport {
        self.info().support
    }

    fn ops(&self) -> ResourceOpSet {
        self.info().ops()
    }

    fn hazards(&self) -> ResourceHazardSet {
        self.info().hazards
    }

    fn contains(&self, ptr: *const u8) -> bool {
        self.range().contains(ptr as usize)
    }

    fn subrange(&self, range: ResourceRange) -> Result<Region, ResourceError> {
        if range.len == 0 {
            return Err(ResourceError::invalid_range());
        }

        self.range()
            .subrange(range.offset, range.len)
            .map_err(|_| ResourceError::invalid_range())
    }
}

pub trait QueryableResource: MemoryResource {
    fn query(&self, addr: NonNull<u8>) -> Result<RegionInfo, ResourceError>;
}

pub trait ProtectableResource: MemoryResource {
    /// # Safety
    /// Caller must ensure the target range is not actively referenced in ways that would
    /// violate the requested protection change.
    unsafe fn protect(&self, range: ResourceRange, protect: Protect) -> Result<(), ResourceError>;
}

pub trait AdvisableResource: MemoryResource {
    /// # Safety
    /// Caller must ensure the target range is valid for the requested advisory update.
    unsafe fn advise(&self, range: ResourceRange, advice: Advise) -> Result<(), ResourceError>;
}

pub trait DiscardableResource: MemoryResource {
    /// # Safety
    /// Caller must ensure discarding the target range is valid for the resource's backing
    /// and does not violate higher-level lifetime assumptions.
    unsafe fn discard(&self, range: ResourceRange) -> Result<(), ResourceError>;
}

pub trait FlushableResource: MemoryResource {
    /// # Safety
    /// Caller must ensure flushing the target range is meaningful for the resource's
    /// backing and ordering model.
    unsafe fn flush(&self, range: ResourceRange) -> Result<(), ResourceError>;
}

pub trait CommitControlledResource: MemoryResource {
    /// # Safety
    /// Caller must ensure committing the target range is valid for this resource's backing
    /// and contract.
    unsafe fn commit(&self, range: ResourceRange, protect: Protect) -> Result<(), ResourceError>;

    /// # Safety
    /// Caller must ensure decommitting the target range does not invalidate live references.
    unsafe fn decommit(&self, range: ResourceRange) -> Result<(), ResourceError>;
}

pub trait LockableResource: MemoryResource {
    /// # Safety
    /// Caller must ensure locking the target range is legal in the current execution context.
    unsafe fn lock(&self, range: ResourceRange) -> Result<(), ResourceError>;

    /// # Safety
    /// Caller must ensure the target range was previously locked in a compatible way.
    unsafe fn unlock(&self, range: ResourceRange) -> Result<(), ResourceError>;
}

#[derive(Debug)]
pub struct VirtualMemoryResource {
    provider: fusion_pal::sys::mem::PlatformMem,
    region: Option<Region>,
    page_info: PageInfo,
    core: ResourceCore,
}

impl VirtualMemoryResource {
    #[must_use]
    pub fn system_acquire_support() -> ResourceAcquireSupport {
        let provider = system_mem();
        resource_acquire_support_from_mem_support(provider.support())
    }

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
            request.contract.required_placement,
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

        if request.preferences.contains(ResourcePreferenceSet::LOCK)
            && !preferred_flags.contains(MapFlags::LOCKED)
        {
            if resource_support
                .residency
                .contains(ResourceResidencySupport::LOCKED)
            {
                preferred_flags |= MapFlags::LOCKED;
            } else {
                unmet |= ResourcePreferenceSet::LOCK;
            }
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
            match unsafe { provider.map(&preferred_request) } {
                Ok(region) => {
                    if !placement_preference_honored(request.initial.placement, region) {
                        unmet |= ResourcePreferenceSet::PLACEMENT;
                    }
                    (region, preferred_flags)
                }
                Err(_) => {
                    if preferred_flags.contains(MapFlags::LOCKED)
                        && !base_flags.contains(MapFlags::LOCKED)
                    {
                        unmet |= ResourcePreferenceSet::LOCK;
                    }
                    if preferred_flags.contains(MapFlags::POPULATE)
                        && !base_flags.contains(MapFlags::POPULATE)
                    {
                        unmet |= ResourcePreferenceSet::PREFAULT;
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
            }
        } else {
            (
                unsafe { provider.map(&required_request) }
                    .map_err(ResourceError::from_request_error)?,
                base_flags,
            )
        };

        verify_required_placement(region, request.contract.required_placement)?;
        apply_resource_preferences_after_map(
            &provider,
            region,
            request,
            acquire_support,
            &mut unmet,
        );
        let initial_state = ResourceState::tracked(
            request.initial.protect,
            actual_flags.contains(MapFlags::LOCKED),
            true,
        );

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
                unmet,
                initial_state,
            ),
            initial_state,
        ))
    }

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

    #[must_use]
    pub const fn page_info(&self) -> PageInfo {
        self.page_info
    }

    #[must_use]
    pub const fn resolved(&self) -> ResolvedResource {
        self.core.resolved()
    }

    #[must_use]
    pub fn region(&self) -> Region {
        self.range()
    }

    pub fn subregion(&self, range: ResourceRange) -> Result<Region, ResourceError> {
        self.subrange(range)
    }

    fn operation_region(&self, range: ResourceRange) -> Result<Region, ResourceError> {
        if range.len == 0 {
            return Err(ResourceError::invalid_range());
        }

        let page = self.page_info.base_page.get();
        if range.offset % page != 0 || range.len % page != 0 {
            return Err(ResourceError::invalid_range());
        }

        self.subrange(range)
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

    fn set_current_protect(&self, protect: Protect) {
        self.core.set_current_protect(protect);
    }

    fn mark_protect_asymmetric(&self) {
        self.core.mark_protect_asymmetric();
    }

    fn set_locked_state(&self, locked: bool) {
        self.core.set_locked_state(locked);
    }

    fn mark_locked_asymmetric(&self) {
        self.core.mark_locked_asymmetric();
    }

    fn set_committed_state(&self, committed: bool) {
        self.core.set_committed_state(committed);
    }

    fn mark_committed_asymmetric(&self) {
        self.core.mark_committed_asymmetric();
    }

    fn is_full_resource_range(&self, range: ResourceRange) -> bool {
        range.offset == 0 && range.len == self.range().len
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
        unsafe { self.provider.protect(region, protect) }
            .map_err(ResourceError::from_operation_error)?;
        if self.is_full_resource_range(range) {
            self.set_current_protect(protect);
        } else {
            self.mark_protect_asymmetric();
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
        unsafe { self.provider.commit(region, protect) }
            .map_err(ResourceError::from_operation_error)?;
        if self.is_full_resource_range(range) {
            self.set_current_protect(protect);
            self.set_committed_state(true);
        } else {
            self.mark_protect_asymmetric();
            self.mark_committed_asymmetric();
        }
        Ok(())
    }

    unsafe fn decommit(&self, range: ResourceRange) -> Result<(), ResourceError> {
        if !self.ops().contains(ResourceOpSet::DECOMMIT) {
            return Err(ResourceError::unsupported_operation());
        }

        let region = self.operation_region(range)?;
        unsafe { self.provider.decommit(region) }.map_err(ResourceError::from_operation_error)?;
        if self.is_full_resource_range(range) {
            self.set_committed_state(false);
        } else {
            self.mark_committed_asymmetric();
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
        unsafe { self.provider.lock(region) }.map_err(ResourceError::from_operation_error)?;
        if self.is_full_resource_range(range) {
            self.set_locked_state(true);
        } else {
            self.mark_locked_asymmetric();
        }
        Ok(())
    }

    unsafe fn unlock(&self, range: ResourceRange) -> Result<(), ResourceError> {
        if !self.ops().contains(ResourceOpSet::LOCK) {
            return Err(ResourceError::unsupported_operation());
        }

        let region = self.operation_region(range)?;
        unsafe { self.provider.unlock(region) }.map_err(ResourceError::from_operation_error)?;
        if self.is_full_resource_range(range) {
            self.set_locked_state(false);
        } else {
            self.mark_locked_asymmetric();
        }
        Ok(())
    }
}

impl QueryableResource for VirtualMemoryResource {
    fn query(&self, addr: NonNull<u8>) -> Result<RegionInfo, ResourceError> {
        if !self.ops().contains(ResourceOpSet::QUERY) {
            return Err(ResourceError::unsupported_operation());
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

fn supports_backing(
    backing: ResourceBackingRequest,
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

fn supports_advice(supported: MemAdviceCaps, advice: Advise) -> bool {
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

fn preferred_discard_advice(supported: MemAdviceCaps) -> Option<Advise> {
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
            flags |= MapFlags::LOCKED;
        }
    }

    Ok(flags)
}

pub(super) fn request_backing_to_mem(backing: ResourceBackingRequest) -> Backing<'static> {
    match backing {
        ResourceBackingRequest::Anonymous => Backing::Anonymous,
        ResourceBackingRequest::File { fd, offset } => Backing::File { fd, offset },
    }
}

pub(super) fn backing_kind_from_request(
    backing: ResourceBackingRequest,
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

pub(super) const fn resource_attrs_for_request(_backing: ResourceBackingRequest) -> RegionAttrs {
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

pub(super) fn required_placement_to_mem(
    placement: Option<RequiredPlacement>,
    granule: usize,
    supported: MemPlacementCaps,
) -> Result<Option<Placement>, ResourceError> {
    match placement {
        None => Ok(None),
        Some(RequiredPlacement::FixedNoReplace(addr)) => {
            if addr % granule != 0 {
                return Err(ResourceError::invalid_request());
            }
            if !supported.contains(MemPlacementCaps::FIXED_NOREPLACE) {
                return Err(ResourceError::unsupported_request());
            }
            Ok(Some(Placement::FixedNoReplace(addr)))
        }
        Some(RequiredPlacement::RequiredNode(_)) | Some(RequiredPlacement::RegionId(_)) => {
            Err(ResourceError::unsupported_request())
        }
    }
}

pub(super) fn preferred_placement_to_mem(
    placement: PlacementPreference,
    granule: usize,
    supported: MemPlacementCaps,
) -> Result<(Option<Placement>, bool), ResourceError> {
    match placement {
        PlacementPreference::Anywhere => Ok((None, false)),
        PlacementPreference::Hint(addr) => {
            if addr % granule != 0 {
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

fn base_map_request(
    len: usize,
    protect: Protect,
    flags: MapFlags,
    placement: Placement,
    attrs: RegionAttrs,
    cache: CachePolicy,
    backing: Backing<'static>,
) -> MapRequest<'static> {
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
    preference: PlacementPreference,
    region: Region,
) -> bool {
    match preference {
        PlacementPreference::Anywhere | PlacementPreference::PreferredNode(_) => true,
        PlacementPreference::Hint(addr) => region.base.as_ptr() as usize == addr,
    }
}

pub(super) fn verify_required_placement(
    region: Region,
    placement: Option<RequiredPlacement>,
) -> Result<(), ResourceError> {
    match placement {
        None => Ok(()),
        Some(RequiredPlacement::FixedNoReplace(addr)) if region.base.as_ptr() as usize == addr => {
            Ok(())
        }
        Some(RequiredPlacement::FixedNoReplace(_))
        | Some(RequiredPlacement::RequiredNode(_))
        | Some(RequiredPlacement::RegionId(_)) => Err(ResourceError::unsupported_request()),
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
    if mem_support.map_flags.contains(MapFlags::LOCKED) && mem_support.caps.contains(MemCaps::LOCK)
    {
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
    if mem_support.map_flags.contains(MapFlags::LOCKED) && mem_support.caps.contains(MemCaps::LOCK)
    {
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

pub(super) fn apply_resource_preferences_after_map(
    provider: &fusion_pal::sys::mem::PlatformMem,
    region: Region,
    request: &ResourceRequest<'_>,
    support: ResourceAcquireSupport,
    unmet: &mut ResourcePreferenceSet,
) {
    if !request
        .preferences
        .contains(ResourcePreferenceSet::HUGE_PAGES)
    {
        return;
    }

    if !support
        .preferences
        .contains(ResourcePreferenceSet::HUGE_PAGES)
        || !support.instance.ops.contains(ResourceOpSet::ADVISE)
    {
        *unmet |= ResourcePreferenceSet::HUGE_PAGES;
        return;
    }

    if unsafe { provider.advise(region, Advise::HugePage) }.is_err() {
        *unmet |= ResourcePreferenceSet::HUGE_PAGES;
    }
}

pub(super) fn build_resolved_resource(
    range: Region,
    request: &ResourceRequest<'_>,
    support: ResourceSupport,
    backing: ResourceBackingKind,
    attrs: ResourceAttrs,
    geometry: MemoryGeometry,
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
            contract: request.contract,
            support,
            hazards: infer_resource_hazards(request.contract, attrs),
        },
        initial_state,
        unmet_preferences,
    }
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

    let mask = align.checked_sub(1)?;
    value.checked_add(mask).map(|rounded| rounded & !mask)
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
        assert!(
            support
                .instance
                .residency
                .contains(ResourceResidencySupport::PREFAULT)
        );
        assert!(
            support
                .instance
                .residency
                .contains(ResourceResidencySupport::LOCKED)
        );
        assert!(support.instance.ops.contains(ResourceOpSet::ADVISE));
        assert!(support.instance.ops.contains(ResourceOpSet::QUERY));
        assert!(support.instance.advice.contains(MemAdviceCaps::FREE));
        assert!(support.instance.advice.contains(MemAdviceCaps::HUGE_PAGE));
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
        assert_eq!(resource.region().len, 16 * 1024);
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
        let info = resource.query(resource.region().base).expect("query");

        assert!(
            info.region
                .contains(resource.region().base.as_ptr() as usize)
        );
        assert!(info.region.len >= resource.region().len);
        assert!(info.protect.contains(Protect::READ));
        assert!(info.protect.contains(Protect::WRITE));
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
        assert_eq!(resource.region().len, 16 * 1024);
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

        assert_eq!(leading.region().len, page);
        assert_eq!(resource.region().len, page);
        assert_eq!(trailing.region().len, page);
        assert_eq!(
            leading.region().end_addr(),
            resource.region().base.as_ptr() as usize
        );
        assert_eq!(
            resource.region().end_addr(),
            trailing.region().base.as_ptr() as usize
        );
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
            required_placement: None,
        };
        let mut spec = BoundResourceSpec::new(
            resource.region(),
            MemoryDomain::StaticRegion,
            ResourceBackingKind::StaticRegion,
            ResourceAttrs::ALLOCATABLE | ResourceAttrs::STATIC_REGION | ResourceAttrs::DMA_VISIBLE,
            resource.geometry(),
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
        let info = bound.query(bound.range().base).expect("query");

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
    fn state_tracks_runtime_protect_and_lock_transitions() {
        let request = ResourceRequest::anonymous_private(16 * 1024);
        let resource = VirtualMemoryResource::create(&request).expect("resource");
        let page = resource.page_info().base_page.get();

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
        let whole = ResourceRange::whole(resource.region().len);

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
        let request = ResourceRequest::anonymous_private(16 * 1024);
        let resource = VirtualMemoryResource::create(&request).expect("resource");
        let page = resource.page_info().base_page.get();
        let region = resource
            .subregion(ResourceRange::new(page, page * 2))
            .expect("subregion");

        assert_eq!(region.len, page * 2);
        assert!(
            resource
                .subregion(ResourceRange::new(resource.region().len, page))
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

        unsafe { resource.advise(ResourceRange::new(0, page), Advise::HugePage) }
            .expect("huge page advice should succeed");
    }
}
