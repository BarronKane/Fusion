use core::num::NonZeroUsize;

use super::{
    CachePolicy,
    IntegrityMode,
    MemAdviceCaps,
    MemBackingCaps,
    MemPlacementCaps,
    Protect,
    Region,
    TagMode,
};
use super::{
    MemTopologyLink,
    MemTopologyNode,
    MemTopologyNodeId,
};

bitflags::bitflags! {
    /// Set of memory domains a fusion-pal catalog can describe or acquire.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MemDomainSet: u32 {
        /// Ordinary virtual-address-backed memory.
        const VIRTUAL_ADDRESS_SPACE = 1 << 0;
        /// Device-local memory such as VRAM or accelerator-managed heaps.
        const DEVICE_LOCAL          = 1 << 1;
        /// Physically addressed memory regions.
        const PHYSICAL              = 1 << 2;
        /// Fixed static or externally provided regions.
        const STATIC_REGION         = 1 << 3;
        /// MMIO-like regions with device side effects.
        const MMIO                  = 1 << 4;
    }
}

/// Coarse domain classification for a cataloged memory object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemDomain {
    /// Ordinary virtual-address-space backed memory.
    VirtualAddressSpace,
    /// Device-local memory exposed through the fusion-pal.
    DeviceLocal,
    /// Physical or physically-addressed memory.
    Physical,
    /// Static or externally provided region-backed memory.
    StaticRegion,
    /// Memory-mapped I/O or similarly hazardous device regions.
    Mmio,
}

/// Concrete backing shape represented by a cataloged memory object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemResourceBackingKind {
    /// Private anonymous virtual memory.
    AnonymousPrivate,
    /// Shared anonymous virtual memory.
    AnonymousShared,
    /// Privately mapped file-backed memory.
    FilePrivate,
    /// Shared file-backed memory.
    FileShared,
    /// Borrowed region supplied by the surrounding environment.
    Borrowed,
    /// Fixed static region with platform-defined lifetime.
    StaticRegion,
    /// RTOS- or firmware-managed memory partition.
    Partition,
    /// Device-local backing such as GPU or accelerator heaps.
    DeviceLocal,
    /// Physical backing selected by address or physical descriptor.
    Physical,
    /// MMIO-like mapping with device side effects.
    Mmio,
}

bitflags::bitflags! {
    /// Catalog-visible intrinsic attributes of a memory object.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MemResourceAttrs: u32 {
        /// The range is suitable for general-purpose allocator or pool use.
        const ALLOCATABLE        = 1 << 0;
        /// The backing is fundamentally read-only even if other metadata is mutable.
        const READ_ONLY_BACKING  = 1 << 1;
        /// The range is visible to DMA-capable devices.
        const DMA_VISIBLE        = 1 << 2;
        /// The range resides in device-local memory rather than ordinary host memory.
        const DEVICE_LOCAL       = 1 << 3;
        /// The range participates in normal CPU caching.
        const CACHEABLE          = 1 << 4;
        /// The range participates in the expected coherency domain.
        const COHERENT           = 1 << 5;
        /// The backing is physically contiguous.
        const PHYS_CONTIGUOUS    = 1 << 6;
        /// The range carries hardware tag semantics.
        const TAGGED             = 1 << 7;
        /// The range participates in a platform integrity-management regime.
        const INTEGRITY_MANAGED  = 1 << 8;
        /// The range refers to a fixed static region rather than a fresh acquisition.
        const STATIC_REGION      = 1 << 9;
        /// The range has MMIO-like or otherwise hazardous side-effecting behavior.
        const HAZARDOUS_IO       = 1 << 10;
        /// The range preserves state across restart until explicitly cleared.
        const PERSISTENT         = 1 << 11;
    }
}

bitflags::bitflags! {
    /// Operations a cataloged memory object can legally expose after binding.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MemResourceOpSet: u32 {
        /// Protection changes are supported.
        const PROTECT  = 1 << 0;
        /// Advisory hints are supported.
        const ADVISE   = 1 << 1;
        /// Residency lock/unlock is supported.
        const LOCK     = 1 << 2;
        /// Region query is supported.
        const QUERY    = 1 << 3;
        /// Commit of reserved backing is supported.
        const COMMIT   = 1 << 4;
        /// Decommit while preserving reservation is supported.
        const DECOMMIT = 1 << 5;
        /// Semantic discard/reset of contents is supported.
        const DISCARD  = 1 << 6;
        /// Explicit persistence or cache flush is supported.
        const FLUSH    = 1 << 7;
    }
}

bitflags::bitflags! {
    /// Residency behaviors a cataloged memory object or strategy can support.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MemResourceResidencySupport: u32 {
        /// Ordinary best-effort residency is supported.
        const BEST_EFFORT = 1 << 0;
        /// Prefault or eager population can be requested.
        const PREFAULT    = 1 << 1;
        /// A verified lock or pin operation is supported.
        const LOCKED      = 1 << 2;
    }
}

