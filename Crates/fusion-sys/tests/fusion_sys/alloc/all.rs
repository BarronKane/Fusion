use core::num::NonZeroUsize;
use core::ptr::NonNull;

use fusion_pal::sys::mem::{CachePolicy, Protect, Region};
use fusion_sys::alloc::{
    MemoryPool, MemoryPoolContributor, MemoryPoolContributorOrigin, MemoryPoolErrorKind,
    MemoryPoolExtentRequest, MemoryPoolPolicy,
};
use fusion_sys::mem::provider::{
    MemoryObjectOrigin, MemoryPoolClassId, MemoryResourceDescriptor, MemoryResourceId,
    MemoryResourceReadiness, MemoryTopologyNodeId,
};
use fusion_sys::mem::resource::{
    BoundMemoryResource, BoundResourceSpec, MemoryDomain, MemoryGeometry, MemoryResource,
    MemoryResourceHandle, OvercommitPolicy, ResourceAttrs, ResourceBackingKind, ResourceContract,
    ResourceOpSet, ResourceRange, ResourceResidencySupport, ResourceState, ResourceSupport,
    SharingPolicy, StateValue,
};

extern crate std;
use self::std::alloc::{Layout, alloc_zeroed};
use self::std::sync::Arc;
use self::std::sync::atomic::{AtomicUsize, Ordering};
use self::std::thread;
use self::std::time::Duration;

