//! Firmware-owned courier tree and runtime publication surfaces.
//!
//! `fusion-firmware` owns the execution lineage for the firmware lane itself and the drivers it
//! activates. The tree here is the proving bench for:
//! - one root/main courier
//! - one firmware child courier
//! - one driver child courier beneath firmware
//!
//! The examples should not have to hand-roll this lineage themselves like desperate little
//! bureaucrats.

use core::sync::atomic::{
    AtomicU8,
    AtomicU32,
    Ordering,
};
use core::cell::UnsafeCell;
use core::mem::MaybeUninit;

use fusion_std::thread::CurrentFiberAsyncSingleton;
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
    CourierFiberClass,
    CourierFiberRecord,
    CourierLaunchControl,
    CourierLaunchControlError,
    CourierLaunchControlVTable,
    CourierLaunchDescriptor,
    CourierMetadataSubject,
    CourierObligationId,
    CourierObligationSpec,
    CourierPedigree,
    CourierResponsiveness,
    CourierRuntimeLedger,
    CourierRuntimeSink,
    CourierRuntimeSinkError,
    CourierRuntimeSinkVTable,
    CourierRuntimeSummary,
    CourierVisibility,
    FiberMetadataAttachment,
    FiberTerminalStatus,
};
use fusion_sys::domain::context::{
    ContextCaps,
    ContextId,
    ContextKind,
};
use fusion_sys::domain::{
    ContextDescriptor,
    CourierDescriptor,
    DomainCaps,
    DomainDescriptor,
    DomainError,
    DomainErrorKind,
    DomainId,
    DomainKind,
    DomainRegistry,
};
use fusion_sys::fiber::{
    FiberId,
    ManagedFiberSnapshot,
};
use fusion_sys::thread::{
    CarrierObservation,
    system_carrier,
};
use fusion_sys::sync::Mutex;

const FIRMWARE_DOMAIN_ID: DomainId = DomainId::new(0x4657_4352);
pub const MAIN_COURIER_ID: fusion_sys::domain::CourierId =
    fusion_sys::domain::CourierId::new(0x1000);
pub const FIRMWARE_COURIER_ID: fusion_sys::domain::CourierId =
    fusion_sys::domain::CourierId::new(0x1100);
pub const CYW43439_BLUETOOTH_DRIVER_COURIER_ID: fusion_sys::domain::CourierId =
    fusion_sys::domain::CourierId::new(0x1200);

pub const MAIN_CONTEXT_ID: ContextId = ContextId::new(0x1000);
pub const FIRMWARE_CONTEXT_ID: ContextId = ContextId::new(0x1100);
pub const CYW43439_BLUETOOTH_DRIVER_CONTEXT_ID: ContextId = ContextId::new(0x1200);

const ROOT_LAUNCH_EPOCH: u64 = 1;
const DRIVER_LAUNCH_EPOCH: u64 = 2;
const ROOT_MAIN_FIBER_STACK_BYTES: usize = 16 * 1024;

type FirmwareDomainRegistry = DomainRegistry<'static, 3, 3, 2, 4, 8, 32>;

#[unsafe(no_mangle)]
pub static FIRMWARE_COURIER_TREE_INIT_PHASE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static FIRMWARE_ROOT_BOOTSTRAP_PHASE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static FIRMWARE_RUN_ROOT_FIBER_PHASE: AtomicU32 = AtomicU32::new(0);

struct FirmwareCourierTree {
    registry: FirmwareDomainRegistry,
}

