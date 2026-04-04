use core::num::NonZeroUsize;
use core::ptr::NonNull;

use fusion_pal::sys::mem::{
    Address,
    CachePolicy,
    Protect,
    Region,
};
use fusion_sys::alloc::{
    Allocator,
    MemoryPoolExtentRequest,
};
use fusion_sys::mem::resource::{
    AllocatorLayoutPolicy,
    BoundMemoryResource,
    BoundResourceSpec,
    MemoryDomain,
    MemoryGeometry,
    MemoryResourceHandle,
    OvercommitPolicy,
    ResourceAttrs,
    ResourceBackingKind,
    ResourceContract,
    ResourceOpSet,
    ResourceResidencySupport,
    ResourceState,
    ResourceSupport,
    SharingPolicy,
    StateValue,
};

extern crate std;
use self::std::alloc::{
    Layout,
    alloc_zeroed,
};

pub(super) fn aligned_region(len: usize, align: usize) -> Region {
    let layout = Layout::from_size_align(len, align).expect("test layout should be valid");
    // SAFETY: the layout is valid and intentionally leaked for the test.
    let ptr = unsafe { alloc_zeroed(layout) };
    let base = NonNull::new(ptr).expect("test allocation should succeed");
    Region {
        base: Address::from(base),
        len,
    }
}

pub(super) fn shifted_region(len: usize, align: usize, offset: usize) -> Region {
    let layout = Layout::from_size_align(
        len.checked_add(offset).expect("shifted region should fit"),
        align,
    )
    .expect("shifted layout should be valid");
    // SAFETY: the layout is valid and intentionally leaked for the test.
    let ptr = unsafe { alloc_zeroed(layout) };
    let base = NonNull::new(unsafe { ptr.add(offset) }).expect("shifted allocation should exist");
    Region {
        base: Address::from(base),
        len,
    }
}

pub(super) const fn general_geometry() -> MemoryGeometry {
    MemoryGeometry {
        base_granule: NonZeroUsize::new(4096).expect("nonzero"),
        alloc_granule: NonZeroUsize::new(4096).expect("nonzero"),
        protect_granule: None,
        commit_granule: None,
        lock_granule: None,
        large_granule: None,
    }
}

pub(super) const fn byte_geometry() -> MemoryGeometry {
    MemoryGeometry {
        base_granule: NonZeroUsize::new(1).expect("nonzero"),
        alloc_granule: NonZeroUsize::new(1).expect("nonzero"),
        protect_granule: None,
        commit_granule: None,
        lock_granule: None,
        large_granule: None,
    }
}

pub(super) fn general_contract() -> ResourceContract {
    ResourceContract {
        allowed_protect: Protect::READ | Protect::WRITE,
        write_xor_execute: true,
        sharing: SharingPolicy::Private,
        overcommit: OvercommitPolicy::Disallow,
        cache_policy: CachePolicy::Default,
        integrity: None,
    }
}

pub(super) fn general_support() -> ResourceSupport {
    ResourceSupport {
        protect: Protect::READ | Protect::WRITE,
        ops: ResourceOpSet::QUERY,
        advice: fusion_pal::sys::mem::MemAdviceCaps::empty(),
        residency: ResourceResidencySupport::BEST_EFFORT,
    }
}

pub(super) fn bound_resource(
    len: usize,
    domain: MemoryDomain,
    backing: ResourceBackingKind,
    attrs: ResourceAttrs,
) -> MemoryResourceHandle {
    let total_len = Allocator::<4, 4>::resource_request_for_extent_request_with_layout_policy(
        MemoryPoolExtentRequest::new(len),
        AllocatorLayoutPolicy::exact_static(),
    )
    .expect("allocator-backed test resource request should build")
    .provisioning_len()
    .expect("allocator-backed test resource length should fit");
    bound_resource_with_region(
        aligned_region(total_len, 4096),
        general_geometry(),
        domain,
        backing,
        attrs,
    )
}

pub(super) fn bound_resource_with_region(
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
            AllocatorLayoutPolicy::exact_static(),
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
