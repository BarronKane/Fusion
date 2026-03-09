use core::ptr::NonNull;

use fusion_pal::sys::mem::{
    Backing, CachePolicy, MapFlags, MapReplaceRequest, MapRequest, MemBackingCaps, MemBase,
    MemCaps, MemLock, MemMap, MemMapReplace, MemPlacementCaps, MemProtect, MemQuery, PageInfo,
    Placement, Protect, Region, RegionAttrs, RegionInfo, ReplacePlacement, system_mem,
};

use super::{
    InitialResidency, MemoryGeometry, PlacementPreference, RequiredPlacement, ResourceError,
    ResourceOpSet, ResourcePreferenceSet, ResourceRequest, ResourceState, VirtualMemoryResource,
    apply_resource_preferences_after_map, backing_kind_from_request, build_resolved_resource,
    geometry_from_page_info, initial_map_flags, normalize_len, preferred_placement_to_mem,
    request_backing_to_mem, required_placement_to_mem, resource_acquire_support_from_mem_support,
    resource_attrs_for_request, resource_attrs_from_request, validate_request,
    verify_required_placement,
};

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ReservationOpSet: u32 {
        const QUERY                 = 1 << 0;
        const MATERIALIZE_IN_PLACE  = 1 << 1;
        const MATERIALIZE_REPLACE   = 1 << 2;
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ReservationHazardSet: u32 {
        const REPLACE_MATERIALIZATION = 1 << 0;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReservationSupport {
    pub backings: MemBackingCaps,
    pub placements: MemPlacementCaps,
    pub in_place_backings: MemBackingCaps,
    pub replace_backings: MemBackingCaps,
    pub ops: ReservationOpSet,
    pub hazards: ReservationHazardSet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolvedAddressReservation {
    pub range: Region,
    pub geometry: MemoryGeometry,
    pub support: ReservationSupport,
    pub unmet_preferences: ResourcePreferenceSet,
}

#[derive(Debug)]
pub struct MaterializedReservation {
    pub leading: Option<AddressReservation>,
    pub resource: VirtualMemoryResource,
    pub trailing: Option<AddressReservation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReservationRequest<'a> {
    pub name: Option<&'a str>,
    pub len: usize,
    pub placement: PlacementPreference,
    pub required_placement: Option<RequiredPlacement>,
}

impl<'a> ReservationRequest<'a> {
    #[must_use]
    pub const fn new(len: usize) -> Self {
        Self {
            name: None,
            len,
            placement: PlacementPreference::Anywhere,
            required_placement: None,
        }
    }
}

#[derive(Debug)]
pub struct AddressReservation {
    provider: fusion_pal::sys::mem::PlatformMem,
    region: Option<Region>,
    page_info: PageInfo,
    resolved: ResolvedAddressReservation,
}

impl AddressReservation {
    fn from_parts(
        provider: fusion_pal::sys::mem::PlatformMem,
        region: Region,
        page_info: PageInfo,
        resolved: ResolvedAddressReservation,
    ) -> Self {
        Self {
            provider,
            region: Some(region),
            page_info,
            resolved,
        }
    }

    #[must_use]
    pub fn system_support() -> ReservationSupport {
        reservation_support_from_mem_support(system_mem().support())
    }

    pub fn create(request: &ReservationRequest<'_>) -> Result<Self, ResourceError> {
        let provider = system_mem();
        let page_info = provider.page_info();
        let support = reservation_support_from_mem_support(provider.support());
        let len = normalize_len(request.len, page_info.alloc_granule.get())?;

        validate_reservation_request(request, support, page_info.alloc_granule.get())?;

        let required_placement = required_placement_to_mem(
            request.required_placement,
            page_info.alloc_granule.get(),
            support.placements,
        )?;
        let (preferred_placement, preferred_unmet) = preferred_placement_to_mem(
            request.placement,
            page_info.alloc_granule.get(),
            support.placements,
        )?;

        if required_placement.is_some()
            && !matches!(request.placement, PlacementPreference::Anywhere)
        {
            return Err(ResourceError::invalid_request());
        }

        let required_request = MapRequest {
            len,
            align: 0,
            protect: Protect::NONE,
            flags: MapFlags::PRIVATE,
            attrs: RegionAttrs::VIRTUAL_ONLY,
            cache: CachePolicy::Default,
            placement: required_placement.unwrap_or(Placement::Anywhere),
            backing: Backing::Anonymous,
        };
        let preferred_request = MapRequest {
            placement: preferred_placement.unwrap_or(required_request.placement),
            ..required_request
        };

        let mut unmet = ResourcePreferenceSet::empty();
        if preferred_unmet {
            unmet |= ResourcePreferenceSet::PLACEMENT;
        }

        let has_preferred_attempt = preferred_request.placement != required_request.placement;
        let region = if has_preferred_attempt {
            match unsafe { provider.map(&preferred_request) } {
                Ok(region) => {
                    if !super::placement_preference_honored(request.placement, region) {
                        unmet |= ResourcePreferenceSet::PLACEMENT;
                    }
                    region
                }
                Err(_) => {
                    unmet |= ResourcePreferenceSet::PLACEMENT;
                    unsafe { provider.map(&required_request) }
                        .map_err(ResourceError::from_request_error)?
                }
            }
        } else {
            unsafe { provider.map(&required_request) }.map_err(ResourceError::from_request_error)?
        };

        verify_required_placement(region, request.required_placement)?;

        Ok(Self::from_parts(
            provider,
            region,
            page_info,
            ResolvedAddressReservation {
                range: region,
                geometry: geometry_from_page_info(page_info, ResourceOpSet::empty()),
                support,
                unmet_preferences: unmet,
            },
        ))
    }

    #[must_use]
    pub fn page_info(&self) -> PageInfo {
        self.page_info
    }

    #[must_use]
    pub fn support(&self) -> ReservationSupport {
        self.resolved.support
    }

    #[must_use]
    pub fn resolved(&self) -> ResolvedAddressReservation {
        self.resolved
    }

    #[must_use]
    pub fn region(&self) -> Region {
        self.region
            .expect("reservation region missing during active use")
    }

    #[must_use]
    pub fn contains(&self, ptr: *const u8) -> bool {
        self.region().contains(ptr as usize)
    }

    pub fn subregion(&self, range: super::ResourceRange) -> Result<Region, ResourceError> {
        if range.len == 0 {
            return Err(ResourceError::invalid_range());
        }

        self.region()
            .subrange(range.offset, range.len)
            .map_err(|_| ResourceError::invalid_range())
    }

    pub fn query(&self, addr: NonNull<u8>) -> Result<RegionInfo, ResourceError> {
        if !self.support().ops.contains(ReservationOpSet::QUERY) {
            return Err(ResourceError::unsupported_operation());
        }

        self.provider
            .query(addr)
            .map_err(ResourceError::from_operation_error)
    }

    pub fn into_resource(
        self,
        request: &ResourceRequest<'_>,
    ) -> Result<VirtualMemoryResource, ResourceError> {
        let region = self.region();
        let materialized =
            self.materialize_range(super::ResourceRange::new(0, region.len), request)?;

        debug_assert!(materialized.leading.is_none());
        debug_assert!(materialized.trailing.is_none());

        Ok(materialized.resource)
    }

    pub fn materialize_range(
        mut self,
        range: super::ResourceRange,
        request: &ResourceRequest<'_>,
    ) -> Result<MaterializedReservation, ResourceError> {
        let acquire_support = resource_acquire_support_from_mem_support(self.provider.support());
        let resource_support = acquire_support.instance;
        let reserved = self.region();
        let region = materialization_region(reserved, range, self.page_info.alloc_granule.get())?;
        validate_request(request, acquire_support)?;
        validate_materialization_request(request, region, self.page_info.alloc_granule.get())?;

        let use_replace = materialization_requires_replace(request);
        let backing_kind = backing_kind_from_request(request.backing, request.contract.sharing);
        let mut unmet = ResourcePreferenceSet::empty();
        let mut actual_locked = false;

        let region = if use_replace {
            if !self
                .support()
                .ops
                .contains(ReservationOpSet::MATERIALIZE_REPLACE)
            {
                return Err(ResourceError::unsupported_request());
            }

            let flags = initial_map_flags(request, resource_support)?;
            let replace_request = MapReplaceRequest {
                len: region.len,
                align: 0,
                protect: request.initial.protect,
                flags,
                attrs: resource_attrs_for_request(request.backing),
                cache: request.contract.cache_policy,
                placement: ReplacePlacement::FixedReplace(region.base.as_ptr() as usize),
                backing: request_backing_to_mem(request.backing),
            };
            actual_locked = flags.contains(MapFlags::LOCKED);

            unsafe { self.provider.map_replace(&replace_request) }
                .map_err(ResourceError::from_request_error)?
        } else {
            if !self
                .support()
                .ops
                .contains(ReservationOpSet::MATERIALIZE_IN_PLACE)
            {
                return Err(ResourceError::unsupported_request());
            }

            if request.initial.protect != Protect::NONE {
                unsafe { self.provider.protect(region, request.initial.protect) }
                    .map_err(ResourceError::from_operation_error)?;
            }

            if matches!(request.initial.residency, InitialResidency::Locked) {
                unsafe { self.provider.lock(region) }
                    .map_err(ResourceError::from_operation_error)?;
                actual_locked = true;
            }

            if request.preferences.contains(ResourcePreferenceSet::LOCK)
                && !matches!(request.initial.residency, InitialResidency::Locked)
            {
                match unsafe { self.provider.lock(region) } {
                    Ok(()) => {
                        actual_locked = true;
                    }
                    Err(_) => unmet |= ResourcePreferenceSet::LOCK,
                }
            }

            if request
                .preferences
                .contains(ResourcePreferenceSet::PREFAULT)
            {
                unmet |= ResourcePreferenceSet::PREFAULT;
            }

            region
        };

        apply_resource_preferences_after_map(
            &self.provider,
            region,
            request,
            acquire_support,
            &mut unmet,
        );

        let leading = remainder_reservation(
            self.provider,
            self.page_info,
            self.resolved,
            reserved,
            0,
            range.offset,
        )?;
        let trailing_offset = range
            .offset
            .checked_add(range.len)
            .ok_or_else(ResourceError::invalid_range)?;
        let trailing = remainder_reservation(
            self.provider,
            self.page_info,
            self.resolved,
            reserved,
            trailing_offset,
            reserved
                .len
                .checked_sub(trailing_offset)
                .ok_or_else(ResourceError::invalid_range)?,
        )?;

        self.region = None;

        Ok(MaterializedReservation {
            leading,
            resource: VirtualMemoryResource::from_parts(
                self.provider,
                region,
                self.page_info,
                build_resolved_resource(
                    region,
                    request,
                    resource_support,
                    backing_kind,
                    resource_attrs_from_request(request),
                    geometry_from_page_info(self.page_info, resource_support.ops),
                    unmet,
                    ResourceState::tracked(request.initial.protect, actual_locked, true),
                ),
                ResourceState::tracked(request.initial.protect, actual_locked, true),
            ),
            trailing,
        })
    }
}

impl Drop for AddressReservation {
    fn drop(&mut self) {
        if let Some(region) = self.region.take() {
            let _ = unsafe { self.provider.unmap(region) };
        }
    }
}

pub(super) fn reservation_support_from_mem_support(
    mem_support: fusion_pal::sys::mem::MemSupport,
) -> ReservationSupport {
    let in_place_backings = if mem_support.caps.contains(MemCaps::PROTECT)
        && mem_support.backings.contains(MemBackingCaps::ANON_PRIVATE)
    {
        MemBackingCaps::ANON_PRIVATE
    } else {
        MemBackingCaps::empty()
    };
    let replace_backings = if mem_support.caps.contains(MemCaps::MAP_FIXED_REPLACE) {
        mem_support.backings
            & (MemBackingCaps::ANON_PRIVATE
                | MemBackingCaps::ANON_SHARED
                | MemBackingCaps::FILE_PRIVATE
                | MemBackingCaps::FILE_SHARED)
    } else {
        MemBackingCaps::empty()
    };

    let mut ops = ReservationOpSet::empty();
    if mem_support.caps.contains(MemCaps::QUERY) {
        ops |= ReservationOpSet::QUERY;
    }
    if !in_place_backings.is_empty() {
        ops |= ReservationOpSet::MATERIALIZE_IN_PLACE;
    }
    if !replace_backings.is_empty() {
        ops |= ReservationOpSet::MATERIALIZE_REPLACE;
    }

    let mut hazards = ReservationHazardSet::empty();
    if !replace_backings.is_empty() {
        hazards |= ReservationHazardSet::REPLACE_MATERIALIZATION;
    }

    ReservationSupport {
        backings: mem_support.backings & MemBackingCaps::ANON_PRIVATE,
        placements: mem_support.placements,
        in_place_backings,
        replace_backings,
        ops,
        hazards,
    }
}

fn validate_reservation_request(
    request: &ReservationRequest<'_>,
    support: ReservationSupport,
    granule: usize,
) -> Result<(), ResourceError> {
    if request.len == 0 {
        return Err(ResourceError::invalid_request());
    }

    if !support.backings.contains(MemBackingCaps::ANON_PRIVATE) {
        return Err(ResourceError::unsupported_request());
    }

    required_placement_to_mem(request.required_placement, granule, support.placements)?;
    preferred_placement_to_mem(request.placement, granule, support.placements)?;

    Ok(())
}

fn validate_materialization_request(
    request: &ResourceRequest<'_>,
    region: Region,
    granule: usize,
) -> Result<(), ResourceError> {
    let base = region.base.as_ptr() as usize;
    let normalized_len = normalize_len(request.len, granule)?;

    if normalized_len != region.len {
        return Err(ResourceError::invalid_request());
    }

    match request.contract.required_placement {
        None => {}
        Some(RequiredPlacement::FixedNoReplace(addr)) if addr == base => {}
        Some(RequiredPlacement::FixedNoReplace(_)) => return Err(ResourceError::invalid_request()),
        Some(RequiredPlacement::RequiredNode(_)) | Some(RequiredPlacement::RegionId(_)) => {
            return Err(ResourceError::unsupported_request());
        }
    }

    match request.initial.placement {
        PlacementPreference::Anywhere => {}
        PlacementPreference::Hint(addr) if addr == base => {}
        PlacementPreference::Hint(_) => return Err(ResourceError::invalid_request()),
        PlacementPreference::PreferredNode(_) => return Err(ResourceError::unsupported_request()),
    }

    Ok(())
}

fn materialization_requires_replace(request: &ResourceRequest<'_>) -> bool {
    !matches!(
        (
            request.backing,
            request.contract.sharing,
            request.initial.residency
        ),
        (
            super::ResourceBackingRequest::Anonymous,
            super::SharingPolicy::Private,
            InitialResidency::BestEffort | InitialResidency::Locked,
        )
    )
}

fn materialization_region(
    reserved: Region,
    range: super::ResourceRange,
    granule: usize,
) -> Result<Region, ResourceError> {
    if range.len == 0 || range.offset % granule != 0 || range.len % granule != 0 {
        return Err(ResourceError::invalid_range());
    }

    reserved
        .subrange(range.offset, range.len)
        .map_err(|_| ResourceError::invalid_range())
}

fn remainder_reservation(
    provider: fusion_pal::sys::mem::PlatformMem,
    page_info: PageInfo,
    resolved: ResolvedAddressReservation,
    reserved: Region,
    offset: usize,
    len: usize,
) -> Result<Option<AddressReservation>, ResourceError> {
    if len == 0 {
        return Ok(None);
    }

    let region = reserved
        .subrange(offset, len)
        .map_err(|_| ResourceError::invalid_range())?;
    let mut next_resolved = resolved;
    next_resolved.range = region;

    Ok(Some(AddressReservation::from_parts(
        provider,
        region,
        page_info,
        next_resolved,
    )))
}