impl FirmwareCourierTree {
    fn new() -> Result<Self, DomainError> {
        FIRMWARE_COURIER_TREE_INIT_PHASE.store(1, Ordering::Release);
        let mut registry = FirmwareDomainRegistry::new(DomainDescriptor {
            id: FIRMWARE_DOMAIN_ID,
            name: "fusion-firmware",
            kind: DomainKind::NativeSubstrate,
            caps: DomainCaps::COURIER_REGISTRY
                | DomainCaps::COURIER_VISIBILITY
                | DomainCaps::CONTEXT_REGISTRY,
        });
        FIRMWARE_COURIER_TREE_INIT_PHASE.store(2, Ordering::Release);
        registry.register_courier(root_courier_descriptor())?;
        FIRMWARE_COURIER_TREE_INIT_PHASE.store(3, Ordering::Release);
        ensure_registered_context(&mut registry, MAIN_COURIER_ID, root_context_descriptor())?;
        FIRMWARE_COURIER_TREE_INIT_PHASE.store(4, Ordering::Release);
        registry.upsert_courier_metadata(MAIN_COURIER_ID, "role", "main", 0)?;
        FIRMWARE_COURIER_TREE_INIT_PHASE.store(5, Ordering::Release);
        Ok(Self { registry })
    }
}

static FIRMWARE_COURIER_TREE: Mutex<Option<FirmwareCourierTree>> = Mutex::new(None);
const ROOT_RUNTIME_UNINITIALIZED: u8 = 0;
const ROOT_RUNTIME_RUNNING: u8 = 1;
const ROOT_RUNTIME_READY: u8 = 2;

struct RootRuntimeSlot {
    state: AtomicU8,
    value: UnsafeCell<MaybeUninit<CurrentFiberAsyncSingleton>>,
}

impl RootRuntimeSlot {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(ROOT_RUNTIME_UNINITIALIZED),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    fn get_or_init(
        &self,
        build: impl FnOnce() -> CurrentFiberAsyncSingleton,
    ) -> &'static CurrentFiberAsyncSingleton {
        loop {
            match self.state.load(Ordering::Acquire) {
                ROOT_RUNTIME_READY => {
                    return unsafe { &*(*self.value.get()).as_ptr() };
                }
                ROOT_RUNTIME_UNINITIALIZED => {
                    if self
                        .state
                        .compare_exchange(
                            ROOT_RUNTIME_UNINITIALIZED,
                            ROOT_RUNTIME_RUNNING,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_err()
                    {
                        continue;
                    }
                    let runtime = build();
                    unsafe { (*self.value.get()).write(runtime) };
                    self.state.store(ROOT_RUNTIME_READY, Ordering::Release);
                    return unsafe { &*(*self.value.get()).as_ptr() };
                }
                ROOT_RUNTIME_RUNNING => core::hint::spin_loop(),
                _ => core::hint::spin_loop(),
            }
        }
    }
}

unsafe impl Sync for RootRuntimeSlot {}

static ROOT_RUNTIME_SLOT: RootRuntimeSlot = RootRuntimeSlot::new();

fn with_tree_mut<R>(
    f: impl FnOnce(&mut FirmwareCourierTree) -> Result<R, DomainError>,
) -> Result<R, DomainError> {
    FIRMWARE_COURIER_TREE_INIT_PHASE.store(10, Ordering::Release);
    let mut guard = FIRMWARE_COURIER_TREE
        .lock()
        .map_err(|_| DomainError::busy())?;
    FIRMWARE_COURIER_TREE_INIT_PHASE.store(11, Ordering::Release);
    if guard.is_none() {
        FIRMWARE_COURIER_TREE_INIT_PHASE.store(12, Ordering::Release);
        *guard = Some(FirmwareCourierTree::new()?);
        FIRMWARE_COURIER_TREE_INIT_PHASE.store(13, Ordering::Release);
    }
    FIRMWARE_COURIER_TREE_INIT_PHASE.store(14, Ordering::Release);
    f(guard
        .as_mut()
        .expect("firmware courier tree should be initialized"))
}

fn with_registry_mut<R>(
    f: impl FnOnce(&mut FirmwareDomainRegistry) -> Result<R, DomainError>,
) -> Result<R, DomainError> {
    with_tree_mut(|tree| f(&mut tree.registry))
}

fn with_registry<R>(
    f: impl FnOnce(&FirmwareDomainRegistry) -> Result<R, DomainError>,
) -> Result<R, DomainError> {
    let mut guard = FIRMWARE_COURIER_TREE
        .lock()
        .map_err(|_| DomainError::busy())?;
    if guard.is_none() {
        *guard = Some(FirmwareCourierTree::new()?);
    }
    f(&guard
        .as_ref()
        .expect("firmware courier tree should be initialized")
        .registry)
}

