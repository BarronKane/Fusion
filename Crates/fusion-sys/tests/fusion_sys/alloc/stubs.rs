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

fn shifted_region(len: usize, align: usize, offset: usize) -> Region {
    let layout = Layout::from_size_align(
        len.checked_add(offset).expect("shifted region should fit"),
        align,
    )
    .expect("shifted layout should be valid");
    // SAFETY: the layout is valid and intentionally leaked for the test.
    let ptr = unsafe { alloc_zeroed(layout) };
    let base = NonNull::new(unsafe { ptr.add(offset) }).expect("shifted allocation should exist");
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

const fn byte_geometry() -> MemoryGeometry {
    MemoryGeometry {
        base_granule: NonZeroUsize::new(1).expect("nonzero"),
        alloc_granule: NonZeroUsize::new(1).expect("nonzero"),
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
    bound_resource_with_region(
        aligned_region(len, 4096),
        general_geometry(),
        domain,
        backing,
        attrs,
    )
}

fn bound_resource_with_region(
    region: Region,
    geometry: MemoryGeometry,
    domain: MemoryDomain,
    backing: ResourceBackingKind,
    attrs: ResourceAttrs,
) -> MemoryResourceHandle {
    MemoryResourceHandle::from(
        BoundMemoryResource::new(BoundResourceSpec::new(
            region,
            domain,
            backing,
            attrs,
            geometry,
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
fn system_default_with_capacity_sizes_backing_for_requested_slabs() {
    let allocator =
        Allocator::<2, 2>::system_default_with_capacity(256).expect("allocator should build");
    let default_domain = allocator
        .default_domain()
        .expect("default domain should exist");
    let slab = allocator
        .slab::<64, 4>(default_domain)
        .expect("requested backing should satisfy the exact slab footprint");

    let mut allocations = std::vec::Vec::new();
    for _ in 0..4 {
        allocations.push(
            slab.allocate(&fusion_sys::alloc::AllocRequest::new(64))
                .expect("slab should consume the requested backing capacity"),
        );
    }

    assert_eq!(
        slab.allocate(&fusion_sys::alloc::AllocRequest::new(64))
            .expect_err("exact-capacity slab should exhaust after four slots")
            .kind,
        AllocErrorKind::CapacityExhausted
    );

    for allocation in allocations {
        slab.deallocate(allocation)
            .expect("exact-capacity slab allocation should release");
    }
}

#[test]
fn arena_with_alignment_preserves_full_capacity_on_misaligned_backing() {
    let mut builder = Allocator::<2, 2>::builder();
    builder
        .add_resource(bound_resource_with_region(
            shifted_region(4096, 256, 8),
            byte_geometry(),
            MemoryDomain::StaticRegion,
            ResourceBackingKind::StaticRegion,
            ResourceAttrs::ALLOCATABLE | ResourceAttrs::CACHEABLE | ResourceAttrs::COHERENT,
        ))
        .expect("resource should fit");
    let allocator = builder.build().expect("allocator should build");
    let default_domain = allocator
        .default_domain()
        .expect("default domain should exist");
    let arena = allocator
        .arena_with_alignment(default_domain, 128, 64)
        .expect("arena should reserve aligned usable backing");

    let first = arena
        .allocate(&fusion_sys::alloc::AllocRequest {
            len: 64,
            align: 64,
            zeroed: false,
        })
        .expect("first aligned allocation should fit");
    let second = arena
        .allocate(&fusion_sys::alloc::AllocRequest {
            len: 64,
            align: 64,
            zeroed: false,
        })
        .expect("second aligned allocation should consume the remaining advertised capacity");

    assert_eq!(first.align, 64);
    assert_eq!(second.align, 64);
    assert_eq!(
        arena
            .allocate(&fusion_sys::alloc::AllocRequest {
                len: 1,
                align: 1,
                zeroed: false,
            })
            .expect_err("arena should exhaust after its advertised usable capacity")
            .kind,
        AllocErrorKind::CapacityExhausted
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
fn arena_supports_typed_slice_metadata_allocation() {
    let allocator = Allocator::<2, 2>::system_default().expect("allocator should build");
    let default_domain = allocator
        .default_domain()
        .expect("default domain should exist");
    let arena = allocator
        .arena(default_domain, 256)
        .expect("arena should reserve backing");

    let mut entries = arena
        .alloc_array_with(4, |index| {
            u32::try_from(index * 3).expect("index should fit")
        })
        .expect("typed arena slice should allocate");
    assert_eq!(entries.as_slice(), &[0, 3, 6, 9]);

    entries[2] = 12;
    assert_eq!(entries.as_slice(), &[0, 3, 12, 9]);
}

#[test]
fn arena_reset_rejects_live_typed_leases_and_recovers_after_drop() {
    let allocator = Allocator::<2, 2>::system_default().expect("allocator should build");
    let default_domain = allocator
        .default_domain()
        .expect("default domain should exist");
    let arena = allocator
        .arena(default_domain, 256)
        .expect("arena should reserve backing");

    let slice = arena
        .alloc_array_with(2, |index| {
            u32::try_from(index + 1).expect("index should fit")
        })
        .expect("typed arena slice should allocate");
    assert_eq!(
        arena
            .reset()
            .expect_err("reset must reject live typed leases")
            .kind,
        AllocErrorKind::Busy
    );
    drop(slice);
    arena
        .reset()
        .expect("arena reset should succeed once all slices are gone");
}

#[test]
fn typed_arena_slice_keeps_backing_alive_after_wrapper_drop() {
    let allocator = Allocator::<2, 2>::system_default().expect("allocator should build");
    let default_domain = allocator
        .default_domain()
        .expect("default domain should exist");
    let arena = allocator
        .arena(default_domain, 256)
        .expect("arena should reserve backing");

    let mut slice = arena
        .alloc_array_with(3, |index| u32::try_from(index).expect("index should fit"))
        .expect("typed arena slice should allocate");
    drop(arena);

    slice[1] = 9;
    assert_eq!(slice.as_slice(), &[0, 9, 2]);
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
