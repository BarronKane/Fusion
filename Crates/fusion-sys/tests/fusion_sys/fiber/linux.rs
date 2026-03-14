use fusion_sys::fiber::{
    ContextCaps, ContextImplementationKind, ContextMigrationSupport, ContextStackDirection,
    ContextTlsIsolation, FiberSystem,
};

#[test]
fn linux_fiber_support_reports_emulated_same_carrier_contexts() {
    let support = FiberSystem::new().support();

    assert_eq!(
        support.context.implementation,
        ContextImplementationKind::Emulated
    );
    assert!(support.context.caps.contains(ContextCaps::MAKE));
    assert!(support.context.caps.contains(ContextCaps::SWAP));
    assert_eq!(support.context.stack_direction, ContextStackDirection::Down);
    assert_eq!(
        support.context.tls_isolation,
        ContextTlsIsolation::SharedCarrierThread
    );
    assert_eq!(
        support.context.migration,
        ContextMigrationSupport::SameCarrierOnly
    );
    assert!(!support.context.unwind_across_boundary);
}