const fn root_courier_plan() -> fusion_sys::courier::CourierPlan {
    fusion_sys::courier::CourierPlan::new(4, 8)
        .with_planned_fiber_capacity(8)
        .with_dynamic_fiber_capacity(8)
        .with_async_capacity(8)
        .with_runnable_capacity(8)
        .with_app_metadata_capacity(32)
        .with_obligation_capacity(16)
        .with_recent_dead_depth(8)
}

const fn firmware_courier_plan() -> fusion_sys::courier::CourierPlan {
    fusion_sys::courier::CourierPlan::new(4, 4)
        .with_planned_fiber_capacity(4)
        .with_dynamic_fiber_capacity(4)
        .with_async_capacity(4)
        .with_runnable_capacity(4)
        .with_app_metadata_capacity(32)
        .with_obligation_capacity(16)
        .with_recent_dead_depth(8)
}

const fn driver_courier_plan() -> fusion_sys::courier::CourierPlan {
    fusion_sys::courier::CourierPlan::new(1, 4)
        .with_planned_fiber_capacity(4)
        .with_dynamic_fiber_capacity(4)
        .with_async_capacity(4)
        .with_runnable_capacity(4)
        .with_app_metadata_capacity(32)
        .with_obligation_capacity(16)
        .with_recent_dead_depth(8)
}

fn root_courier_descriptor() -> CourierDescriptor<'static> {
    CourierDescriptor {
        id: MAIN_COURIER_ID,
        name: "main",
        caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS
            | CourierCaps::SPAWN_SUB_FIBERS
            | CourierCaps::DEBUG_CHANNEL,
        visibility: CourierVisibility::Full,
        claim_awareness: ClaimAwareness::Blind,
        claim_context: None,
        plan: root_courier_plan(),
    }
}

const fn root_context_descriptor() -> ContextDescriptor<'static> {
    ContextDescriptor {
        id: MAIN_CONTEXT_ID,
        name: "root-main",
        kind: ContextKind::Custom,
        caps: ContextCaps::CONTROL_ENDPOINT,
        claim_context: None,
    }
}

const fn firmware_context_descriptor() -> ContextDescriptor<'static> {
    ContextDescriptor {
        id: FIRMWARE_CONTEXT_ID,
        name: "firmware",
        kind: ContextKind::ServiceEndpoint,
        caps: ContextCaps::CONTROL_ENDPOINT,
        claim_context: None,
    }
}

const fn cyw43439_bluetooth_driver_context_descriptor() -> ContextDescriptor<'static> {
    ContextDescriptor {
        id: CYW43439_BLUETOOTH_DRIVER_CONTEXT_ID,
        name: "cyw43439-bluetooth",
        kind: ContextKind::DeviceEndpoint,
        caps: ContextCaps::CONTROL_ENDPOINT,
        claim_context: None,
    }
}

fn ensure_registered_context(
    registry: &mut FirmwareDomainRegistry,
    owner: fusion_sys::domain::CourierId,
    descriptor: ContextDescriptor<'static>,
) -> Result<(), DomainError> {
    match registry.register_context(owner, descriptor) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == DomainErrorKind::StateConflict => Ok(()),
        Err(error) => Err(error),
    }
}

fn ensure_known_runtime_context_registered(
    registry: &mut FirmwareDomainRegistry,
    courier: fusion_sys::domain::CourierId,
    context: ContextId,
) -> Result<(), DomainError> {
    match (courier, context) {
        (MAIN_COURIER_ID, MAIN_CONTEXT_ID) => {
            ensure_registered_context(registry, MAIN_COURIER_ID, root_context_descriptor())
        }
        (FIRMWARE_COURIER_ID, FIRMWARE_CONTEXT_ID) => ensure_registered_context(
            registry,
            FIRMWARE_COURIER_ID,
            firmware_context_descriptor(),
        ),
        (CYW43439_BLUETOOTH_DRIVER_COURIER_ID, CYW43439_BLUETOOTH_DRIVER_CONTEXT_ID) => {
            ensure_registered_context(
                registry,
                CYW43439_BLUETOOTH_DRIVER_COURIER_ID,
                cyw43439_bluetooth_driver_context_descriptor(),
            )
        }
        _ => Err(DomainError::not_found()),
    }
}

