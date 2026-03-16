use fusion_sys::fiber::{
    ContextCaps, ContextImplementationKind, ContextMigrationSupport, ContextStackDirection,
    ContextTlsIsolation, FiberSystem,
};

#[test]
fn linux_fiber_support_reports_expected_cross_carrier_contexts() {
    let support = FiberSystem::new().support();

    #[cfg(target_arch = "x86_64")]
    assert_eq!(
        support.context.implementation,
        ContextImplementationKind::Native
    );
    #[cfg(target_arch = "aarch64")]
    assert_eq!(
        support.context.implementation,
        ContextImplementationKind::Native
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
        ContextMigrationSupport::CrossCarrier
    );
    #[cfg(target_arch = "x86_64")]
    assert_eq!(support.context.red_zone_bytes, 128);
    #[cfg(target_arch = "aarch64")]
    assert_eq!(support.context.red_zone_bytes, 0);
    assert!(!support.context.unwind_across_boundary);
}