bitflags::bitflags! {
    /// Optional acquisition-time features a strategy may expose.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MemStrategyFeatureSupport: u32 {
        /// Stronger no-overcommit acquisition semantics are available.
        const OVERCOMMIT_DISALLOW = 1 << 0;
        /// Non-default cache-policy requests are available.
        const CACHE_POLICY        = 1 << 1;
        /// Integrity or tag-mode acquisition requests are available.
        const INTEGRITY           = 1 << 2;
    }
}

bitflags::bitflags! {
    /// Soft preferences that acquisition may try to honor but may legally miss.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MemStrategyPreferenceSet: u32 {
        /// Prefer a non-default placement strategy.
        const PLACEMENT  = 1 << 0;
        /// Prefer eager population or prefaulting.
        const PREFAULT   = 1 << 1;
        /// Prefer initial residency locking.
        const LOCK       = 1 << 2;
        /// Prefer large-page backing at acquisition time or huge-page advice after mapping.
        const HUGE_PAGES = 1 << 3;
    }
}

bitflags::bitflags! {
    /// Catalog-visible inherent hazards of a memory object.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MemResourceHazardSet: u32 {
        /// Executable code may exist or be permitted within the resource contract.
        const EXECUTABLE                 = 1 << 0;
        /// Shared aliasing or externally visible writes can occur.
        const SHARED_ALIASING            = 1 << 1;
        /// Some semantics are emulated rather than enforced by the platform.
        const EMULATED                   = 1 << 2;
        /// Overcommit or lazy commitment may cause later allocation failure.
        const OVERCOMMIT                 = 1 << 3;
        /// The range is not fully coherent with all relevant agents.
        const NON_COHERENT               = 1 << 4;
        /// State may change outside the control of this handle.
        const EXTERNAL_MUTATION          = 1 << 5;
        /// Access may trigger device-visible side effects.
        const MMIO_SIDE_EFFECTS          = 1 << 6;
        /// Data persistence requires explicit flush-like operations.
        const PERSISTENCE_REQUIRES_FLUSH = 1 << 7;
    }
}

bitflags::bitflags! {
    /// Coarse catalog capabilities reported by a fusion-pal backend.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MemCatalogCaps: u32 {
        /// Exposes an inventory of discovered or bindable resources.
        const RESOURCE_INVENTORY   = 1 << 0;
        /// Exposes an inventory of acquisition strategies.
        const STRATEGY_INVENTORY   = 1 << 1;
        /// Exposes a normalized topology view.
        const TOPOLOGY             = 1 << 2;
        /// Resource inventory is expected to be complete.
        const EXHAUSTIVE_RESOURCES = 1 << 3;
        /// Topology inventory is expected to be complete.
        const EXHAUSTIVE_TOPOLOGY  = 1 << 4;
    }
}

/// Sharing contract for a cataloged memory object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemSharingPolicy {
    /// Resource contents are private to a single address space or owner.
    Private,
    /// Resource contents may be visible through shared aliases.
    Shared,
}

/// Integrity and tag-mode constraints for a cataloged memory object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemIntegrityConstraints {
    /// Requested or discovered integrity mode for the backing.
    pub mode: IntegrityMode,
    /// Optional tag-mode policy layered on top of the integrity regime.
    pub tag: Option<TagMode>,
}

/// Overcommit regime described by a cataloged memory object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemOvercommitPolicy {
    /// The platform may rely on lazy commitment or overcommit.
    Allow,
    /// The platform can provide stronger no-overcommit semantics.
    Disallow,
}

/// Granularity information for a cataloged pool-capable resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemGeometry {
    /// Smallest meaningful granule for the domain, usually the base page size.
    pub base_granule: NonZeroUsize,
    /// Minimum acquisition or materialization granule.
    pub alloc_granule: NonZeroUsize,
    /// Protection-change granule when protection control exists.
    pub protect_granule: Option<NonZeroUsize>,
    /// Commit/decommit granule when commitment control exists.
    pub commit_granule: Option<NonZeroUsize>,
    /// Lock/unlock granule when residency locking exists.
    pub lock_granule: Option<NonZeroUsize>,
    /// Larger granule such as huge-page size when the backend exposes one.
    pub large_granule: Option<NonZeroUsize>,
}

/// Coarse realization regime the allocator layout policy is shaped for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemAllocatorLayoutRealization {
    /// Backing is virtual or reservation-backed and only materializes lazily as needed.
    LazyVirtual,
    /// Backing is immediate physical or statically bound memory and should stay thin.
    EagerPhysical,
}