fn firmware_launch_descriptor() -> CourierLaunchDescriptor<'static> {
    CourierLaunchDescriptor {
        id: FIRMWARE_COURIER_ID,
        name: "fusion-firmware",
        caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS
            | CourierCaps::SPAWN_SUB_FIBERS
            | CourierCaps::DEBUG_CHANNEL,
        visibility: CourierVisibility::Scoped,
        claim_awareness: ClaimAwareness::Blind,
        claim_context: None,
        plan: firmware_courier_plan(),
    }
}

fn cyw43439_bluetooth_driver_launch_descriptor() -> CourierLaunchDescriptor<'static> {
    CourierLaunchDescriptor {
        id: CYW43439_BLUETOOTH_DRIVER_COURIER_ID,
        name: "net.bluetooth.infineon.cyw43439",
        caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS
            | CourierCaps::SPAWN_SUB_FIBERS
            | CourierCaps::DEBUG_CHANNEL,
        visibility: CourierVisibility::Scoped,
        claim_awareness: ClaimAwareness::Blind,
        claim_context: None,
        plan: driver_courier_plan(),
    }
}

const fn local_runtime_seal(id: u64) -> LocalAdmissionSeal {
    LocalAdmissionSeal::new(
        ImageSealId::new(id),
        ClaimsDigest::zero(),
        ClaimsDigest::zero(),
        ClaimsDigest::zero(),
        id,
    )
}

pub fn ensure_root_courier() -> Result<(), DomainError> {
    with_registry_mut(|_| Ok(()))
}

fn build_root_runtime() -> CurrentFiberAsyncSingleton {
    CurrentFiberAsyncSingleton::new()
        .with_courier_plan(root_courier_plan())
        .with_courier_id(MAIN_COURIER_ID)
        .with_context_id(MAIN_CONTEXT_ID)
        .with_runtime_sink(runtime_sink())
        .with_launch_control(launch_control())
}

/// Root-courier bootstrap policy.
///
/// This is intentionally small today. The important part is reserving one honest policy slot at
/// the true entry boundary before claims/security requirements arrive and force us to retrofit
/// doctrine into a shape that was never designed to carry it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RootCourierPolicy {
    pub security: RootCourierSecurityPolicy,
}

impl RootCourierPolicy {
    #[must_use]
    pub const fn disabled() -> Self {
        Self {
            security: RootCourierSecurityPolicy::Disabled,
        }
    }

    #[must_use]
    pub const fn claims_required() -> Self {
        Self {
            security: RootCourierSecurityPolicy::claims_required(),
        }
    }

    #[must_use]
    pub const fn with_security(mut self, security: RootCourierSecurityPolicy) -> Self {
        self.security = security;
        self
    }
}

/// Root-courier security/claims posture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RootCourierSecurityPolicy {
    /// Claims/security enforcement is intentionally disabled for this root boot.
    #[default]
    Disabled,
    /// Reserve the full claims/keyring/signature gate for a future cut.
    ///
    /// When this becomes real, the root courier should require a signed root keyring and child
    /// couriers should be admitted only when their claims/signatures chain back to that root.
    ClaimsRequired {
        root_keyring: RootCourierKeyringRequirement,
        descendants: RootCourierDescendantRequirement,
    },
}

impl RootCourierSecurityPolicy {
    #[must_use]
    pub const fn disabled() -> Self {
        Self::Disabled
    }

    #[must_use]
    pub const fn claims_required() -> Self {
        Self::ClaimsRequired {
            root_keyring: RootCourierKeyringRequirement::RequireSignedRootKeyring,
            descendants: RootCourierDescendantRequirement::RequireSignedChain,
        }
    }
}

