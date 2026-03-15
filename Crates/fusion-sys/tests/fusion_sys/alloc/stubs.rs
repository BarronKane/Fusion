use core::num::NonZeroUsize;
use core::ptr::NonNull;

use fusion_pal::sys::mem::{CachePolicy, Protect, Region};
use fusion_sys::alloc::{
    AllocErrorKind, AllocModeSet, AllocPolicy, AllocResult, Allocator, BoundedArena,
    CriticalSafetyRequirements, HeapAllocator, Slab,
};
use fusion_sys::mem::resource::{
    BoundMemoryResource, BoundResourceSpec, MemoryDomain, MemoryGeometry, MemoryResourceHandle,
    OvercommitPolicy, ResourceAttrs, ResourceBackingKind, ResourceContract, ResourceOpSet,
    ResourceResidencySupport, ResourceState, ResourceSupport, SharingPolicy, StateValue,
};

extern crate std;
use self::std::alloc::{Layout, alloc_zeroed};

fn aligned_region(len: usize, align: usize) -> Region {
    let layout = Layout::from_size_align(len, align).expect("test layout should be valid");
    // SAFETY: the layout is valid and intentionally leaked for the test.
    let ptr = unsafe { alloc_zeroed(layout) };
    let base = NonNull::new(ptr).expect("test allocation should succeed");
    Region { base, len }
}

const fn general_geometry() -> MemoryGeometry {
    MemoryGeometry {
        base_granule: NonZeroUsize::new(4096).expect("nonzero"),
        alloc_granule: NonZeroUsize::new(4096).expect("nonzero"),
        protect_granule: None,
        commit_granule: None,
        lock_granule: None,
        large_granule: None,
    }
}

fn general_contract() -> ResourceContract {
    ResourceContract {
        allowed_protect: Protect::READ | Protect::WRITE,
        write_xor_execute: true,
        sharing: SharingPolicy::Private,
        overcommit: OvercommitPolicy::Disallow,
        cache_policy: CachePolicy::Default,
        integrity: None,
    }
}

fn general_support() -> ResourceSupport {
    ResourceSupport {
        protect: Protect::READ | Protect::WRITE,
        ops: ResourceOpSet::QUERY,
        advice: fusion_pal::sys::mem::MemAdviceCaps::empty(),
        residency: ResourceResidencySupport::BEST_EFFORT,
    }
}

fn bound_resource(
    len: usize,
    domain: MemoryDomain,
    backing: ResourceBackingKind,
    attrs: ResourceAttrs,
) -> MemoryResourceHandle {
    MemoryResourceHandle::from(
        BoundMemoryResource::new(BoundResourceSpec::new(
            aligned_region(len, 4096),
            domain,
            backing,
            attrs,
            general_geometry(),
            general_contract(),
            general_support(),
            ResourceState::static_state(
                StateValue::uniform(Protect::READ | Protect::WRITE),
                StateValue::uniform(false),
                StateValue::uniform(true),
            ),
        ))
        .expect("bound resource should build"),
    )
}

#[test]
fn heap_allocator_respects_policy_gate_before_implementation_exists() {
    assert_eq!(
        HeapAllocator::new(AllocPolicy::critical_safe())
            .expect_err("critical-safe policy should deny heap allocation")
            .kind,
        AllocErrorKind::PolicyDenied
    );

    assert_eq!(
        HeapAllocator::new(AllocPolicy::general_purpose())
            .expect_err("heap implementation is still intentionally absent")
            .kind,
        AllocErrorKind::Unsupported
    );
}

#[test]
fn slab_and_arena_validate_shape_before_reporting_unsupported() {
    assert_eq!(
        Slab::<0, 4>::new(AllocPolicy::critical_safe())
            .expect_err("zero-sized slabs should be rejected")
            .kind,
        AllocErrorKind::InvalidRequest
    );
    assert_eq!(
        BoundedArena::new(0, AllocPolicy::critical_safe())
            .expect_err("zero-capacity arenas should be rejected")
            .kind,
        AllocErrorKind::InvalidRequest
    );
}

#[test]
fn allocator_builder_creates_default_domain_and_tracks_owned_resources() {
    let mut builder = Allocator::<2, 4>::builder();
    builder
        .add_resource(bound_resource(
            4096,
            MemoryDomain::StaticRegion,
            ResourceBackingKind::StaticRegion,
            ResourceAttrs::ALLOCATABLE | ResourceAttrs::CACHEABLE | ResourceAttrs::COHERENT,
        ))
        .expect("resource should fit");
    let allocator = builder.build().expect("allocator should build");

    assert_eq!(allocator.domain_count(), 1);
    assert_eq!(allocator.resource_count(), 1);

    let default_domain = allocator
        .default_domain()
        .expect("default domain should exist");
    let info = allocator
        .domain(default_domain)
        .expect("default domain info should exist");
    assert_eq!(info.resource_count, 1);
    assert!(
        info.memory_domains
            .contains(fusion_sys::alloc::MemoryDomainSet::STATIC_REGION)
    );
}