/// Allocator-facing metadata and extent packing policy for one cataloged resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemAllocatorLayoutPolicy {
    /// Granule used when allocator metadata needs rounding.
    pub metadata_granule: NonZeroUsize,
    /// Minimum alignment allocator-managed extents should request.
    pub min_extent_align: NonZeroUsize,
    /// Default maximum alignment to assume for general bounded arenas.
    pub default_arena_align: NonZeroUsize,
    /// Default maximum alignment to assume for slab payloads.
    pub default_slab_align: NonZeroUsize,
    /// Broad realization regime this policy is shaped for.
    pub realization: MemAllocatorLayoutRealization,
}

/// Per-property summary for a resource-wide catalog state value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemStateValue<T> {
    /// The property is uniform across the described resource.
    Uniform(T),
    /// The property differs across subranges of the resource.
    Asymmetric,
    /// The catalog cannot currently prove a resource-wide answer.
    Unknown,
}

/// Catalog-visible runtime state summary for a pool-capable resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemResourceStateSummary {
    /// Resource-wide summary of the current protection state.
    pub current_protect: MemStateValue<Protect>,
    /// Resource-wide summary of lock state.
    pub locked: MemStateValue<bool>,
    /// Resource-wide summary of commitment state.
    pub committed: MemStateValue<bool>,
}

/// Current readiness of a cataloged pool-capable resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemPoolResourceReadiness {
    /// The resource is immediately usable for pooling.
    ReadyNow,
    /// The resource exists but requires commit-like backing activation first.
    RequiresCommit,
    /// The resource exists descriptively but must be materialized first.
    RequiresMaterialization,
    /// The resource exists but requires some other legal state transition first.
    RequiresStateTransition,
    /// The catalog cannot presently make this resource pool-usable.
    Unavailable,
}

/// Runtime support surface of a cataloged memory object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemResourceSupport {
    /// Protection bits that this object can legally hold.
    pub protect: Protect,
    /// Operations the object may expose once bound.
    pub ops: MemResourceOpSet,
    /// Advisory hints accepted for the object.
    pub advice: MemAdviceCaps,
    /// Residency behaviors accepted for the object.
    pub residency: MemResourceResidencySupport,
}

/// Immutable contract of a cataloged memory object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemResourceContract {
    /// Maximum protection set the object may ever hold.
    pub allowed_protect: Protect,
    /// Whether writable and executable protections must remain mutually exclusive.
    pub write_xor_execute: bool,
    /// Sharing behavior the object must preserve.
    pub sharing: MemSharingPolicy,
    /// Overcommit policy expected for the object.
    pub overcommit: MemOvercommitPolicy,
    /// Cache policy expected for the object.
    pub cache_policy: CachePolicy,
    /// Optional integrity/tag-mode constraints.
    pub integrity: Option<MemIntegrityConstraints>,
}

/// Pool-visible envelope of a cataloged pool-capable resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemResourceEnvelope {
    /// Domain classification for the object.
    pub domain: MemDomain,
    /// Concrete backing kind for the object.
    pub backing: MemResourceBackingKind,
    /// Intrinsic attributes of the object.
    pub attrs: MemResourceAttrs,
    /// Operation granularity information.
    pub geometry: MemGeometry,
    /// Allocator-facing metadata and extent layout policy.
    pub layout: MemAllocatorLayoutPolicy,
    /// Immutable contract of the object.
    pub contract: MemResourceContract,
    /// Runtime support surface of the object.
    pub support: MemResourceSupport,
    /// Inherent hazards of the object.
    pub hazards: MemResourceHazardSet,
}

/// Stable identifier for a catalog-known resource record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemCatalogResourceId(pub u32);

/// Stable identifier for a catalog-known acquisition strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemCatalogStrategyId(pub u32);

/// Provenance class for a catalog-known concrete resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemCatalogResourceOrigin {
    /// Resource already existed as a discovered or bound range.
    Discovered,
    /// Resource was actively created by the surrounding environment.
    Created,
    /// Resource is borrowed from an external owner with catalog bookkeeping.
    Borrowed,
    /// Resource was materialized from a reservation or similar placeholder.
    Materialized,
}

/// Coarse acquisition story for a catalog strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemCatalogStrategyKind {
    /// Hosted virtual-memory creation path such as anonymous or file-backed mappings.
    VirtualCreate,
    /// Bind an externally governed static, physical, or board-defined range.
    BindExisting,
    /// Materialize from a reserved address window.
    ReservationMaterialize,
    /// Map or bind direct physical memory.
    PhysicalMap,
    /// Map or bind device-local or MMIO-style memory.
    DeviceMap,
    /// Use a backend-native pool or partition source.
    NativePool,
}

