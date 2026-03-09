use core::ptr::NonNull;

use fusion_pal::sys::mem::{Placement, Protect, Region, RegionInfo};

use super::{
    MemoryDomain, MemoryGeometry, MemoryResource, QueryableResource, ResolvedResource,
    ResourceAttrs, ResourceBackingKind, ResourceContract, ResourceError, ResourceHazardSet,
    ResourceInfo, ResourceOpSet, ResourcePreferenceSet, ResourceState, ResourceSupport, StateValue,
    core::ResourceCore, infer_resource_hazards, resource_region_attrs_from_attrs,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BoundResourceSpec {
    pub range: Region,
    pub domain: MemoryDomain,
    pub backing: ResourceBackingKind,
    pub attrs: ResourceAttrs,
    pub geometry: MemoryGeometry,
    pub contract: ResourceContract,
    pub support: ResourceSupport,
    pub additional_hazards: ResourceHazardSet,
    pub initial_state: ResourceState,
}

impl BoundResourceSpec {
    #[must_use]
    pub const fn new(
        range: Region,
        domain: MemoryDomain,
        backing: ResourceBackingKind,
        attrs: ResourceAttrs,
        geometry: MemoryGeometry,
        contract: ResourceContract,
        support: ResourceSupport,
        initial_state: ResourceState,
    ) -> Self {
        Self {
            range,
            domain,
            backing,
            attrs,
            geometry,
            contract,
            support,
            additional_hazards: ResourceHazardSet::empty(),
            initial_state,
        }
    }
}

#[derive(Debug)]
pub struct BoundMemoryResource {
    core: ResourceCore,
}

impl BoundMemoryResource {
    pub fn new(spec: BoundResourceSpec) -> Result<Self, ResourceError> {
        validate_bound_spec(&spec)?;

        let hazards = infer_resource_hazards(spec.contract, spec.attrs) | spec.additional_hazards;
        let resolved = ResolvedResource {
            info: ResourceInfo {
                range: spec.range,
                domain: spec.domain,
                backing: spec.backing,
                attrs: spec.attrs,
                geometry: spec.geometry,
                contract: spec.contract,
                support: spec.support,
                hazards,
            },
            initial_state: spec.initial_state,
            unmet_preferences: ResourcePreferenceSet::empty(),
        };

        Ok(Self {
            core: ResourceCore::new(resolved, spec.initial_state),
        })
    }

    #[must_use]
    pub const fn resolved(&self) -> ResolvedResource {
        self.core.resolved()
    }
}

impl MemoryResource for BoundMemoryResource {
    fn info(&self) -> &ResourceInfo {
        self.core.info()
    }

    fn state(&self) -> ResourceState {
        self.core.state()
    }
}

impl QueryableResource for BoundMemoryResource {
    fn query(&self, addr: NonNull<u8>) -> Result<RegionInfo, ResourceError> {
        if !self.ops().contains(ResourceOpSet::QUERY) {
            return Err(ResourceError::unsupported_operation());
        }

        if !self.range().contains(addr.as_ptr() as usize) {
            return Err(ResourceError::invalid_range());
        }

        Ok(RegionInfo {
            region: self.range(),
            protect: match self.state().current_protect {
                StateValue::Uniform(protect) => protect,
                StateValue::Asymmetric | StateValue::Unknown => Protect::NONE,
            },
            attrs: resource_region_attrs_from_attrs(self.attrs()),
            cache: self.contract().cache_policy,
            placement: Placement::Anywhere,
            committed: !matches!(self.state().committed, StateValue::Uniform(false)),
        })
    }
}

fn validate_bound_spec(spec: &BoundResourceSpec) -> Result<(), ResourceError> {
    if spec.range.len == 0 {
        return Err(ResourceError::invalid_request());
    }

    if !backing_matches_domain(spec.backing, spec.domain) {
        return Err(ResourceError::invalid_request());
    }

    let supported_ops = ResourceOpSet::QUERY;
    if !(spec.support.ops - supported_ops).is_empty() {
        return Err(ResourceError::invalid_request());
    }

    if let StateValue::Uniform(protect) = spec.initial_state.current_protect {
        if !spec.contract.allowed_protect.contains(protect)
            || !spec.support.protect.contains(protect)
        {
            return Err(ResourceError::invalid_request());
        }
    }

    if spec.contract.write_xor_execute
        && matches!(
            spec.initial_state.current_protect,
            StateValue::Uniform(protect)
                if protect.contains(Protect::WRITE) && protect.contains(Protect::EXEC)
        )
    {
        return Err(ResourceError::contract_violation());
    }

    Ok(())
}

fn backing_matches_domain(backing: ResourceBackingKind, domain: MemoryDomain) -> bool {
    match backing {
        ResourceBackingKind::AnonymousPrivate
        | ResourceBackingKind::AnonymousShared
        | ResourceBackingKind::FilePrivate
        | ResourceBackingKind::FileShared => matches!(domain, MemoryDomain::VirtualAddressSpace),
        ResourceBackingKind::Mmio => matches!(domain, MemoryDomain::Mmio),
        ResourceBackingKind::DeviceLocal => matches!(domain, MemoryDomain::DeviceLocal),
        ResourceBackingKind::Physical => matches!(domain, MemoryDomain::Physical),
        ResourceBackingKind::Borrowed
        | ResourceBackingKind::StaticRegion
        | ResourceBackingKind::Partition => {
            matches!(
                domain,
                MemoryDomain::StaticRegion | MemoryDomain::Physical | MemoryDomain::Mmio
            )
        }
    }
}
