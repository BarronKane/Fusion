use fusion_sys::alloc::{AllocErrorKind, AllocPolicy, BoundedArena, HeapAllocator, Slab};

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