/// Capacity description for a catalog strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemCatalogStrategyCapacity {
    /// Smallest request size the strategy is willing to serve.
    pub min_len: usize,
    /// Largest request size the strategy can serve when known.
    pub max_len: Option<usize>,
    /// Allocation or acquisition granule used by the strategy.
    pub granule: NonZeroUsize,
}

/// Catalog support summary for resource and strategy discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemCatalogSupport {
    /// Coarse inventory capabilities of the catalog.
    pub caps: MemCatalogCaps,
    /// Domains for which the catalog knows about existing resources.
    pub discovered_domains: MemDomainSet,
    /// Domains the catalog may be able to create or materialize later.
    pub acquirable_domains: MemDomainSet,
}

/// Catalog-known descriptor for a concrete memory object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemCatalogResource {
    /// Stable catalog-local resource identifier.
    pub id: MemCatalogResourceId,
    /// Pool-visible envelope for the object when it is CPU-addressable and pool-capable.
    pub envelope: MemResourceEnvelope,
    /// CPU-addressable range when one exists in the current execution context.
    pub cpu_range: Option<Region>,
    /// Bytes the catalog considers immediately pool-usable from this object.
    pub usable_now_len: usize,
    /// Maximum bytes the catalog considers potentially pool-usable after legal preparation.
    pub usable_max_len: usize,
    /// Runtime state summary for the object when it is CPU-addressable and pool-capable.
    pub state: MemResourceStateSummary,
    /// Current readiness classification for pool use.
    pub readiness: MemPoolResourceReadiness,
    /// Source story for the object.
    pub origin: MemCatalogResourceOrigin,
    /// Optional topology node associated with the object.
    pub topology_node: Option<MemTopologyNodeId>,
}

/// Pool-capable output envelope of a cataloged acquisition strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemCatalogStrategyOutput {
    /// Pool-visible output envelope this strategy can produce in this mode.
    pub envelope: MemResourceEnvelope,
    /// Readiness expected on the created or materialized resource.
    pub readiness: MemPoolResourceReadiness,
    /// Optional topology node naturally associated with the output.
    pub topology_node: Option<MemTopologyNodeId>,
}

/// Catalog-known descriptor for a way to create or materialize more memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemCatalogStrategy {
    /// Stable catalog-local strategy identifier.
    pub id: MemCatalogStrategyId,
    /// Coarse acquisition story represented by the strategy.
    pub kind: MemCatalogStrategyKind,
    /// Domains the strategy can produce.
    pub domains: MemDomainSet,
    /// Backing kinds the strategy can create.
    pub backings: MemBackingCaps,
    /// Placement modes the strategy can accept.
    pub placements: MemPlacementCaps,
    /// Optional acquisition-only feature support.
    pub features: MemStrategyFeatureSupport,
    /// Soft preferences the strategy may try to honor.
    pub preferences: MemStrategyPreferenceSet,
    /// Capacity limits or granularity for the strategy.
    pub capacity: MemCatalogStrategyCapacity,
    /// Pool-capable output envelope when this strategy can create a CPU-addressable pool
    /// resource.
    pub output: Option<MemCatalogStrategyOutput>,
}

/// Backend-neutral catalog of memory inventory and topology.
///
/// This trait is intentionally platform-independent. A Linux backend may populate it from
/// `procfs`, syscalls, or nothing at all. A firmware or RTOS backend may populate it from a
/// BSP, linker script, device tree, or board table. None of that translation belongs above
/// this trait boundary.
pub trait MemCatalogContract: super::MemBaseContract {
    /// Returns the catalog's coarse support surface.
    fn catalog_support(&self) -> MemCatalogSupport {
        MemCatalogSupport {
            caps: MemCatalogCaps::empty(),
            discovered_domains: MemDomainSet::empty(),
            acquirable_domains: MemDomainSet::empty(),
        }
    }

    /// Returns the number of topology nodes available through the catalog.
    fn topology_node_count(&self) -> usize {
        0
    }

    /// Returns a topology node descriptor by index.
    fn topology_node(&self, _index: usize) -> Option<MemTopologyNode> {
        None
    }

    /// Returns the number of topology links available through the catalog.
    fn topology_link_count(&self) -> usize {
        0
    }

    /// Returns a topology link descriptor by index.
    fn topology_link(&self, _index: usize) -> Option<MemTopologyLink> {
        None
    }

    /// Returns the number of cataloged concrete resources.
    fn resource_count(&self) -> usize {
        0
    }

    /// Returns a cataloged concrete resource descriptor by index.
    fn resource(&self, _index: usize) -> Option<MemCatalogResource> {
        None
    }

    /// Returns the number of cataloged acquisition strategies.
    fn strategy_count(&self) -> usize {
        0
    }

    /// Returns a cataloged acquisition strategy descriptor by index.
    fn strategy(&self, _index: usize) -> Option<MemCatalogStrategy> {
        None
    }
}
