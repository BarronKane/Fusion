//! Cortex-M bare-metal memory backend.
//!
//! Cortex-M does not offer a generic virtual-memory story, but the selected SoC still knows a
//! fair amount about its static memory map. This backend therefore keeps mapping unsupported
//! while surfacing static query/catalog answers for board-known regions.

use core::num::NonZeroUsize;

use crate::contract::hardware::mem::{
    Address,
    Advise,
    MapFlags,
    MapReplaceRequest,
    MapRequest,
    MemAdviceCaps,
    MemAdvise,
    MemAllocatorLayoutPolicy,
    MemAllocatorLayoutRealization,
    MemBackingCaps,
    MemBase,
    MemCaps,
    MemCatalog,
    MemCatalogCaps,
    MemCatalogResource,
    MemCatalogResourceId,
    MemCatalogResourceOrigin,
    MemCatalogSupport,
    MemCommit,
    MemDomain,
    MemDomainSet,
    MemError,
    MemGeometry,
    MemLock,
    MemMap,
    MemMapReplace,
    MemOvercommitPolicy,
    MemPlacementCaps,
    MemPoolResourceReadiness,
    MemProtect,
    MemQuery,
    MemResourceAttrs,
    MemResourceContract,
    MemResourceEnvelope,
    MemResourceHazardSet,
    MemResourceOpSet,
    MemResourceResidencySupport,
    MemResourceStateSummary,
    MemResourceSupport,
    MemSharingPolicy,
    MemStateValue,
    MemSupport,
    PageInfo,
    Placement,
    Protect,
    Region,
    RegionAttrs,
    RegionInfo,
};

use crate::pal::soc::cortex_m::hal::soc::board::{
    self,
    CortexMMemoryRegionDescriptor,
    CortexMMemoryRegionKind,
};

/// Cortex-M bare-metal memory provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct CortexMMem;

/// Target-selected memory provider alias for Cortex-M builds.
pub type PlatformMem = CortexMMem;

/// Cortex-M has no page table. Use a conservative 4-byte granule for static-region metadata.
const CORTEX_M_GRANULE: NonZeroUsize = NonZeroUsize::new(4).unwrap();

/// Returns the process-wide Cortex-M memory provider handle.
#[must_use]
pub const fn system_mem() -> PlatformMem {
    PlatformMem::new()
}

impl CortexMMem {
    /// Creates a new Cortex-M memory provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl MemBase for CortexMMem {
    fn caps(&self) -> MemCaps {
        MemCaps::QUERY
    }

    fn support(&self) -> MemSupport {
        MemSupport {
            caps: MemCaps::QUERY,
            map_flags: MapFlags::empty(),
            protect: Protect::empty(),
            backings: MemBackingCaps::empty(),
            placements: MemPlacementCaps::empty(),
            advice: MemAdviceCaps::empty(),
        }
    }

    fn page_info(&self) -> PageInfo {
        PageInfo {
            base_page: CORTEX_M_GRANULE,
            alloc_granule: CORTEX_M_GRANULE,
            huge_page: None,
        }
    }
}

impl MemMap for CortexMMem {
    unsafe fn map(&self, _req: &MapRequest<'_>) -> Result<Region, MemError> {
        Err(MemError::unsupported())
    }

    unsafe fn unmap(&self, _region: Region) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

unsafe impl MemMapReplace for CortexMMem {
    unsafe fn map_replace(&self, _req: &MapReplaceRequest<'_>) -> Result<Region, MemError> {
        Err(MemError::unsupported())
    }
}

impl MemProtect for CortexMMem {
    unsafe fn protect(&self, _region: Region, _protect: Protect) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

impl MemCommit for CortexMMem {}

impl MemQuery for CortexMMem {
    fn query(&self, addr: Address) -> Result<RegionInfo, MemError> {
        let address = addr.get();
        let descriptor = selected_owned_memory_region_containing(address)
            .or_else(|| {
                selected_memory_map()
                    .iter()
                    .copied()
                    .find(|descriptor| contains_addr(descriptor.base, descriptor.len, address))
            })
            .ok_or_else(MemError::invalid_addr)?;

        let region = region_from_descriptor(descriptor);

        Ok(RegionInfo {
            region,
            protect: descriptor.protect,
            attrs: descriptor.attrs,
            cache: descriptor.cache,
            placement: Placement::FixedNoReplace(descriptor.base),
            committed: true,
        })
    }
}

impl MemAdvise for CortexMMem {
    unsafe fn advise(&self, _region: Region, _advice: Advise) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

impl MemLock for CortexMMem {
    unsafe fn lock(&self, _region: Region) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }

