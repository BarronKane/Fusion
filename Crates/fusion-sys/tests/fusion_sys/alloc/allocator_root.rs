use fusion_sys::alloc::{
    AllocCapabilities,
    AllocErrorKind,
    AllocModeSet,
    AllocPolicy,
    AllocationStrategy,
    Allocator,
    CriticalSafetyRequirements,
};
use fusion_sys::mem::resource::{
    MemoryDomain,
    ResourceAttrs,
    ResourceBackingKind,
};

use super::support::{bound_resource, bound_resource_with_region, byte_geometry, shifted_region};

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
    assert!(allocator.capabilities().contains(AllocCapabilities::SLAB));
    assert!(allocator.capabilities().contains(AllocCapabilities::ARENA));

    let default_domain = allocator
        .default_domain()
        .expect("default domain should exist");
    let audit = allocator
        .domain_audit(default_domain)
        .expect("domain audit should be available");
    assert_eq!(audit.info.id, default_domain);
    assert!(audit.pool_stats.is_some());
    assert!(audit.primary_layout_policy.is_some());
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
fn heap_routing_is_policy_gated_and_still_unimplemented() {
    let allocator = Allocator::<2, 2>::system_default().expect("allocator should build");
    let default_domain = allocator
        .default_domain()
        .expect("default domain should exist");
    let heap = allocator
        .heap(default_domain)
        .expect("general-purpose allocator should still surface a heap wrapper");
    assert!(heap.capabilities().is_empty());
    assert!(heap.hazards().is_empty());

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
fn allocator_can_write_domain_ids_and_audits() {
    let allocator = Allocator::<4, 4>::system_default().expect("allocator should build");
    let expected = allocator
        .default_domain()
        .expect("default domain should exist");

    let mut ids = [fusion_sys::alloc::AllocatorDomainId(u16::MAX); 4];
    let written = allocator.write_domain_ids(&mut ids);
    assert_eq!(written, 1);
    assert_eq!(ids[0], expected);

    let mut audits = [fusion_sys::alloc::AllocatorDomainAudit {
        info: allocator
            .domain(expected)
            .expect("domain info should exist"),
        primary_layout_policy: None,
        pool_stats: None,
    }; 4];
    let written = allocator
        .write_domain_audits(&mut audits)
        .expect("domain audits should write");
    assert_eq!(written, 1);
    assert_eq!(audits[0].info.id, expected);
    assert!(audits[0].pool_stats.is_some());
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
fn arena_with_alignment_preserves_full_capacity_on_misaligned_backing() {
    let mut builder = Allocator::<2, 2>::builder();
    builder
        .add_resource(bound_resource_with_region(
            shifted_region(8192, 256, 8),
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
