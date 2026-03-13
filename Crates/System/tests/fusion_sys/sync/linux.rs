use fusion_sys::sync::{
    MutexCaps, Once, OnceCaps, RwLock, RwLockCaps, RwLockFairnessSupport, SyncImplementationKind,
    ThinMutex,
};

#[test]
fn linux_thin_mutex_prefers_native_backend() {
    let support = ThinMutex::new().support();

    assert_eq!(support.implementation, SyncImplementationKind::Native);
    assert!(support.caps.contains(MutexCaps::BLOCKING));
    assert!(support.caps.contains(MutexCaps::TRY_LOCK));
}

#[test]
fn linux_once_reports_waiting_resettable_surface() {
    let support = Once::new().support();

    assert_eq!(support.implementation, SyncImplementationKind::Emulated);
    assert!(support.caps.contains(OnceCaps::WAITING));
    assert!(support.caps.contains(OnceCaps::RESET_ON_FAILURE));
    assert!(support.caps.contains(OnceCaps::STATIC_INIT));
}

#[test]
fn linux_rwlock_reports_writer_preferred_surface() {
    let support = RwLock::new(0_u32).support();

    assert_eq!(support.implementation, SyncImplementationKind::Emulated);
    assert_eq!(support.fairness, RwLockFairnessSupport::WriterPreferred);
    assert!(support.caps.contains(RwLockCaps::BLOCKING_READ));
    assert!(support.caps.contains(RwLockCaps::BLOCKING_WRITE));
    assert!(support.caps.contains(RwLockCaps::TRY_READ));
    assert!(support.caps.contains(RwLockCaps::TRY_WRITE));
}
