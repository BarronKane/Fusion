use fusion_sys::alloc::{
    AllocErrorKind,
    AllocationStrategy,
    Allocator,
};

extern crate std;
use self::std::sync::{
    Arc,
    Barrier,
};
use self::std::thread;

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
fn slab_direct_allocation_returns_slot_on_drop() {
    let allocator = Allocator::<2, 2>::system_default().expect("allocator should build");
    let default_domain = allocator
        .default_domain()
        .expect("default domain should exist");
    let slab = allocator
        .slab::<64, 2>(default_domain)
        .expect("slab should reserve backing");

    {
        let mut allocation = slab
            .alloc(&fusion_sys::alloc::AllocRequest::zeroed(32))
            .expect("direct slab allocation should succeed");
        assert_eq!(allocation.len(), 64);
        assert_eq!(allocation.align(), 64);
        assert!(
            allocation
                .as_bytes()
                .expect("direct slab bytes should exist")
                .iter()
                .all(|byte| *byte == 0)
        );
        allocation
            .as_bytes_mut()
            .expect("direct slab bytes should be mutable")[0] = 0xAA;
    }

    let reused = slab
        .alloc(&fusion_sys::alloc::AllocRequest::new(64))
        .expect("dropped direct slab allocation should release its slot");
    assert!(reused.is_live());
}

#[test]
fn slab_serializes_concurrent_direct_allocations() {
    let allocator =
        Allocator::<2, 2>::system_default_with_capacity(512).expect("allocator should build");
    let default_domain = allocator
        .default_domain()
        .expect("default domain should exist");
    let slab = Arc::new(
        allocator
            .slab::<64, 8>(default_domain)
            .expect("slab should reserve backing"),
    );
    let barrier = Arc::new(Barrier::new(8));
    let mut workers = std::vec::Vec::new();

    for worker_index in 0..8usize {
        let slab = Arc::clone(&slab);
        let barrier = Arc::clone(&barrier);
        workers.push(thread::spawn(move || {
            barrier.wait();
            let mut allocation = slab
                .alloc(&fusion_sys::alloc::AllocRequest::new(64))
                .expect("concurrent slab allocation should succeed");
            allocation
                .as_bytes_mut()
                .expect("slab bytes should be mutable")[0] =
                u8::try_from(worker_index).expect("worker index should fit");
        }));
    }

    for worker in workers {
        worker.join().expect("worker should finish");
    }

    for _ in 0..8 {
        let allocation = slab
            .alloc(&fusion_sys::alloc::AllocRequest::new(64))
            .expect("all dropped concurrent slab allocations should have returned");
        drop(allocation);
    }
}
