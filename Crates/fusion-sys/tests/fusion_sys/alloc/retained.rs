use fusion_sys::alloc::Allocator;

#[test]
fn immortal_slab_retained_value_survives_wrapper_and_allocator_drop() {
    let allocator = Allocator::<2, 2>::system_default().expect("allocator should build");
    let default_domain = allocator
        .default_domain()
        .expect("default domain should exist");
    let slab = allocator
        .immortal_slab::<64, 4>(default_domain)
        .expect("immortal slab should reserve backing");

    let retained = slab
        .alloc_retained_value(u64::from(0xDEAD_BEEF_u32))
        .expect("immortal slab should allocate retained value");
    drop(slab);
    drop(allocator);

    assert_eq!(*retained, u64::from(0xDEAD_BEEF_u32));
}

#[test]
fn immortal_arena_retained_value_survives_wrapper_and_allocator_drop() {
    let allocator = Allocator::<2, 2>::system_default().expect("allocator should build");
    let default_domain = allocator
        .default_domain()
        .expect("default domain should exist");
    let arena = allocator
        .immortal_arena(default_domain, 256)
        .expect("immortal arena should reserve backing");

    let retained = arena
        .alloc_retained_value(0x1234_5678_u32)
        .expect("immortal arena should allocate retained value");
    drop(arena);
    drop(allocator);

    assert_eq!(*retained, 0x1234_5678_u32);
}
