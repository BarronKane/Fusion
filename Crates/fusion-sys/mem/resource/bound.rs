//! Bound-resource support for externally governed contiguous memory ranges.
//!
//! This module is the honest path for memory that already exists and is not actively acquired
//! through the virtual-memory creation flow in [`super::VirtualMemoryResource`]. The caller
//! supplies a `BoundResourceSpec` describing an existing CPU-addressable range, and the result
//! is a `BoundMemoryResource` that carries the same `MemoryResource`-level contract as any
//! other resource instance.
//!
//! This matters for future non-VM targets. On bare-metal, RTOS, firmware, or board-specific
//! deployments, many resources will be discovered rather than created:
//!
//! - linker-defined SRAM regions
//! - DMA-visible carveouts
//! - fixed static partitions
//! - mapped physical windows
//! - MMIO-like apertures with device-visible side effects
//!
//! Those ranges should not be funneled through a virtual-memory request shape just because the
//! hosted path uses one. A future `MemoryProvider` should inventory the hardware or firmware
//! topology, decide which concrete ranges exist, and bind them through this module when they
//! are already present. The provider can then expose allocator-relevant semantics such as
//! `DMA_VISIBLE`, `PHYS_CONTIGUOUS`, `COHERENT`, `INTEGRITY_MANAGED`, or `HAZARDOUS_IO`
//! without implying that the range was freshly allocated here.
//!
//! Bound resources are still subject to the same core rule as the rest of this subsystem: the
//! metadata must remain truthful. Support bits, state summaries, and hazards should describe
//! only what the environment can actually prove. This module intentionally rejects internally
//! inconsistent specifications rather than normalizing them into something prettier and less
//! honest.
//!
//! Like the rest of `fusion-sys::mem::resource`, bound resources expose borrowed range views as
//! the primary safe surface. The underlying fusion-pal `Region` remains crate-internal metadata rather
//! than a public ownership-adjacent token that callers can copy and squirrel away forever.
//!
//! The current bound-resource implementation is deliberately conservative. It is primarily for
//! describing externally governed ranges, and today it only accepts the `QUERY` operation when
//! the bound state is precise enough to answer point queries truthfully. That is enough for the
//! initial inventory-and-govern use case. If future targets need MPU protection changes, cache
//! maintenance, flush, or lock-style operations on bound ranges, those capabilities can be
//! added here later without changing the more important architectural split: created virtual
//! memory and pre-existing board memory are different acquisition stories, even when both end
//! up represented as `MemoryResource`s.

use core::num::NonZeroUsize;

use fusion_pal::sys::mem::{Address, CachePolicy, Placement, Protect, Region, RegionInfo};

use super::{
    MemoryDomain,
    MemoryGeometry,
    MemoryResource,
    QueryableResource,
    ResolvedResource,
    ResourceAttrs,
    ResourceBackingKind,
    ResourceContract,
    ResourceError,
    ResourceHazardSet,
    ResourceInfo,
    ResourceOpSet,
    ResourcePreferenceSet,
    ResourceResidencySupport,
    ResourceState,
    ResourceSupport,
    SharingPolicy,
    StateValue,
    core::ResourceCore,
    infer_resource_hazards,
    resource_region_attrs_from_attrs,
};

/// Specification for binding an externally governed range into a `MemoryResource`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BoundResourceSpec {
    /// Contiguous governed range represented by the bound resource.
    pub range: Region,
    /// Domain classification for the bound range.
    pub domain: MemoryDomain,
    /// Concrete backing kind for the bound range.
    pub backing: ResourceBackingKind,
    /// Intrinsic attributes of the bound range.
    pub attrs: ResourceAttrs,
    /// Operation granularity metadata for the range.
    pub geometry: MemoryGeometry,
    /// Immutable contract that higher layers must continue to honor.
    pub contract: ResourceContract,
    /// Runtime support surface the bound resource may expose.
    pub support: ResourceSupport,
    /// Additional hazards not inferred from contract and attributes alone.
    pub additional_hazards: ResourceHazardSet,
    /// Initial summary state for the range.
    ///
    /// When [`ResourceOpSet::QUERY`] is advertised, this must be precise enough to synthesize
    /// a truthful point query for the whole bound range.
    pub initial_state: ResourceState,
}

impl BoundResourceSpec {
    /// Creates a bound-resource specification with no additional hazards.
    #[allow(clippy::too_many_arguments)]
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

/// Concrete memory resource for externally governed ranges that are not actively acquired here.
#[derive(Debug)]
pub struct BoundMemoryResource {
    core: ResourceCore,
}

impl BoundMemoryResource {
    /// Binds a specification into a concrete resource handle.
    ///
    /// # Errors
    /// Returns an error when the supplied range, domain, contract, or support surface is
    /// internally inconsistent.
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