/// Future requirement for one root-courier trust anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RootCourierKeyringRequirement {
    /// No keyring is required yet.
    #[default]
    Disabled,
    /// A signed root keyring must be present before entry is admitted.
    RequireSignedRootKeyring,
}

/// Future requirement imposed on descendant couriers admitted under the root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RootCourierDescendantRequirement {
    /// No descendant signature/claims chain is enforced yet.
    #[default]
    Disabled,
    /// Descendants must be signed/admitted against the root courier trust anchor.
    RequireSignedChain,
}

/// Bootstrap record returned when firmware adopts the initial execution lane.
///
/// This is the first honest handoff from the bare-metal entry path into Fusion execution. The
/// adopted boot lane is not a magical main thread exception; it is the first carrier, and the
/// root courier/context are bound here before ordinary user code runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirmwareBootstrapContext {
    pub root_courier_id: fusion_sys::domain::CourierId,
    pub root_context_id: ContextId,
    pub adopted_carrier: Option<CarrierObservation>,
    pub root_policy: RootCourierPolicy,
}

/// Adopts the current bare-metal boot lane as the first published Fusion execution context.
///
/// This is intentionally a narrow first cut. It makes the root courier/context explicit and
/// observable at the firmware boundary without pretending the entire ambient thread substrate has
/// already been rewritten into carrier law.
pub fn bootstrap_root_execution() -> Result<FirmwareBootstrapContext, DomainError> {
    bootstrap_root_execution_with_policy(RootCourierPolicy::disabled())
}

/// Adopts the current bare-metal boot lane using one explicit root-courier policy.
///
/// Today only the disabled security posture is active. The richer claims/keyring modes are
/// intentionally carried here now so the entry boundary grows one honest policy seam before the
/// security doctrine lands for real.
pub fn bootstrap_root_execution_with_policy(
    policy: RootCourierPolicy,
) -> Result<FirmwareBootstrapContext, DomainError> {
    FIRMWARE_ROOT_BOOTSTRAP_PHASE.store(1, Ordering::Release);
    if !matches!(policy.security, RootCourierSecurityPolicy::Disabled) {
        FIRMWARE_ROOT_BOOTSTRAP_PHASE.store(0xff, Ordering::Release);
        return Err(DomainError::unsupported());
    }
    FIRMWARE_ROOT_BOOTSTRAP_PHASE.store(2, Ordering::Release);
    ensure_root_courier()?;
    FIRMWARE_ROOT_BOOTSTRAP_PHASE.store(3, Ordering::Release);
    with_registry_mut(|registry| {
        FIRMWARE_ROOT_BOOTSTRAP_PHASE.store(4, Ordering::Release);
        ensure_known_runtime_context_registered(registry, MAIN_COURIER_ID, MAIN_CONTEXT_ID)?;
        FIRMWARE_ROOT_BOOTSTRAP_PHASE.store(5, Ordering::Release);
        registry.record_runtime_context(MAIN_COURIER_ID, MAIN_CONTEXT_ID, ROOT_LAUNCH_EPOCH)?;
        FIRMWARE_ROOT_BOOTSTRAP_PHASE.store(6, Ordering::Release);
        registry.upsert_context_metadata(
            MAIN_CONTEXT_ID,
            "role",
            "root-main",
            ROOT_LAUNCH_EPOCH,
        )?;
        FIRMWARE_ROOT_BOOTSTRAP_PHASE.store(7, Ordering::Release);
        registry.upsert_courier_metadata(
            MAIN_COURIER_ID,
            "root-security",
            root_security_policy_label(policy.security),
            ROOT_LAUNCH_EPOCH,
        )?;
        FIRMWARE_ROOT_BOOTSTRAP_PHASE.store(8, Ordering::Release);
        Ok(())
    })?;
    FIRMWARE_ROOT_BOOTSTRAP_PHASE.store(9, Ordering::Release);
    Ok(FirmwareBootstrapContext {
        root_courier_id: MAIN_COURIER_ID,
        root_context_id: MAIN_CONTEXT_ID,
        adopted_carrier: system_carrier().observe_current().ok(),
        root_policy: policy,
    })
}