#[test]
fn allocator_builder_can_split_resources_across_explicit_domains() {
    let mut builder = Allocator::<4, 4>::builder();
    let device_domain = builder
        .add_domain(AllocPolicy::slab_only())
        .expect("explicit domain should fit");
    builder
        .add_resource(bound_resource(
            4096,
            MemoryDomain::StaticRegion,
            ResourceBackingKind::StaticRegion,
            ResourceAttrs::ALLOCATABLE | ResourceAttrs::CACHEABLE | ResourceAttrs::COHERENT,
        ))
        .expect("default-domain resource should fit");
    builder
        .add_resource_to_domain(
            device_domain,
            bound_resource(
                8192,
                MemoryDomain::DeviceLocal,
                ResourceBackingKind::DeviceLocal,
                ResourceAttrs::ALLOCATABLE
                    | ResourceAttrs::DEVICE_LOCAL
                    | ResourceAttrs::CACHEABLE
                    | ResourceAttrs::COHERENT,
            ),
        )
        .expect("device-domain resource should fit");
    let allocator = builder.build().expect("allocator should build");

    assert_eq!(allocator.domain_count(), 2);
    let default_info = allocator
        .domain(
            allocator
                .default_domain()
                .expect("default domain should exist"),
        )
        .expect("default domain info should exist");
    let device_info = allocator
        .domain(device_domain)
        .expect("device domain info should exist");

    assert!(
        default_info
            .memory_domains
            .contains(fusion_sys::alloc::MemoryDomainSet::STATIC_REGION)
    );
    assert!(
        device_info
            .memory_domains
            .contains(fusion_sys::alloc::MemoryDomainSet::DEVICE_LOCAL)
    );
    assert!(device_info.attrs.contains(ResourceAttrs::DEVICE_LOCAL));
}

#[test]
fn allocator_root_routes_heap_and_strategy_policy_honestly() {
    let allocator = Allocator::<2, 2>::system_default();
    let default_domain = allocator
        .default_domain()
        .expect("system default should expose a default domain");
    assert_eq!(
        allocator
            .heap(default_domain)
            .expect_err("heap remains unsupported until implementation lands")
            .kind,
        AllocErrorKind::Unsupported
    );

    let critical = Allocator::<2, 2>::builder()
        .build()
        .expect("allocator should build");
    assert_eq!(
        critical
            .malloc(4096)
            .expect_err("critical-safe allocator should deny implicit heap routing")
            .kind,
        AllocErrorKind::PolicyDenied
    );
}

#[test]
fn allocator_root_routes_slab_and_arena_honestly() {
    let critical = Allocator::<2, 2>::builder()
        .build()
        .expect("allocator should build");
    let default_domain = critical
        .default_domain()
        .expect("allocator should expose a default domain");

    assert_eq!(
        critical
            .slab::<64, 8>(default_domain)
            .expect_err("slab backing is still intentionally absent")
            .kind,
        AllocErrorKind::Unsupported
    );
    assert_eq!(
        critical
            .arena(default_domain, 4096)
            .expect_err("arena backing is still intentionally absent")
            .kind,
        AllocErrorKind::Unsupported
    );

    let mut builder = Allocator::<4, 2>::builder();
    let heap_only = builder
        .add_domain(AllocPolicy {
            modes: AllocModeSet::HEAP,
            safety: CriticalSafetyRequirements::empty(),
        })
        .expect("explicit domain should fit");
    let allocator = builder.build().expect("allocator should build");

    assert_eq!(
        allocator
            .slab::<64, 8>(heap_only)
            .expect_err("heap-only domain should deny slab routing")
            .kind,
        AllocErrorKind::PolicyDenied
    );
    assert_eq!(
        allocator
            .arena(heap_only, 4096)
            .expect_err("heap-only domain should deny arena routing")
            .kind,
        AllocErrorKind::PolicyDenied
    );
}

#[test]
fn allocator_root_realloc_and_free_fail_honestly() {
    let allocator = Allocator::<2, 2>::system_default();
    let allocation = AllocResult {
        ptr: NonNull::dangling(),
        len: 64,
        align: 16,
        domain: MemoryDomain::StaticRegion,
        attrs: ResourceAttrs::ALLOCATABLE | ResourceAttrs::CACHEABLE | ResourceAttrs::COHERENT,
        hazards: fusion_sys::alloc::ResourceHazardSet::empty(),
        geometry: general_geometry(),
    };

    assert_eq!(
        allocator
            .realloc(allocation, 0)
            .expect_err("realloc still fails at the unsupported heap boundary")
            .kind,
        AllocErrorKind::Unsupported
    );
    assert_eq!(
        allocator
            .realloc(allocation, 128)
            .expect_err("realloc remains intentionally unsupported")
            .kind,
        AllocErrorKind::Unsupported
    );
    assert_eq!(
        allocator
            .free(allocation)
            .expect_err("free remains intentionally unsupported")
            .kind,
        AllocErrorKind::Unsupported
    );
}

#[test]
fn allocator_builder_reports_metadata_exhaustion_for_domains_and_resources() {
    let mut one_domain = Allocator::<1, 1>::builder();
    one_domain
        .add_domain(AllocPolicy::slab_only())
        .expect("first domain should fit");
    assert_eq!(
        one_domain
            .add_domain(AllocPolicy::arena_only())
            .expect_err("second domain should exhaust fixed metadata")
            .kind,
        AllocErrorKind::MetadataExhausted
    );

    let mut one_resource = Allocator::<2, 1>::builder();
    one_resource
        .add_resource(bound_resource(
            4096,
            MemoryDomain::StaticRegion,
            ResourceBackingKind::StaticRegion,
            ResourceAttrs::ALLOCATABLE | ResourceAttrs::CACHEABLE | ResourceAttrs::COHERENT,
        ))
        .expect("first resource should fit");
    assert_eq!(
        one_resource
            .add_resource(bound_resource(
                4096,
                MemoryDomain::StaticRegion,
                ResourceBackingKind::StaticRegion,
                ResourceAttrs::ALLOCATABLE | ResourceAttrs::CACHEABLE | ResourceAttrs::COHERENT,
            ))
            .expect_err("second resource should exhaust fixed metadata")
            .kind,
        AllocErrorKind::MetadataExhausted
    );
}
