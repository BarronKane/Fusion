use core::ptr::NonNull;

use fusion_pal::sys::mem::{
    Backing, CachePolicy, MapFlags, MapReplaceRequest, MapRequest, MemBackingCaps, MemBase,
    MemCaps, MemMap, MemMapReplace, MemPlacementCaps, MemProtect, MemQuery, PageInfo, Placement,
    Protect, Region, RegionAttrs, RegionInfo, ReplacePlacement, system_mem,
};

use super::{
    InitialResidency, MemoryGeometry, PlacementPreference, RequiredPlacement, ResourceError,
    ResourceOpSet, ResourcePreferenceSet, ResourceRequest, ResourceState, VirtualMemoryResource,
    backing_kind_from_request, build_resolved_resource, finalize_post_map_state,
    geometry_from_page_info, initial_map_flags, normalize_len, preferred_placement_to_mem,
    request_backing_to_mem, required_placement_to_mem, resource_acquire_support_from_mem_support,
    resource_attrs_for_request, resource_attrs_from_request, supports_huge_page_advice,
    supports_map_time_huge_pages, validate_request, verify_required_placement,
};

bitflags::bitflags! {
    /// Operations that an address reservation may support before materialization.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ReservationOpSet: u32 {
        /// Best-effort query of the reserved address range is supported.
        const QUERY                 = 1 << 0;
        /// Anonymous private materialization can occur in place.
        const MATERIALIZE_IN_PLACE  = 1 << 1;
        /// Replacement materialization can swap another backing into the reserved range.
        const MATERIALIZE_REPLACE   = 1 << 2;
    }
}

bitflags::bitflags! {
    /// Hazards inherent to reservation materialization paths.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ReservationHazardSet: u32 {
        /// Some materialization flows require hazardous replace mapping.
        const REPLACE_MATERIALIZATION = 1 << 0;
    }
}

/// Support surface for acquiring and materializing an address reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReservationSupport {
    /// Backings that may be reserved or later materialized from this reservation flow.
    pub backings: MemBackingCaps,
    /// Placement modes supported while acquiring the reservation.
    pub placements: MemPlacementCaps,
    /// Backings that can be materialized without replacing the reservation mapping.
    pub in_place_backings: MemBackingCaps,
    /// Backings that can be materialized through replacement mapping.
    pub replace_backings: MemBackingCaps,
    /// Reservation operations supported by the backend.
    pub ops: ReservationOpSet,
    /// Hazards inherent to the supported materialization paths.
    pub hazards: ReservationHazardSet,
}

/// Creation-time resolution metadata for an address reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolvedAddressReservation {
    /// Contiguous reserved address range.
    pub range: Region,
    /// Geometry of the reservation and any future materialized resources.
    pub geometry: MemoryGeometry,
    /// Support surface for the reservation instance.
    pub support: ReservationSupport,
    /// Soft preferences that were not honored when the reservation was created.
    pub unmet_preferences: ResourcePreferenceSet,
}

/// Result of materializing a subrange of an address reservation.
#[derive(Debug)]
pub struct MaterializedReservation {
    /// Remaining reservation that precedes the materialized resource, if any.
    pub leading: Option<AddressReservation>,
    /// Newly materialized resource covering the requested subrange.
    pub resource: VirtualMemoryResource,
    /// Remaining reservation that follows the materialized resource, if any.
    pub trailing: Option<AddressReservation>,
}

/// Request for acquiring a raw address reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReservationRequest<'a> {
    /// Optional human-readable name for diagnostics or provider-specific bookkeeping.
    pub name: Option<&'a str>,
    /// Requested reservation length in bytes before backend rounding.
    pub len: usize,
    /// Soft placement preference for the reservation.
    pub placement: PlacementPreference,
    /// Hard placement requirement for the reservation.
    pub required_placement: Option<RequiredPlacement>,
}

impl ReservationRequest<'_> {
    /// Returns a default reservation request for `len` bytes.
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

/// Reserved address-space range that can later be materialized into one or more resources.
#[derive(Debug)]
pub struct AddressReservation {
    provider: fusion_pal::sys::mem::PlatformMem,
    region: Option<Region>,
    page_info: PageInfo,
    resolved: ResolvedAddressReservation,
}

impl AddressReservation {
    /// Creates a reservation handle from already-resolved parts.
    const fn from_parts(
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

    /// Returns system-wide reservation acquisition support for the current backend.
    #[must_use]
    pub fn system_support() -> ReservationSupport {
        reservation_support_from_mem_support(system_mem().support())
    }

    /// Acquires a new address reservation.
    ///
    /// # Errors
    /// Returns an error when the request is invalid, unsupported by the backend, or the
    /// reservation cannot be created.
    pub fn create(request: &ReservationRequest<'_>) -> Result<Self, ResourceError> {
        let provider = system_mem();
        let page_info = provider.page_info();
        let mem_support = provider.support();
        let support = reservation_support_from_mem_support(mem_support);
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
            if let Ok(region) = unsafe { provider.map(&preferred_request) } {
                if !super::placement_preference_honored(provider, request.placement, region) {
                    unmet |= ResourcePreferenceSet::PLACEMENT;
                }
                region
            } else {
                unmet |= ResourcePreferenceSet::PLACEMENT;
                unsafe { provider.map(&required_request) }
                    .map_err(ResourceError::from_request_error)?
            }
        } else {
            unsafe { provider.map(&required_request) }.map_err(ResourceError::from_request_error)?
        };

        verify_required_placement(provider, region, request.required_placement)?;

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

    /// Returns page and granule information associated with the reservation backend.
    #[must_use]
    pub const fn page_info(&self) -> PageInfo {
        self.page_info
    }

    /// Returns the reservation support surface for this instance.
    #[must_use]
    pub const fn support(&self) -> ReservationSupport {
        self.resolved.support
    }

    /// Returns the creation-time resolution metadata for the reservation.
    #[must_use]
    pub const fn resolved(&self) -> ResolvedAddressReservation {
        self.resolved
    }

    /// Returns the reserved address range governed by this handle.
    ///
    /// # Panics
    /// Panics only if internal materialization logic has already consumed the reservation's
    /// owned range while this handle is still being used.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn region(&self) -> Region {
        self.region
            .expect("reservation region missing during active use")
    }