const fn root_security_policy_label(policy: RootCourierSecurityPolicy) -> &'static str {
    match policy {
        RootCourierSecurityPolicy::Disabled => "disabled",
        RootCourierSecurityPolicy::ClaimsRequired {
            root_keyring: RootCourierKeyringRequirement::Disabled,
            descendants: RootCourierDescendantRequirement::Disabled,
        } => "claims-required",
        RootCourierSecurityPolicy::ClaimsRequired {
            root_keyring: RootCourierKeyringRequirement::RequireSignedRootKeyring,
            descendants: RootCourierDescendantRequirement::Disabled,
        } => "claims-root-keyring",
        RootCourierSecurityPolicy::ClaimsRequired {
            root_keyring: RootCourierKeyringRequirement::Disabled,
            descendants: RootCourierDescendantRequirement::RequireSignedChain,
        } => "claims-descendants",
        RootCourierSecurityPolicy::ClaimsRequired {
            root_keyring: RootCourierKeyringRequirement::RequireSignedRootKeyring,
            descendants: RootCourierDescendantRequirement::RequireSignedChain,
        } => "claims-full",
    }
}

pub fn runtime_sink() -> CourierRuntimeSink {
    CourierRuntimeSink::new(core::ptr::null_mut(), &FIRMWARE_RUNTIME_SINK_VTABLE)
}

pub fn launch_control() -> CourierLaunchControl<'static> {
    CourierLaunchControl::new(core::ptr::null_mut(), FIRMWARE_LAUNCH_CONTROL_VTABLE)
}

/// Returns the firmware-owned managed runtime bound to the root courier.
#[must_use]
pub fn root_runtime() -> &'static CurrentFiberAsyncSingleton {
    ROOT_RUNTIME_SLOT.get_or_init(build_root_runtime)
}

/// Runs one managed root fiber inside the root courier on the adopted initial carrier.
///
/// This is the honest `main thread` realization for bare metal: ordinary user entry is lowered
/// into one root fiber instead of living forever on the ambient board stack.
///
/// # Errors
///
/// Returns an error when the root runtime cannot admit or complete the managed root fiber.
pub fn run_root_fiber<F, T>(job: F) -> Result<T, fusion_sys::fiber::FiberError>
where
    F: FnOnce() -> T + Send + 'static,
    T: 'static,
{
    FIRMWARE_RUN_ROOT_FIBER_PHASE.store(1, Ordering::Release);
    let handle = root_runtime().spawn_fiber_with_stack::<ROOT_MAIN_FIBER_STACK_BYTES, _, _>(job)?;
    FIRMWARE_RUN_ROOT_FIBER_PHASE.store(2, Ordering::Release);
    handle.join()
}

pub fn courier_pedigree<const MAX_DEPTH: usize>(
    courier: fusion_sys::domain::CourierId,
) -> Result<CourierPedigree<'static, MAX_DEPTH>, DomainError> {
    with_registry(|registry| registry.courier_pedigree(courier))
}

pub(crate) fn upsert_courier_metadata(
    courier: fusion_sys::domain::CourierId,
    key: &'static str,
    value: &'static str,
    tick: u64,
) -> Result<(), DomainError> {
    with_registry_mut(|registry| registry.upsert_courier_metadata(courier, key, value, tick))
}

pub(crate) fn firmware_child_launch_request() -> CourierChildLaunchRequest<'static> {
    CourierChildLaunchRequest {
        parent: MAIN_COURIER_ID,
        descriptor: firmware_launch_descriptor(),
        principal: PrincipalId::parse("firmware@svc[fusion.local]")
            .expect("firmware principal should parse"),
        image_seal: local_runtime_seal(1),
        launch_epoch: ROOT_LAUNCH_EPOCH,
    }
}

pub(crate) fn cyw43439_bluetooth_driver_launch_request() -> CourierChildLaunchRequest<'static> {
    CourierChildLaunchRequest {
        parent: FIRMWARE_COURIER_ID,
        descriptor: cyw43439_bluetooth_driver_launch_descriptor(),
        principal: PrincipalId::parse("cyw43439-bt@driver[pico2w.local]")
            .expect("bluetooth driver principal should parse"),
        image_seal: local_runtime_seal(2),
        launch_epoch: DRIVER_LAUNCH_EPOCH,
    }
}