fn aligned_region(len: usize, align: usize) -> Region {
    let layout = Layout::from_size_align(len, align).expect("test layout should be valid");
    // SAFETY: the layout is valid and the allocation is intentionally leaked for the test.
    let ptr = unsafe { alloc_zeroed(layout) };
    let base = NonNull::new(ptr).expect("test allocation should succeed");
    Region { base, len }
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

fn general_support() -> ResourceSupport {
    ResourceSupport {
        protect: Protect::READ | Protect::WRITE,
        ops: ResourceOpSet::QUERY,
        advice: fusion_pal::sys::mem::MemAdviceCaps::empty(),
        residency: ResourceResidencySupport::BEST_EFFORT,
    }
}

fn bound_resource_with_shape(
    len: usize,
    domain: MemoryDomain,
    backing: ResourceBackingKind,
    attrs: ResourceAttrs,
) -> BoundMemoryResource {
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
    .expect("bound resource should build")
}

fn bound_resource(len: usize, attrs: ResourceAttrs) -> BoundMemoryResource {
    bound_resource_with_shape(
        len,
        MemoryDomain::StaticRegion,
        ResourceBackingKind::StaticRegion,
        attrs,
    )
}

fn poolable_attrs() -> ResourceAttrs {
    ResourceAttrs::ALLOCATABLE | ResourceAttrs::CACHEABLE | ResourceAttrs::COHERENT
}

#[test]
fn pool_builds_from_explicit_ready_contributors() {
    let first = MemoryPoolContributor::explicit_ready(MemoryResourceHandle::from(bound_resource(
        8192,
        poolable_attrs(),
    )));
    let second = MemoryPoolContributor::explicit_ready(MemoryResourceHandle::from(bound_resource(
        4096,
        poolable_attrs(),
    )));

    let mut builder = MemoryPool::<4, 8>::builder(MemoryPoolPolicy::ready_only());
    builder
        .add_contributor(first)
        .expect("first contributor fits");
    builder
        .add_contributor(second)
        .expect("second contributor fits");
    let pool = builder.build().expect("pool should build");
    let stats = pool.stats().expect("pool stats should be available");

    assert_eq!(stats.member_count, 2);
    assert_eq!(stats.total_bytes, 12_288);
    assert_eq!(stats.free_bytes, 12_288);
    assert_eq!(stats.leased_bytes, 0);
    assert_eq!(stats.free_extent_count, 2);
}

#[test]
fn pool_acquires_and_releases_extents_with_exact_lease_tracking() {
    let contributor = MemoryPoolContributor::explicit_ready(MemoryResourceHandle::from(
        bound_resource(8192, poolable_attrs()),
    ));
    let mut builder = MemoryPool::<2, 8>::builder(MemoryPoolPolicy::ready_only());
    builder
        .add_contributor(contributor)
        .expect("contributor should fit");
    let pool = builder.build().expect("pool should build");

    let lease_a = pool
        .acquire_extent(&MemoryPoolExtentRequest {
            len: 2048,
            align: 1024,
        })
        .expect("first extent should allocate");
    let lease_b = pool
        .acquire_extent(&MemoryPoolExtentRequest {
            len: 1024,
            align: 512,
        })
        .expect("second extent should allocate");

    assert_eq!(
        pool.lease_view(&lease_a)
            .expect("lease view should exist")
            .len(),
        2048
    );
    assert_eq!(
        pool.lease_view(&lease_b)
            .expect("lease view should exist")
            .len(),
        1024
    );

    let stats = pool.stats().expect("pool stats should be available");
    assert_eq!(stats.leased_bytes, 3072);
    assert_eq!(stats.free_bytes, 5120);

    pool.release_extent(lease_a)
        .expect("first lease should release");
    pool.release_extent(lease_b)
        .expect("second lease should release");

    let full = pool
        .acquire_extent(&MemoryPoolExtentRequest {
            len: 8192,
            align: 1024,
        })
        .expect("merged full extent should allocate");
    assert_eq!(full.len(), 8192);
}

#[test]
fn pool_rejects_incompatible_contributors() {
    let first = MemoryPoolContributor::explicit_ready(MemoryResourceHandle::from(bound_resource(
        4096,
        poolable_attrs(),
    )));
    let second = MemoryPoolContributor::explicit_ready(MemoryResourceHandle::from(bound_resource(
        4096,
        ResourceAttrs::ALLOCATABLE | ResourceAttrs::COHERENT,
    )));

    let mut builder = MemoryPool::<4, 8>::builder(MemoryPoolPolicy::ready_only());
    builder
        .add_contributor(first)
        .expect("first contributor fits");
    builder
        .add_contributor(second)
        .expect("second contributor fits");

    assert_eq!(
        builder
            .build()
            .expect_err("incompatible contributors should be rejected")
            .kind,
        MemoryPoolErrorKind::IncompatibleContributor
    );
}

#[test]
fn contributor_from_provider_descriptor_preserves_origin_and_range() {
    let resource = bound_resource(4096, poolable_attrs());
    let info = *resource.info();
    let contributor = MemoryPoolContributor::from_resource_descriptor(
        MemoryResourceHandle::from(resource),
        &MemoryResourceDescriptor {
            id: MemoryResourceId(7),
            object_id: None,
            info,
            state: ResourceState::static_state(
                StateValue::uniform(Protect::READ | Protect::WRITE),
                StateValue::uniform(false),
                StateValue::uniform(true),
            ),
            origin: MemoryObjectOrigin::Discovered,
            usable_now_len: 2048,
            usable_max_len: 2048,
            readiness: MemoryResourceReadiness::ReadyNow,
            topology_node: Some(MemoryTopologyNodeId(3)),
            pool_class: Some(MemoryPoolClassId(9)),
        },
        ResourceRange::new(0, 2048),
    )
    .expect("descriptor-backed contributor should build");

    assert_eq!(
        contributor.origin,
        MemoryPoolContributorOrigin::PresentResource(MemoryResourceId(7))
    );
    assert_eq!(contributor.pool_class, Some(MemoryPoolClassId(9)));
    assert_eq!(contributor.topology_node, Some(MemoryTopologyNodeId(3)));
    assert_eq!(contributor.usable_range, ResourceRange::new(0, 2048));
}

#[test]
fn pool_and_lease_types_are_send_and_sync_where_expected() {
    fn assert_send_sync<T: Send + Sync>() {}
    fn assert_send<T: Send>() {}

    assert_send_sync::<MemoryPool<4, 8>>();
    assert_send::<fusion_sys::alloc::MemoryPoolLease>();
}

#[test]
fn pool_reports_member_usage_after_leasing() {
    let contributor = MemoryPoolContributor::explicit_ready(MemoryResourceHandle::from(
        bound_resource(8192, poolable_attrs()),
    ));
    let mut builder = MemoryPool::<2, 8>::builder(MemoryPoolPolicy::ready_only());
    builder
        .add_contributor(contributor)
        .expect("contributor should fit");
    let pool = builder.build().expect("pool should build");
    let lease = pool
        .acquire_extent(&MemoryPoolExtentRequest {
            len: 2048,
            align: 1024,
        })
        .expect("extent should allocate");

    let info = pool
        .member_info(lease.member())
        .expect("member info should be available");
    assert_eq!(info.free_bytes, 6144);
    assert_eq!(info.leased_bytes, 2048);
}

#[test]
fn pool_rejects_split_that_exceeds_fixed_extent_metadata() {
    let contributor = MemoryPoolContributor::explicit_ready(MemoryResourceHandle::from(
        bound_resource(8192, poolable_attrs()),
    ));
    let mut builder = MemoryPool::<1, 1>::builder(MemoryPoolPolicy::ready_only());
    builder
        .add_contributor(contributor)
        .expect("contributor should fit");
    let pool = builder.build().expect("pool should build");

    assert_eq!(
        pool.acquire_extent(&MemoryPoolExtentRequest {
            len: 1024,
            align: 1024,
        })
        .expect_err("split should exceed extent metadata")
        .kind,
        MemoryPoolErrorKind::MetadataExhausted
    );

    let full = pool
        .acquire_extent(&MemoryPoolExtentRequest {
            len: 8192,
            align: 1024,
        })
        .expect("full-range lease should require no split metadata");
    assert_eq!(full.len(), 8192);
}

#[test]
fn pool_honors_actual_address_alignment_for_leased_views() {
    let contributor = MemoryPoolContributor::explicit_ready(MemoryResourceHandle::from(
        bound_resource(16 * 1024, poolable_attrs()),
    ));
    let mut builder = MemoryPool::<1, 8>::builder(MemoryPoolPolicy::ready_only());
    builder
        .add_contributor(contributor)
        .expect("contributor should fit");
    let pool = builder.build().expect("pool should build");

    let lease = pool
        .acquire_extent(&MemoryPoolExtentRequest {
            len: 1024,
            align: 2048,
        })
        .expect("aligned lease should allocate");
    let view = pool.lease_view(&lease).expect("lease view should exist");
    let raw = view.as_range_view();
    let addr = unsafe { raw.base().as_ptr() as usize };
    assert_eq!(addr % 2048, 0);
}

#[test]
fn pool_serializes_concurrent_acquire_release_cycles() {
    let first = MemoryPoolContributor::explicit_ready(MemoryResourceHandle::from(bound_resource(
        16 * 1024,
        poolable_attrs(),
    )));
    let second = MemoryPoolContributor::explicit_ready(MemoryResourceHandle::from(bound_resource(
        16 * 1024,
        poolable_attrs(),
    )));
    let mut builder = MemoryPool::<4, 32>::builder(MemoryPoolPolicy::ready_only());
    builder
        .add_contributor(first)
        .expect("first contributor fits");
    builder
        .add_contributor(second)
        .expect("second contributor fits");
    let pool = Arc::new(builder.build().expect("pool should build"));
    let completions = Arc::new(AtomicUsize::new(0));
    let mut threads = std::vec::Vec::new();

    for _ in 0..4 {
        let pool = Arc::clone(&pool);
        let completions = Arc::clone(&completions);
        threads.push(thread::spawn(move || {
            for _ in 0..200 {
                let mut lease = None;
                for attempt in 0..10_000 {
                    match pool.acquire_extent(&MemoryPoolExtentRequest {
                        len: 1024,
                        align: 1024,
                    }) {
                        Ok(acquired) => {
                            lease = Some(acquired);
                            break;
                        }
                        Err(error) if error.kind == MemoryPoolErrorKind::CapacityExhausted => {
                            if attempt % 32 == 31 {
                                thread::sleep(Duration::from_micros(50));
                            } else {
                                thread::yield_now();
                            }
                        }
                        Err(error) => panic!("unexpected pool acquire failure: {error:?}"),
                    }
                }
                let lease = lease.expect("worker should eventually acquire an extent");
                let view = pool.lease_view(&lease).expect("lease view should exist");
                assert_eq!(view.len(), 1024);
                pool.release_extent(lease)
                    .expect("lease should release cleanly");
                completions.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }

    for thread in threads {
        thread.join().expect("thread should finish");
    }

    assert_eq!(completions.load(Ordering::Relaxed), 800);
    let stats = pool.stats().expect("pool stats should be available");
    assert_eq!(stats.leased_bytes, 0);
    assert_eq!(stats.free_bytes, 32 * 1024);
}

// Look this is just for fun.
// I realized VRAM is a valid target for CPU addressable memory space.
// Leaving commented out because it's a lousy test path.
/*
#[test]
fn device_local_vram_like_pool_tracks_large_addressable_capacity() {
    const VRAM_BYTES: usize = 200 * 1024 * 1024;
    const FIRST_LEASE: usize = 64 * 1024 * 1024;
    const SECOND_LEASE: usize = 32 * 1024 * 1024;

    fn device_local_poolable_attrs() -> ResourceAttrs {
        ResourceAttrs::ALLOCATABLE
            | ResourceAttrs::DEVICE_LOCAL
            | ResourceAttrs::CACHEABLE
            | ResourceAttrs::COHERENT
    }

    let contributor = MemoryPoolContributor::explicit_ready(MemoryResourceHandle::from(
        bound_resource_with_shape(
            VRAM_BYTES,
            MemoryDomain::DeviceLocal,
            ResourceBackingKind::DeviceLocal,
            device_local_poolable_attrs(),
        ),
    ));
    let mut builder = MemoryPool::<1, 8>::builder(MemoryPoolPolicy::ready_only());
    builder
        .add_contributor(contributor)
        .expect("device-local contributor should fit");
    let pool = builder.build().expect("device-local pool should build");

    assert_eq!(pool.compatibility().domain, MemoryDomain::DeviceLocal);
    assert_eq!(
        pool.compatibility().backing,
        ResourceBackingKind::DeviceLocal
    );
    assert!(
        pool.compatibility()
            .attrs
            .contains(ResourceAttrs::DEVICE_LOCAL)
    );

    let first = pool
        .acquire_extent(&MemoryPoolExtentRequest {
            len: FIRST_LEASE,
            align: 4096,
        })
        .expect("first vram-like extent should allocate");
    let second = pool
        .acquire_extent(&MemoryPoolExtentRequest {
            len: SECOND_LEASE,
            align: 64 * 1024,
        })
        .expect("second vram-like extent should allocate");

    assert_eq!(
        pool.lease_view(&first)
            .expect("first lease should remain viewable")
            .len(),
        FIRST_LEASE
    );
    assert_eq!(
        pool.lease_view(&second)
            .expect("second lease should remain viewable")
            .len(),
        SECOND_LEASE
    );

    let stats = pool.stats().expect("pool stats should be available");
    assert_eq!(stats.total_bytes, VRAM_BYTES);
    assert_eq!(stats.leased_bytes, FIRST_LEASE + SECOND_LEASE);
    assert_eq!(stats.free_bytes, VRAM_BYTES - FIRST_LEASE - SECOND_LEASE);
}
*/