    unsafe fn unlock(&self, _region: Region) -> Result<(), MemError> {
        Err(MemError::unsupported())
    }
}

impl MemCatalog for CortexMMem {
    fn catalog_support(&self) -> MemCatalogSupport {
        let mut discovered_domains = MemDomainSet::empty();

        for descriptor in selected_memory_map() {
            match domain_from_descriptor(*descriptor) {
                MemDomain::StaticRegion => discovered_domains |= MemDomainSet::STATIC_REGION,
                MemDomain::Mmio => discovered_domains |= MemDomainSet::MMIO,
                MemDomain::Physical => discovered_domains |= MemDomainSet::PHYSICAL,
                MemDomain::VirtualAddressSpace => {
                    discovered_domains |= MemDomainSet::VIRTUAL_ADDRESS_SPACE;
                }
                MemDomain::DeviceLocal => discovered_domains |= MemDomainSet::DEVICE_LOCAL,
            }
        }

        for index in 0..selected_owned_memory_region_count() {
            let Some(descriptor) = selected_owned_memory_region(index) else {
                continue;
            };
            match domain_from_descriptor(descriptor) {
                MemDomain::StaticRegion => discovered_domains |= MemDomainSet::STATIC_REGION,
                MemDomain::Mmio => discovered_domains |= MemDomainSet::MMIO,
                MemDomain::Physical => discovered_domains |= MemDomainSet::PHYSICAL,
                MemDomain::VirtualAddressSpace => {
                    discovered_domains |= MemDomainSet::VIRTUAL_ADDRESS_SPACE;
                }
                MemDomain::DeviceLocal => discovered_domains |= MemDomainSet::DEVICE_LOCAL,
            }
        }

        MemCatalogSupport {
            caps: MemCatalogCaps::RESOURCE_INVENTORY,
            discovered_domains,
            acquirable_domains: MemDomainSet::empty(),
        }
    }

    fn resource_count(&self) -> usize {
        selected_owned_memory_region_count().saturating_add(selected_memory_map().len())
    }