static FIRMWARE_RUNTIME_SINK_VTABLE: CourierRuntimeSinkVTable = CourierRuntimeSinkVTable {
    record_context: firmware_runtime_record_context,
    register_fiber: firmware_runtime_register_fiber,
    update_fiber: firmware_runtime_update_fiber,
    mark_fiber_terminal: firmware_runtime_mark_fiber_terminal,
    record_runtime_summary: firmware_runtime_record_runtime_summary,
    runtime_ledger: firmware_runtime_ledger,
    fiber_record: firmware_fiber_record,
    evaluate_responsiveness: firmware_evaluate_responsiveness,
    upsert_metadata: firmware_upsert_metadata,
    remove_metadata: firmware_remove_metadata,
    register_obligation: firmware_register_obligation,
    record_obligation_progress: firmware_record_obligation_progress,
    remove_obligation: firmware_remove_obligation,
};

const FIRMWARE_LAUNCH_CONTROL_VTABLE: CourierLaunchControlVTable<'static> =
    CourierLaunchControlVTable {
        register_child_courier: firmware_register_child_courier,
    };

unsafe fn firmware_runtime_record_context(
    _context: *mut (),
    courier: fusion_sys::domain::CourierId,
    runtime_context: ContextId,
    tick: u64,
) -> Result<(), CourierRuntimeSinkError> {
    with_registry_mut(|registry| {
        ensure_known_runtime_context_registered(registry, courier, runtime_context)?;
        registry.record_runtime_context(courier, runtime_context, tick)
    })
    .map_err(Into::into)
}

unsafe fn firmware_runtime_register_fiber(
    _context: *mut (),
    courier: fusion_sys::domain::CourierId,
    snapshot: ManagedFiberSnapshot,
    generation: u64,
    class: CourierFiberClass,
    is_root: bool,
    metadata_attachment: Option<FiberMetadataAttachment>,
    tick: u64,
) -> Result<(), CourierRuntimeSinkError> {
    with_registry_mut(|registry| {
        registry.register_fiber_with_class(
            courier,
            snapshot,
            generation,
            class,
            is_root,
            metadata_attachment,
            tick,
        )
    })
    .map_err(Into::into)
}

unsafe fn firmware_runtime_update_fiber(
    _context: *mut (),
    courier: fusion_sys::domain::CourierId,
    snapshot: ManagedFiberSnapshot,
    tick: u64,
) -> Result<(), CourierRuntimeSinkError> {
    with_registry_mut(|registry| registry.update_fiber_snapshot(courier, snapshot, tick))
        .map_err(Into::into)
}

unsafe fn firmware_runtime_mark_fiber_terminal(
    _context: *mut (),
    courier: fusion_sys::domain::CourierId,
    fiber: FiberId,
    terminal: FiberTerminalStatus,
    tick: u64,
) -> Result<(), CourierRuntimeSinkError> {
    with_registry_mut(|registry| registry.mark_fiber_terminal(courier, fiber, terminal, tick))
        .map_err(Into::into)
}

unsafe fn firmware_runtime_record_runtime_summary(
    _context: *mut (),
    courier: fusion_sys::domain::CourierId,
    summary: CourierRuntimeSummary,
    tick: u64,
) -> Result<(), CourierRuntimeSinkError> {
    with_registry_mut(|registry| registry.record_runtime_summary(courier, summary, tick))
        .map_err(Into::into)
}

unsafe fn firmware_runtime_ledger(
    _context: *mut (),
    courier: fusion_sys::domain::CourierId,
) -> Result<CourierRuntimeLedger, CourierRuntimeSinkError> {
    with_registry(|registry| registry.runtime_ledger(courier)).map_err(Into::into)
}

unsafe fn firmware_fiber_record(
    _context: *mut (),
    courier: fusion_sys::domain::CourierId,
    fiber: FiberId,
) -> Result<Option<CourierFiberRecord>, CourierRuntimeSinkError> {
    with_registry(|registry| registry.fiber_record(courier, fiber)).map_err(Into::into)
}

