use fusion_firmware::sys::hal::runtime::{
    CYW43439_BLUETOOTH_DRIVER_COURIER_ID,
    FIRMWARE_COURIER_ID,
    MAIN_COURIER_ID,
    bootstrap_root_execution,
    launch_control,
};
use fusion_sys::claims::{
    ClaimAwareness,
    ClaimsDigest,
    ImageSealId,
    LocalAdmissionSeal,
    PrincipalId,
};
use fusion_sys::courier::{
    CourierCaps,
    CourierChildLaunchRequest,
    CourierLaunchControlError,
    CourierLaunchDescriptor,
    CourierPlan,
    CourierScopeRole,
    CourierVisibility,
};
use fusion_sys::domain::DomainErrorKind;
use fusion_sys::fiber::FiberId;
use fusion_sys::{
    context,
    courier,
};

use crate::lock_fusion_firmware_tests;

const ROOT_LAUNCH_EPOCH: u64 = 1;
const DRIVER_LAUNCH_EPOCH: u64 = 2;

fn local_runtime_seal(id: u64) -> LocalAdmissionSeal {
    LocalAdmissionSeal::new(
        ImageSealId::new(id),
        ClaimsDigest::zero(),
        ClaimsDigest::zero(),
        ClaimsDigest::zero(),
        id,
    )
}

const fn firmware_courier_plan() -> CourierPlan {
    CourierPlan::new(4, 4)
        .with_planned_fiber_capacity(4)
        .with_dynamic_fiber_capacity(4)
        .with_async_capacity(4)
        .with_runnable_capacity(4)
        .with_app_metadata_capacity(32)
        .with_obligation_capacity(16)
        .with_recent_dead_depth(8)
}

const fn driver_courier_plan() -> CourierPlan {
    CourierPlan::new(1, 4)
        .with_planned_fiber_capacity(4)
        .with_dynamic_fiber_capacity(4)
        .with_async_capacity(4)
        .with_runnable_capacity(4)
        .with_app_metadata_capacity(32)
        .with_obligation_capacity(16)
        .with_recent_dead_depth(8)
}

fn firmware_child_launch_request() -> CourierChildLaunchRequest<'static> {
    CourierChildLaunchRequest {
        parent: MAIN_COURIER_ID,
        descriptor: CourierLaunchDescriptor {
            id: FIRMWARE_COURIER_ID,
            name: "firmware",
            scope_role: CourierScopeRole::ContextRoot,
            caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS
                | CourierCaps::SPAWN_SUB_FIBERS
                | CourierCaps::DEBUG_CHANNEL,
            visibility: CourierVisibility::Scoped,
            claim_awareness: ClaimAwareness::Blind,
            claim_context: None,
            plan: firmware_courier_plan(),
        },
        principal: PrincipalId::parse("firmware@svc[fusion.local]")
            .expect("firmware principal should parse"),
        image_seal: local_runtime_seal(1),
        launch_epoch: ROOT_LAUNCH_EPOCH,
    }
}

fn cyw43439_bluetooth_driver_launch_request() -> CourierChildLaunchRequest<'static> {
    CourierChildLaunchRequest {
        parent: FIRMWARE_COURIER_ID,
        descriptor: CourierLaunchDescriptor {
            id: CYW43439_BLUETOOTH_DRIVER_COURIER_ID,
            name: "cyw43439",
            scope_role: CourierScopeRole::Leaf,
            caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS
                | CourierCaps::SPAWN_SUB_FIBERS
                | CourierCaps::DEBUG_CHANNEL,
            visibility: CourierVisibility::Scoped,
            claim_awareness: ClaimAwareness::Blind,
            claim_context: None,
            plan: driver_courier_plan(),
        },
        principal: PrincipalId::parse("cyw43439-bt@driver[pico2w.local]")
            .expect("bluetooth driver principal should parse"),
        image_seal: local_runtime_seal(2),
        launch_epoch: DRIVER_LAUNCH_EPOCH,
    }
}

fn ensure_child_registered(
    request: CourierChildLaunchRequest<'static>,
    launch_epoch: u64,
    root_fiber: FiberId,
) {
    match launch_control().register_child_courier(request, launch_epoch, root_fiber) {
        Ok(()) | Err(CourierLaunchControlError::StateConflict) => {}
        Err(error) => panic!("child courier registration should succeed: {error:?}"),
    }
}

#[test]
fn local_syscall_surface_is_honest_outside_managed_execution() {
    let _guard = lock_fusion_firmware_tests();
    bootstrap_root_execution().expect("root bootstrap should succeed");
    assert!(matches!(
        context::local::id(),
        Err(error) if error.kind() == DomainErrorKind::Unsupported
    ));
    assert!(matches!(
        courier::local::id(),
        Err(error) if error.kind() == DomainErrorKind::Unsupported
    ));
}

#[test]
fn local_syscall_surface_resolves_known_firmware_couriers() {
    let _guard = lock_fusion_firmware_tests();
    bootstrap_root_execution().expect("root bootstrap should succeed");
    ensure_child_registered(
        firmware_child_launch_request(),
        ROOT_LAUNCH_EPOCH,
        FiberId::new(1),
    );
    ensure_child_registered(
        cyw43439_bluetooth_driver_launch_request(),
        DRIVER_LAUNCH_EPOCH,
        FiberId::new(2),
    );
    let domain = fusion_pal::sys::identity::system_domain_name();
    let root = format!("root-courier[{domain}]");
    let firmware = format!("firmware@root-courier[{domain}]");
    let cyw = format!("cyw43439@firmware.root-courier[{domain}]");
    assert_eq!(
        courier::local::resolve_qualified_name(&root)
            .expect("root courier should resolve through local surface"),
        MAIN_COURIER_ID
    );
    assert_eq!(
        courier::local::resolve_qualified_name(&firmware)
            .expect("firmware courier should resolve through local surface"),
        FIRMWARE_COURIER_ID
    );
    assert_eq!(
        courier::local::resolve_qualified_name(&cyw)
            .expect("cyw courier should resolve through local surface"),
        CYW43439_BLUETOOTH_DRIVER_COURIER_ID
    );
    let surface = format!("fusion://cyw43439@firmware.root-courier[{domain}]/channel/control");
    assert_eq!(
        courier::local::resolve_surface(&surface)
            .expect("surface authority should resolve through local surface"),
        CYW43439_BLUETOOTH_DRIVER_COURIER_ID
    );
}