    /// Returns `true` when `ptr` lies within the reserved range.
    #[must_use]
    pub fn contains(&self, ptr: *const u8) -> bool {
        self.region().contains(ptr as usize)
    }

    /// Returns a checked subrange of the reservation.
    ///
    /// # Errors
    /// Returns an error when the requested range is empty or falls outside the reservation.
    pub fn subregion(&self, range: super::ResourceRange) -> Result<Region, ResourceError> {
        if range.len == 0 {
            return Err(ResourceError::invalid_range());
        }

        self.region()
            .subrange(range.offset, range.len)
            .map_err(|_| ResourceError::invalid_range())
    }

    /// Queries reservation metadata for the region containing `addr`.
    ///
    /// # Errors
    /// Returns an error when query is unsupported for this reservation, when `addr` lies
    /// outside the reserved range, or when the backend rejects the query.
    pub fn query(&self, addr: NonNull<u8>) -> Result<RegionInfo, ResourceError> {
        if !self.support().ops.contains(ReservationOpSet::QUERY) {
            return Err(ResourceError::unsupported_operation());
        }

        if !self.contains(addr.as_ptr()) {
            return Err(ResourceError::invalid_range());
        }

        self.provider
            .query(addr)
            .map_err(ResourceError::from_operation_error)
    }

    /// Materializes the entire reservation into a single virtual memory resource.
    ///
    /// # Errors
    /// Returns an error when the request is incompatible with the reservation or when the
    /// backend cannot materialize the requested backing.
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

    /// Materializes a subrange of the reservation and returns any leading or trailing remains.
    ///
    /// # Errors
    /// Returns an error when the range is invalid, when the request is incompatible with the
    /// chosen subrange, or when the backend cannot materialize the requested backing.
    #[allow(clippy::too_many_lines)]
    pub fn materialize_range(
        mut self,
        range: super::ResourceRange,
        request: &ResourceRequest<'_>,
    ) -> Result<MaterializedReservation, ResourceError> {
        let mem_support = self.provider.support();
        let acquire_support = resource_acquire_support_from_mem_support(mem_support);
        let resource_support = acquire_support.instance;
        let reserved = self.region();
        let region = materialization_region(reserved, range, self.page_info.alloc_granule.get())?;
        validate_request(request, acquire_support)?;
        validate_materialization_request(request, region, self.page_info.alloc_granule.get())?;

        let use_replace = materialization_requires_replace(request);
        let backing_kind = backing_kind_from_request(request.backing, request.contract.sharing);
        let mut unmet = ResourcePreferenceSet::empty();
        let (region, actual_flags) = if use_replace {
            if !self
                .support()
                .ops
                .contains(ReservationOpSet::MATERIALIZE_REPLACE)
            {
                return Err(ResourceError::unsupported_request());
            }

            let base_flags = initial_map_flags(request, resource_support)?;
            let mut preferred_flags = base_flags;

            if request
                .preferences
                .contains(ResourcePreferenceSet::PREFAULT)
                && !preferred_flags.contains(MapFlags::POPULATE)
            {
                if resource_support
                    .residency
                    .contains(super::ResourceResidencySupport::PREFAULT)
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

            let base_request = MapReplaceRequest {
                len: region.len,
                align: 0,
                protect: request.initial.protect,
                flags: base_flags,
                attrs: resource_attrs_for_request(request.backing),
                cache: request.contract.cache_policy,
                placement: ReplacePlacement::FixedReplace(region.base.as_ptr() as usize),
                backing: request_backing_to_mem(request.backing),
            };
            let preferred_request = MapReplaceRequest {
                flags: preferred_flags,
                ..base_request
            };

            if preferred_request.flags == base_request.flags {
                (
                    unsafe { self.provider.map_replace(&base_request) }
                        .map_err(ResourceError::from_request_error)?,
                    base_flags,
                )
            } else if let Ok(region) = unsafe { self.provider.map_replace(&preferred_request) } {
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

                (
                    unsafe { self.provider.map_replace(&base_request) }
                        .map_err(ResourceError::from_request_error)?,
                    base_flags,
                )
            }
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

            if request
                .preferences
                .contains(ResourcePreferenceSet::PREFAULT)
            {
                unmet |= ResourcePreferenceSet::PREFAULT;
            }

            (region, MapFlags::empty())
        };

        let actual_locked = finalize_post_map_state(
            self.provider,
            region,
            request,
            mem_support,
            actual_flags,
            &mut unmet,
        )?;

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
        Some(RequiredPlacement::RequiredNode(_) | RequiredPlacement::RegionId(_)) => {
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

const fn materialization_requires_replace(request: &ResourceRequest<'_>) -> bool {
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
    if range.len == 0 || !range.offset.is_multiple_of(granule) || !range.len.is_multiple_of(granule)
    {
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