    fn resource(&self, index: usize) -> Option<MemCatalogResource> {
        let descriptor = selected_catalog_resource(index)?;

        Some(MemCatalogResource {
            id: MemCatalogResourceId(u32::try_from(index).ok()?),
            envelope: MemResourceEnvelope {
                domain: domain_from_descriptor(descriptor),
                backing: descriptor.backing,
                attrs: resource_attrs_from_descriptor(descriptor),
                geometry: geometry_from_descriptor(descriptor),
                layout: layout_from_descriptor(descriptor),
                contract: MemResourceContract {
                    allowed_protect: descriptor.protect,
                    write_xor_execute: false,
                    sharing: MemSharingPolicy::Private,
                    overcommit: MemOvercommitPolicy::Disallow,
                    cache_policy: descriptor.cache,
                    integrity: None,
                },
                support: MemResourceSupport {
                    protect: descriptor.protect,
                    ops: MemResourceOpSet::QUERY,
                    advice: MemAdviceCaps::empty(),
                    residency: MemResourceResidencySupport::empty(),
                },
                hazards: hazards_from_descriptor(descriptor),
            },
            cpu_range: Some(region_from_descriptor(descriptor)),
            usable_now_len: usable_len_from_descriptor(descriptor),
            usable_max_len: usable_len_from_descriptor(descriptor),
            state: MemResourceStateSummary {
                current_protect: MemStateValue::Uniform(descriptor.protect),
                locked: MemStateValue::Uniform(false),
                committed: MemStateValue::Uniform(true),
            },
            readiness: MemPoolResourceReadiness::ReadyNow,
            origin: MemCatalogResourceOrigin::Discovered,
            topology_node: None,
        })
    }
}

fn selected_memory_map() -> &'static [CortexMMemoryRegionDescriptor] {
    board::memory_map()
}

fn selected_owned_memory_region_count() -> usize {
    board::owned_memory_region_count()
}

fn selected_owned_memory_region(index: usize) -> Option<CortexMMemoryRegionDescriptor> {
    board::owned_memory_region(index)
}

fn selected_owned_memory_region_containing(addr: usize) -> Option<CortexMMemoryRegionDescriptor> {
    for index in 0..selected_owned_memory_region_count() {
        let descriptor = selected_owned_memory_region(index)?;
        if contains_addr(descriptor.base, descriptor.len, addr) {
            return Some(descriptor);
        }
    }

    None
}

fn selected_catalog_resource(index: usize) -> Option<CortexMMemoryRegionDescriptor> {
    let owned_count = selected_owned_memory_region_count();
    if index < owned_count {
        return selected_owned_memory_region(index);
    }

    selected_memory_map().get(index - owned_count).copied()
}

fn contains_addr(base: usize, len: usize, addr: usize) -> bool {
    base.checked_add(len)
        .is_some_and(|end| addr >= base && addr < end)
}

const fn region_from_descriptor(descriptor: CortexMMemoryRegionDescriptor) -> Region {
    Region {
        base: Address::new(descriptor.base),
        len: descriptor.len,
    }
}

const fn domain_from_descriptor(descriptor: CortexMMemoryRegionDescriptor) -> MemDomain {
    match descriptor.kind {
        CortexMMemoryRegionKind::Mmio => MemDomain::Mmio,
        _ => MemDomain::StaticRegion,
    }
}

const fn geometry_from_descriptor(descriptor: CortexMMemoryRegionDescriptor) -> MemGeometry {
    let protect_granule = if matches!(descriptor.kind, CortexMMemoryRegionKind::Mmio) {
        None
    } else {
        Some(CORTEX_M_GRANULE)
    };

    MemGeometry {
        base_granule: CORTEX_M_GRANULE,
        alloc_granule: CORTEX_M_GRANULE,
        protect_granule,
        commit_granule: None,
        lock_granule: None,
        large_granule: None,
    }
}

const fn layout_from_descriptor(
    _descriptor: CortexMMemoryRegionDescriptor,
) -> MemAllocatorLayoutPolicy {
    MemAllocatorLayoutPolicy {
        metadata_granule: CORTEX_M_GRANULE,
        min_extent_align: CORTEX_M_GRANULE,
        default_arena_align: CORTEX_M_GRANULE,
        default_slab_align: CORTEX_M_GRANULE,
        realization: MemAllocatorLayoutRealization::EagerPhysical,
    }
}

fn resource_attrs_from_descriptor(descriptor: CortexMMemoryRegionDescriptor) -> MemResourceAttrs {
    let mut attrs = MemResourceAttrs::empty();

    if descriptor.allocatable {
        attrs |= MemResourceAttrs::ALLOCATABLE;
    }
    if descriptor.attrs.contains(RegionAttrs::DMA_VISIBLE) {
        attrs |= MemResourceAttrs::DMA_VISIBLE;
    }
    if descriptor.attrs.contains(RegionAttrs::CACHEABLE) {
        attrs |= MemResourceAttrs::CACHEABLE;
    }
    if descriptor.attrs.contains(RegionAttrs::COHERENT) {
        attrs |= MemResourceAttrs::COHERENT;
    }
    if descriptor.attrs.contains(RegionAttrs::PHYS_CONTIGUOUS) {
        attrs |= MemResourceAttrs::PHYS_CONTIGUOUS;
    }
    if descriptor.attrs.contains(RegionAttrs::TAGGED) {
        attrs |= MemResourceAttrs::TAGGED;
    }
    if descriptor.attrs.contains(RegionAttrs::INTEGRITY_MANAGED) {
        attrs |= MemResourceAttrs::INTEGRITY_MANAGED;
    }
    if descriptor.attrs.contains(RegionAttrs::STATIC_REGION) {
        attrs |= MemResourceAttrs::STATIC_REGION;
    }
    if matches!(descriptor.kind, CortexMMemoryRegionKind::Mmio) {
        attrs |= MemResourceAttrs::HAZARDOUS_IO;
    }

    attrs
}

fn hazards_from_descriptor(descriptor: CortexMMemoryRegionDescriptor) -> MemResourceHazardSet {
    let mut hazards = MemResourceHazardSet::empty();

    if descriptor.protect.contains(Protect::EXEC) {
        hazards |= MemResourceHazardSet::EXECUTABLE;
    }
    if matches!(descriptor.kind, CortexMMemoryRegionKind::Mmio) {
        hazards |=
            MemResourceHazardSet::MMIO_SIDE_EFFECTS | MemResourceHazardSet::EXTERNAL_MUTATION;
    }

    hazards
}

const fn usable_len_from_descriptor(descriptor: CortexMMemoryRegionDescriptor) -> usize {
    if descriptor.allocatable {
        descriptor.len
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::{CortexMMemoryRegionDescriptor, CortexMMemoryRegionKind, region_from_descriptor};
    use crate::contract::hardware::mem::{
        CachePolicy,
        MemResourceBackingKind,
        Protect,
        RegionAttrs,
    };

    #[test]
    fn address_zero_region_is_not_materialized_as_region_handle() {
        let descriptor = CortexMMemoryRegionDescriptor {
            name: "rom",
            kind: CortexMMemoryRegionKind::Rom,
            base: 0,
            len: 4096,
            protect: Protect::READ.union(Protect::EXEC),
            attrs: RegionAttrs::STATIC_REGION.union(RegionAttrs::EXECUTABLE),
            cache: CachePolicy::Default,
            backing: MemResourceBackingKind::StaticRegion,
            allocatable: false,
        };

        let region = region_from_descriptor(descriptor);
        assert_eq!(region.base.get(), 0);
        assert_eq!(region.len, 4096);
    }
}