unsafe fn firmware_evaluate_responsiveness(
    _context: *mut (),
    courier: fusion_sys::domain::CourierId,
    tick: u64,
) -> Result<CourierResponsiveness, CourierRuntimeSinkError> {
    with_registry_mut(|registry| registry.courier_responsiveness(courier, tick)).map_err(Into::into)
}

unsafe fn firmware_upsert_metadata(
    _context: *mut (),
    courier: fusion_sys::domain::CourierId,
    subject: CourierMetadataSubject,
    key: &'static str,
    value: &'static str,
    tick: u64,
) -> Result<(), CourierRuntimeSinkError> {
    with_registry_mut(|registry| match subject {
        CourierMetadataSubject::Courier => {
            registry.upsert_courier_metadata(courier, key, value, tick)
        }
        CourierMetadataSubject::Fiber(fiber) => {
            registry.upsert_fiber_metadata(courier, fiber, key, value, tick)
        }
        CourierMetadataSubject::ChildCourier(child) => {
            registry.upsert_child_courier_metadata(courier, child, key, value, tick)
        }
        CourierMetadataSubject::Context(context) => {
            registry.upsert_context_metadata(context, key, value, tick)
        }
        CourierMetadataSubject::AsyncLane => {
            registry.upsert_async_metadata(courier, key, value, tick)
        }
    })
    .map_err(Into::into)
}

unsafe fn firmware_remove_metadata(
    _context: *mut (),
    courier: fusion_sys::domain::CourierId,
    subject: CourierMetadataSubject,
    key: &str,
) -> Result<(), CourierRuntimeSinkError> {
    with_registry_mut(|registry| match subject {
        CourierMetadataSubject::Courier
        | CourierMetadataSubject::Fiber(_)
        | CourierMetadataSubject::Context(_)
        | CourierMetadataSubject::AsyncLane => registry.remove_metadata(courier, subject, key),
        CourierMetadataSubject::ChildCourier(child) => {
            registry.remove_child_courier_metadata(courier, child, key)
        }
    })
    .map_err(Into::into)
}

unsafe fn firmware_register_obligation(
    _context: *mut (),
    courier: fusion_sys::domain::CourierId,
    spec: CourierObligationSpec<'static>,
    tick: u64,
) -> Result<CourierObligationId, CourierRuntimeSinkError> {
    with_registry_mut(|registry| registry.register_obligation_spec(courier, spec, tick))
        .map_err(Into::into)
}

unsafe fn firmware_record_obligation_progress(
    _context: *mut (),
    courier: fusion_sys::domain::CourierId,
    obligation: CourierObligationId,
    tick: u64,
) -> Result<(), CourierRuntimeSinkError> {
    with_registry_mut(|registry| registry.record_obligation_progress(courier, obligation, tick))
        .map_err(Into::into)
}

unsafe fn firmware_remove_obligation(
    _context: *mut (),
    courier: fusion_sys::domain::CourierId,
    obligation: CourierObligationId,
) -> Result<(), CourierRuntimeSinkError> {
    with_registry_mut(|registry| registry.remove_obligation(courier, obligation))
        .map_err(Into::into)
}

unsafe fn firmware_register_child_courier(
    _context: *mut (),
    request: CourierChildLaunchRequest<'static>,
    launched_at_tick: u64,
    root_fiber: FiberId,
) -> Result<(), CourierLaunchControlError> {
    with_registry_mut(|registry| {
        registry.register_child_courier(
            request.parent,
            CourierDescriptor {
                id: request.descriptor.id,
                name: request.descriptor.name,
                caps: request.descriptor.caps,
                visibility: request.descriptor.visibility,
                claim_awareness: request.descriptor.claim_awareness,
                claim_context: request.descriptor.claim_context,
                plan: request.descriptor.plan,
            },
            request.principal,
            request.image_seal,
            request.launch_epoch,
            launched_at_tick,
            root_fiber,
        )
    })
    .map_err(Into::into)
}
