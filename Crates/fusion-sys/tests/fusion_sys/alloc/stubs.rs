use core::num::NonZeroUsize;
use core::ptr::NonNull;

use fusion_pal::sys::mem::{CachePolicy, Protect, Region};
use fusion_sys::alloc::{
    AllocErrorKind, AllocModeSet, AllocPolicy, AllocationStrategy, Allocator,
    CriticalSafetyRequirements,
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
fn system_default_constructs_a_real_backed_allocator() {
    let allocator = Allocator::<2, 2>::system_default().expect("allocator should build");
    assert_eq!(allocator.domain_count(), 1);
    assert_eq!(allocator.resource_count(), 1);
    assert!(
        allocator
            .capabilities()
            .contains(fusion_sys::alloc::AllocCapabilities::SLAB)
    );
    assert!(
        allocator
            .capabilities()
            .contains(fusion_sys::alloc::AllocCapabilities::ARENA)
    );
}

#[test]
fn heap_routing_is_policy_gated_and_still_unimplemented() {
    let allocator = Allocator::<2, 2>::system_default().expect("allocator should build");
    assert_eq!(
        allocator
            .malloc(4096)
            .expect_err("heap remains intentionally unimplemented")
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
fn slab_validates_shape_and_reuses_capacity() {
    let allocator = Allocator::<2, 2>::system_default().expect("allocator should build");
    let default_domain = allocator
        .default_domain()
        .expect("default domain should exist");

    assert_eq!(
        allocator
            .slab::<0, 4>(default_domain)
            .expect_err("zero-sized slabs should be rejected")
            .kind,
        AllocErrorKind::InvalidRequest
    );

    let slab = allocator
        .slab::<64, 4>(default_domain)
        .expect("slab should reserve backing");
    let first = slab
        .allocate(&fusion_sys::alloc::AllocRequest::new(32))
        .expect("first slab allocation should succeed");
    let second = slab
        .allocate(&fusion_sys::alloc::AllocRequest::zeroed(64))
        .expect("second slab allocation should succeed");
    assert_eq!(first.len, 64);
    assert_eq!(second.align, 64);

    slab.deallocate(first)
        .expect("first slab allocation should release");
    slab.deallocate(second)
        .expect("second slab allocation should release");

    for _ in 0..4 {
        let allocation = slab
            .allocate(&fusion_sys::alloc::AllocRequest::new(64))
            .expect("slab should recycle freed slots");
        slab.deallocate(allocation)
            .expect("recycled slab allocation should release");
    }
}

#[test]
fn arena_provides_bounded_bump_allocation_and_reset() {
    let allocator = Allocator::<2, 2>::system_default().expect("allocator should build");
    let default_domain = allocator
        .default_domain()
        .expect("default domain should exist");

    assert_eq!(
        allocator
            .arena(default_domain, 0)
            .expect_err("zero-capacity arenas should be rejected")
            .kind,
        AllocErrorKind::InvalidRequest
    );

    let arena = allocator
        .arena(default_domain, 256)
        .expect("arena should reserve backing");
    let first = arena
        .allocate(&fusion_sys::alloc::AllocRequest {
            len: 32,
            align: 16,
            zeroed: false,
        })
        .expect("first arena allocation should succeed");
    let second = arena
        .allocate(&fusion_sys::alloc::AllocRequest {
            len: 64,
            align: 32,
            zeroed: true,
        })
        .expect("second arena allocation should succeed");
    assert_eq!(second.ptr.as_ptr() as usize % 32, 0);

    arena
        .deallocate(second)
        .expect("most recent arena allocation should release");
    arena
        .deallocate(first)
        .expect("earlier allocation becomes releasable after stack unwind");
    arena.reset().expect("arena reset should succeed");
}

#[test]
fn heap_only_domains_deny_slab_and_arena_routing() {
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