    /// Binds one caller-owned static allocatable region using the canonical deterministic
    /// bare-metal contract.
    ///
    /// This is the board/application-facing path for SRAM-backed runtime slabs that already
    /// exist and just need to be governed truthfully.
    ///
    /// # Errors
    ///
    /// Returns an error when the supplied region is empty or otherwise inconsistent.
    pub fn static_allocatable_region(region: Region) -> Result<Self, ResourceError> {
        Self::new(BoundResourceSpec::new(
            region,
            MemoryDomain::StaticRegion,
            ResourceBackingKind::StaticRegion,
            ResourceAttrs::ALLOCATABLE
                | ResourceAttrs::STATIC_REGION
                | ResourceAttrs::CACHEABLE
                | ResourceAttrs::COHERENT,
            static_allocatable_geometry(),
            static_allocatable_contract(),
            static_allocatable_support(),
            static_allocatable_state(),
        ))
    }

    /// Binds one caller-owned static allocatable byte range using the canonical deterministic
    /// bare-metal contract.
    ///
    /// # Safety
    ///
    /// The caller must guarantee the supplied pointer/length pair names one valid writable static
    /// memory extent for the whole lifetime of the bound resource.
    ///
    /// # Errors
    ///
    /// Returns an error when the supplied range is empty or otherwise inconsistent.
    pub unsafe fn static_allocatable_bytes(
        ptr: *mut u8,
        len: usize,
    ) -> Result<Self, ResourceError> {
        Self::static_allocatable_region(Region {
            base: Address::new(ptr as usize),
            len,
        })
    }

    /// Returns the creation-time resolution metadata for the bound resource.
    #[must_use]
    pub const fn resolved(&self) -> ResolvedResource {
        self.core.resolved()
    }
}

fn static_allocatable_geometry() -> MemoryGeometry {
    MemoryGeometry {
        base_granule: NonZeroUsize::new(1).expect("non-zero"),
        alloc_granule: NonZeroUsize::new(1).expect("non-zero"),
        protect_granule: None,
        commit_granule: None,
        lock_granule: None,
        large_granule: None,
    }
}

fn static_allocatable_contract() -> ResourceContract {
    ResourceContract {
        allowed_protect: Protect::READ | Protect::WRITE,
        write_xor_execute: true,
        sharing: SharingPolicy::Private,
        overcommit: super::OvercommitPolicy::Disallow,
        cache_policy: CachePolicy::Default,
        integrity: None,
    }
}

fn static_allocatable_support() -> ResourceSupport {
    ResourceSupport {
        protect: Protect::READ | Protect::WRITE,
        ops: ResourceOpSet::QUERY,
        advice: fusion_pal::sys::mem::MemAdviceCaps::empty(),
        residency: ResourceResidencySupport::BEST_EFFORT,
    }
}

fn static_allocatable_state() -> ResourceState {
    ResourceState::static_state(
        StateValue::Uniform(Protect::READ | Protect::WRITE),
        StateValue::Uniform(false),
        StateValue::Uniform(true),
    )
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
    /// Returns a query record synthesized from the bound resource's own metadata.
    ///
    /// # Errors
    /// Returns an error when query is not supported for this bound resource, when `addr`
    /// does not lie within the governed range, or when the bound state is not precise enough
    /// to answer the point query truthfully.
    fn query(&self, addr: Address) -> Result<RegionInfo, ResourceError> {
        if !self.ops().contains(ResourceOpSet::QUERY) {
            return Err(ResourceError::unsupported_operation());
        }

        if !self.range().contains_addr(addr) {
            return Err(ResourceError::invalid_range());
        }

        Ok(RegionInfo {
            region: self.info().range,
            protect: match self.state().current_protect {
                StateValue::Uniform(protect) => protect,
                StateValue::Asymmetric | StateValue::Unknown => {
                    return Err(ResourceError::unsupported_operation());
                }
            },
            attrs: resource_region_attrs_from_attrs(self.attrs()),
            cache: self.contract().cache_policy,
            placement: Placement::Anywhere,
            committed: match self.state().committed {
                StateValue::Uniform(committed) => committed,
                StateValue::Asymmetric | StateValue::Unknown => {
                    return Err(ResourceError::unsupported_operation());
                }
            },
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

    if spec.support.ops.contains(ResourceOpSet::QUERY) {
        if !matches!(spec.initial_state.current_protect, StateValue::Uniform(_)) {
            return Err(ResourceError::invalid_request());
        }
        if !matches!(spec.initial_state.committed, StateValue::Uniform(_)) {
            return Err(ResourceError::invalid_request());
        }
    }

    if let StateValue::Uniform(protect) = spec.initial_state.current_protect
        && (!spec.contract.allowed_protect.contains(protect)
            || !spec.support.protect.contains(protect))
    {
        return Err(ResourceError::invalid_request());
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

const fn backing_matches_domain(backing: ResourceBackingKind, domain: MemoryDomain) -> bool {
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
