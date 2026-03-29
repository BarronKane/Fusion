use fusion_sys::alloc::{AllocErrorKind, AllocationStrategy, Allocator};

extern crate std;
use self::std::sync::{Arc, Barrier};
use self::std::thread;

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
fn arena_direct_allocation_blocks_reset_until_drop() {
    let allocator = Allocator::<2, 2>::system_default().expect("allocator should build");
    let default_domain = allocator
        .default_domain()
        .expect("default domain should exist");
    let arena = allocator
        .arena(default_domain, 256)
        .expect("arena should reserve backing");

    let mut allocation = arena
        .alloc(&fusion_sys::alloc::AllocRequest {
            len: 48,
            align: 16,
            zeroed: true,
        })
        .expect("direct arena allocation should succeed");
    assert_eq!(allocation.len(), 48);
    assert_eq!(allocation.align(), 16);
    assert!(
        allocation
            .as_bytes()
            .expect("direct arena bytes should exist")
            .iter()
            .all(|byte| *byte == 0)
    );
    assert_eq!(
        arena
            .reset()
            .expect_err("reset must reject live direct arena allocations")
            .kind,
        AllocErrorKind::Busy
    );

    allocation
        .try_release()
        .expect("top-of-stack direct arena allocation should pop cleanly");
    arena
        .reset()
        .expect("reset should succeed once direct arena allocations are gone");
}

#[test]
fn arena_failed_early_release_preserves_allocation_token() {
    let allocator = Allocator::<2, 2>::system_default().expect("allocator should build");
    let default_domain = allocator
        .default_domain()
        .expect("default domain should exist");
    let arena = allocator
        .arena(default_domain, 256)
        .expect("arena should reserve backing");

    let mut first = arena
        .alloc(&fusion_sys::alloc::AllocRequest {
            len: 32,
            align: 16,
            zeroed: false,
        })
        .expect("first direct arena allocation should succeed");
    let second = arena
        .alloc(&fusion_sys::alloc::AllocRequest {
            len: 32,
            align: 16,
            zeroed: false,
        })
        .expect("second direct arena allocation should succeed");

    assert_eq!(
        first
            .try_release()
            .expect_err("non-top arena allocation should not pop early")
            .kind,
        AllocErrorKind::InvalidRequest
    );
    assert!(first.is_live());
    assert!(first.as_bytes().is_some());

    drop(first);
    assert_eq!(
        arena
            .reset()
            .expect_err("later direct allocation still keeps arena busy")
            .kind,
        AllocErrorKind::Busy
    );
    drop(second);
    arena
        .reset()
        .expect("reset should recover once all direct allocations are gone");
}

#[test]
fn arena_serializes_concurrent_direct_allocations_and_reset() {
    let allocator = Allocator::<2, 2>::system_default().expect("allocator should build");
    let default_domain = allocator
        .default_domain()
        .expect("default domain should exist");
    let arena = Arc::new(
        allocator
            .arena(default_domain, 1024)
            .expect("arena should reserve backing"),
    );
    let barrier = Arc::new(Barrier::new(8));
    let mut workers = std::vec::Vec::new();

    for worker_index in 0..8usize {
        let arena = Arc::clone(&arena);
        let barrier = Arc::clone(&barrier);
        workers.push(thread::spawn(move || {
            barrier.wait();
            let mut allocation = arena
                .alloc(&fusion_sys::alloc::AllocRequest {
                    len: 64,
                    align: 16,
                    zeroed: false,
                })
                .expect("concurrent arena allocation should succeed");
            allocation
                .as_bytes_mut()
                .expect("arena bytes should be mutable")[0] =
                u8::try_from(worker_index).expect("worker index should fit");
        }));
    }

    for worker in workers {
        worker.join().expect("worker should finish");
    }

    arena
        .reset()
        .expect("reset should recover once all concurrent direct allocations are gone");
    let allocation = arena
        .alloc(&fusion_sys::alloc::AllocRequest {
            len: 128,
            align: 16,
            zeroed: false,
        })
        .expect("reset arena should allocate again from the front");
    assert_eq!(
        allocation
            .ptr()
            .expect("reset allocation should still be live")
            .as_ptr() as usize
            % 16,
        0
    );
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
