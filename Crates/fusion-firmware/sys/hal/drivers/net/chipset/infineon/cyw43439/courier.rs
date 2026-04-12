//! Courier-backed CYW43439 Bluetooth service.
//!
//! The service itself lives inside a driver courier under the firmware courier. Callers only get
//! a cross-courier channel-backed control surface that can power the adapter and move canonical
//! Bluetooth HCI frames.

use core::cell::UnsafeCell;
use core::hint::spin_loop;
use core::mem::MaybeUninit;
use core::sync::atomic::{
    AtomicU32,
    AtomicU8,
    Ordering,
};
use core::time::Duration;

use fusion_hal::contract::drivers::net::bluetooth::{
    BluetoothAdapterDescriptor,
    BluetoothAdapterId,
    BluetoothAdapterSupport,
    BluetoothAdvertisingControlContract,
    BluetoothAdvertisingParameters,
    BluetoothAdvertisingSetId,
    BluetoothCanonicalFrame,
    BluetoothCanonicalFrameControlContract,
    BluetoothOwnedAdapterContract,
    BluetoothError,
    BluetoothErrorKind,
    BluetoothHciAclFrame,
    BluetoothHciCommandFrame,
    BluetoothHciCommandHeader,
    BluetoothHciEventFrame,
    BluetoothHciEventHeader,
    BluetoothHciFrame,
    BluetoothHciFrameView,
    BluetoothHciPacketType,
    BluetoothRadioControlContract,
    BluetoothTransportCaps,
    BluetoothRoleCaps,
    BluetoothLePhyCaps,
    BluetoothAdvertisingCaps,
    BluetoothScanningCaps,
    BluetoothConnectionCaps,
    BluetoothSecurityCaps,
    BluetoothL2capCaps,
    BluetoothAttCaps,
    BluetoothGattCaps,
    BluetoothIsoCaps,
    BluetoothVersion,
    BluetoothVersionRange,
};
use fusion_std::thread::{
    CurrentFiberAsyncSingleton,
    yield_now as fiber_yield_now,
};
use fusion_sys::channel::{
    ChannelError,
    ChannelErrorKind,
    ChannelReceiveContract,
    ChannelSendContract,
    LocalChannel,
    LocalChannelDebugState,
};
use fusion_sys::thread::{
    system_monotonic_time,
    system_thread,
};
use fusion_sys::transport::{
    TransportAttachmentControlContract,
    TransportAttachmentLaw,
    TransportAttachmentRequest,
    TransportError,
};
use fusion_sys::transport::protocol::{
    ProtocolBootstrapKind,
    ProtocolCaps,
    ProtocolContract,
    ProtocolDebugView,
    ProtocolDescriptor,
    ProtocolId,
    ProtocolImplementationKind,
    ProtocolTransportRequirements,
    ProtocolVersion,
};

use crate::module::StackDriverStorage;
use crate::sys::hal::runtime::{
    CYW43439_BLUETOOTH_DRIVER_CONTEXT_ID,
    CYW43439_BLUETOOTH_DRIVER_COURIER_ID,
    FIRMWARE_CONTEXT_ID,
    FIRMWARE_COURIER_ID,
    cyw43439_bluetooth_driver_launch_request,
    ensure_root_courier,
    firmware_child_launch_request,
    launch_control,
    runtime_sink,
    upsert_courier_metadata,
};

use super::activate_bluetooth_adapter;

const FIRMWARE_RUNTIME_UNINITIALIZED: u8 = 0;
const FIRMWARE_RUNTIME_RUNNING: u8 = 1;
const FIRMWARE_RUNTIME_READY: u8 = 2;

const FIRMWARE_SUPERVISOR_UNINITIALIZED: u8 = 0;
const FIRMWARE_SUPERVISOR_RUNNING: u8 = 1;
const FIRMWARE_SUPERVISOR_READY: u8 = 2;

const SERVICE_UNINITIALIZED: u8 = 0;
const SERVICE_STARTING: u8 = 1;
const SERVICE_READY: u8 = 2;
const SERVICE_FAILED: u8 = 3;

const FIRMWARE_COURIER_STACK_BYTES: usize = 8192;
const DRIVER_COURIER_STACK_BYTES: usize = 32768;
const BLUETOOTH_DRIVER_STORAGE_WORDS: usize = 256;
const BLUETOOTH_COMMAND_CAPACITY: usize = 16;
const BLUETOOTH_STATUS_CAPACITY: usize = 16;
const BLUETOOTH_FRAME_MAX_BYTES: usize = 272;
const BLUETOOTH_LE_ADVERTISING_DATA_BYTES: usize = 31;
const BLUETOOTH_SEND_SCRATCH_BYTES: usize = 320;

static FIRMWARE_RUNTIME_SLOT: RuntimeSingletonSlot = RuntimeSingletonSlot::new();
static DRIVER_RUNTIME_SLOT: RuntimeSingletonSlot = RuntimeSingletonSlot::new();
static FIRMWARE_SUPERVISOR_STATE: AtomicU8 = AtomicU8::new(FIRMWARE_SUPERVISOR_UNINITIALIZED);
static BLUETOOTH_SERVICE_STATE: AtomicU8 = AtomicU8::new(SERVICE_UNINITIALIZED);
static BLUETOOTH_SERVICE_ERROR_KIND: AtomicU8 = AtomicU8::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_PHASE: AtomicU8 = AtomicU8::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_LAST_REQUEST_ID: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_LAST_STATUS_REQUEST_ID: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_SERVICE_PHASE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_CLIENT_PHASE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_CLIENT_LAST_STATUS_REQUEST_ID: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_SERVICE_COMMAND_CHANNEL_ADDR: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_SERVICE_STATUS_CHANNEL_ADDR: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_SERVICE_STATUS_PENDING_AFTER_SEND: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_SERVICE_STATUS_HEAD_AFTER_SEND: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_SERVICE_STATUS_TAIL_AFTER_SEND: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_SERVICE_STATUS_HEAD_OCCUPIED_AFTER_SEND: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_SERVICE_STATUS_PENDING_IDLE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_SERVICE_STATUS_HEAD_IDLE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_SERVICE_STATUS_TAIL_IDLE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_SERVICE_STATUS_HEAD_OCCUPIED_IDLE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_CLIENT_COMMAND_CHANNEL_ADDR: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_CLIENT_STATUS_CHANNEL_ADDR: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_CLIENT_STATUS_PENDING_BEFORE_RECV: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_CLIENT_STATUS_HEAD_BEFORE_RECV: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_CLIENT_STATUS_TAIL_BEFORE_RECV: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_CLIENT_STATUS_HEAD_OCCUPIED_BEFORE_RECV: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_CLIENT_STATUS_PENDING_AFTER_NONE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_CLIENT_STATUS_HEAD_AFTER_NONE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_CLIENT_STATUS_TAIL_AFTER_NONE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_CLIENT_STATUS_HEAD_OCCUPIED_AFTER_NONE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_SERVICE_ADDR: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_SERVICE_STACK_DRIVER_STORAGE_ADDR: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_SERVICE_SCRATCH_ADDR: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static CYW43439_COURIER_SERVICE_READ_BUFFER_ADDR: AtomicU32 = AtomicU32::new(0);

struct RuntimeSingletonSlot {
    state: AtomicU8,
    value: UnsafeCell<MaybeUninit<CurrentFiberAsyncSingleton>>,
}

impl RuntimeSingletonSlot {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(FIRMWARE_RUNTIME_UNINITIALIZED),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    fn get_or_init(
        &self,
        build: impl FnOnce() -> CurrentFiberAsyncSingleton,
    ) -> &'static CurrentFiberAsyncSingleton {
        loop {
            match self.state.load(Ordering::Acquire) {
                FIRMWARE_RUNTIME_READY => {
                    return unsafe { &*(*self.value.get()).as_ptr() };
                }
                FIRMWARE_RUNTIME_UNINITIALIZED => {
                    if self
                        .state
                        .compare_exchange(
                            FIRMWARE_RUNTIME_UNINITIALIZED,
                            FIRMWARE_RUNTIME_RUNNING,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_err()
                    {
                        continue;
                    }
                    let runtime = build();
                    unsafe { (*self.value.get()).write(runtime) };
                    self.state.store(FIRMWARE_RUNTIME_READY, Ordering::Release);
                    return unsafe { &*(*self.value.get()).as_ptr() };
                }
                FIRMWARE_RUNTIME_RUNNING => spin_loop(),
                _ => spin_loop(),
            }
        }
    }

    fn request_if_ready(&self) {
        if self.state.load(Ordering::Acquire) != FIRMWARE_RUNTIME_READY {
            return;
        }
        // SAFETY: once the slot reaches READY the singleton is written exactly once and remains
        // pinned in place for the life of the process.
        let runtime = unsafe { &*(*self.value.get()).as_ptr() };
        runtime.request_autonomous_dispatch();
    }
}

unsafe impl Sync for RuntimeSingletonSlot {}

struct BluetoothCourierCommandProtocol;
struct BluetoothCourierStatusProtocol;

#[derive(Debug, Clone, Copy)]
enum BluetoothCourierCommandKind {
    SetPowered {
        powered: bool,
    },
    IsPowered,
    StartAdvertising {
        parameters: BluetoothAdvertisingParameters,
        data_len: u8,
        data: [u8; BLUETOOTH_LE_ADVERTISING_DATA_BYTES],
        has_scan_response: bool,
        scan_response_len: u8,
        scan_response: [u8; BLUETOOTH_LE_ADVERTISING_DATA_BYTES],
    },
    StopAdvertising {
        advertising_set: BluetoothAdvertisingSetId,
    },
    WaitFrame {
        timeout_ms: u32,
        has_timeout: bool,
    },
    SendHci {
        packet_type: BluetoothHciPacketType,
        len: u16,
        bytes: [u8; BLUETOOTH_FRAME_MAX_BYTES],
    },
    ReceiveHci,
}

#[derive(Debug, Clone, Copy)]
struct BluetoothCourierCommand {
    request_id: u32,
    kind: BluetoothCourierCommandKind,
}

#[derive(Debug, Clone, Copy)]
enum BluetoothCourierStatus {
    Ack {
        request_id: u32,
    },
    AdvertisingStarted {
        request_id: u32,
        advertising_set: BluetoothAdvertisingSetId,
    },
    Powered {
        request_id: u32,
        powered: bool,
    },
    FrameAvailable {
        request_id: u32,
        available: bool,
    },
    ReceivedHci {
        request_id: u32,
        packet_type: BluetoothHciPacketType,
        len: u16,
        bytes: [u8; BLUETOOTH_FRAME_MAX_BYTES],
    },
    NoFrame {
        request_id: u32,
    },
    Failed {
        request_id: u32,
        kind: BluetoothErrorKind,
    },
}

impl ProtocolContract for BluetoothCourierCommandProtocol {
    type Message = BluetoothCourierCommand;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x7f0d_1c0f_4b7b_4e7d_8e11_0000_0000_1201),
        version: ProtocolVersion::new(1, 0, 0),
        caps: ProtocolCaps::empty(),
        bootstrap: ProtocolBootstrapKind::Immediate,
        debug_view: ProtocolDebugView::Structured,
        transport: ProtocolTransportRequirements::message_local(),
        implementation: ProtocolImplementationKind::Native,
    };
}

impl ProtocolContract for BluetoothCourierStatusProtocol {
    type Message = BluetoothCourierStatus;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x7f0d_1c0f_4b7b_4e7d_8e11_0000_0000_1202),
        version: ProtocolVersion::new(1, 0, 0),
        caps: ProtocolCaps::empty(),
        bootstrap: ProtocolBootstrapKind::Immediate,
        debug_view: ProtocolDebugView::Structured,
        transport: ProtocolTransportRequirements::message_local(),
        implementation: ProtocolImplementationKind::Native,
    };
}

struct BluetoothCourierClientIo {
    commands: LocalChannel<BluetoothCourierCommandProtocol, BLUETOOTH_COMMAND_CAPACITY, 1>,
    statuses: LocalChannel<BluetoothCourierStatusProtocol, BLUETOOTH_STATUS_CAPACITY, 1>,
    command_producer: usize,
    status_consumer: usize,
    next_request_id: UnsafeCell<u32>,
}

unsafe impl Sync for BluetoothCourierClientIo {}

impl BluetoothCourierClientIo {
    fn new() -> Result<(Self, usize, usize), BluetoothError> {
        let request = TransportAttachmentRequest::cross_courier()
            .with_requested_law(TransportAttachmentLaw::ExclusiveSpsc);
        let commands = LocalChannel::<BluetoothCourierCommandProtocol, BLUETOOTH_COMMAND_CAPACITY, 1>::new_with_attachment_law(
            TransportAttachmentLaw::ExclusiveSpsc,
        )
        .map_err(bluetooth_error_from_channel)?;
        let statuses = LocalChannel::<BluetoothCourierStatusProtocol, BLUETOOTH_STATUS_CAPACITY, 1>::new_with_attachment_law(
            TransportAttachmentLaw::ExclusiveSpsc,
        )
        .map_err(bluetooth_error_from_channel)?;
        let command_producer = commands
            .attach_producer(request)
            .map_err(bluetooth_error_from_transport)?;
        let command_consumer = commands
            .attach_consumer(request)
            .map_err(bluetooth_error_from_transport)?;
        let status_producer = statuses
            .attach_producer(request)
            .map_err(bluetooth_error_from_transport)?;
        let status_consumer = statuses
            .attach_consumer(request)
            .map_err(bluetooth_error_from_transport)?;

        Ok((
            Self {
                commands,
                statuses,
                command_producer,
                status_consumer,
                next_request_id: UnsafeCell::new(0),
            },
            command_consumer,
            status_producer,
        ))
    }

    fn next_request_id(&self) -> u32 {
        unsafe {
            let next = (*self.next_request_id.get()).wrapping_add(1).max(1);
            *self.next_request_id.get() = next;
            next
        }
    }
}

struct BluetoothCourierService {
    client: BluetoothCourierClientIo,
    command_consumer: usize,
    status_producer: usize,
}

#[derive(Clone, Copy)]
pub struct SystemBluetoothCourier {
    client: &'static BluetoothCourierClientIo,
}

struct BluetoothServiceSlot {
    state: AtomicU8,
    value: UnsafeCell<MaybeUninit<BluetoothCourierService>>,
}

impl BluetoothServiceSlot {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(SERVICE_UNINITIALIZED),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    fn get_or_init(&self) -> Result<&'static BluetoothCourierService, BluetoothError> {
        loop {
            match self.state.load(Ordering::Acquire) {
                SERVICE_READY => {
                    CYW43439_COURIER_PHASE.store(7, Ordering::Release);
                    return Ok(unsafe { &*(*self.value.get()).as_ptr() });
                }
                SERVICE_FAILED => return Err(service_error_from_state()),
                SERVICE_UNINITIALIZED => {
                    if self
                        .state
                        .compare_exchange(
                            SERVICE_UNINITIALIZED,
                            SERVICE_STARTING,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_err()
                    {
                        continue;
                    }
                    let initialized =
                        (|| -> Result<&'static BluetoothCourierService, BluetoothError> {
                            CYW43439_COURIER_PHASE.store(8, Ordering::Release);
                            let (service, command_consumer, status_producer) =
                                BluetoothCourierClientIo::new()?;
                            let service = BluetoothCourierService {
                                client: service,
                                command_consumer,
                                status_producer,
                            };
                            unsafe { (*self.value.get()).write(service) };
                            let service = unsafe { &*(*self.value.get()).as_ptr() };
                            CYW43439_COURIER_PHASE.store(9, Ordering::Release);
                            ensure_firmware_supervisor()?;
                            CYW43439_COURIER_PHASE.store(10, Ordering::Release);
                            ensure_driver_service_spawned(service)?;
                            CYW43439_COURIER_PHASE.store(11, Ordering::Release);
                            // Expose the channel surface as soon as the driver courier is admitted.
                            // The service itself may still be starting, but command/status exchange can
                            // wait on that honestly. Courier acquisition should not eagerly kick the
                            // child runtime just to look alive.
                            CYW43439_COURIER_PHASE.store(12, Ordering::Release);
                            Ok(service)
                        })();
                    match initialized {
                        Ok(service) => {
                            CYW43439_COURIER_PHASE.store(13, Ordering::Release);
                            self.state.store(SERVICE_READY, Ordering::Release);
                            CYW43439_COURIER_PHASE.store(14, Ordering::Release);
                            return Ok(service);
                        }
                        Err(error) => {
                            BLUETOOTH_SERVICE_ERROR_KIND
                                .store(encode_error_kind(error.kind()), Ordering::Release);
                            self.state.store(SERVICE_FAILED, Ordering::Release);
                            return Err(error);
                        }
                    }
                }
                SERVICE_STARTING => wait_for_firmware_progress(),
                _ => wait_for_firmware_progress(),
            }
        }
    }
}

unsafe impl Sync for BluetoothServiceSlot {}

static BLUETOOTH_SERVICE_SLOT: BluetoothServiceSlot = BluetoothServiceSlot::new();

pub fn system_bluetooth_courier() -> Result<SystemBluetoothCourier, BluetoothError> {
    let service = BLUETOOTH_SERVICE_SLOT.get_or_init()?;
    Ok(SystemBluetoothCourier {
        client: &service.client,
    })
}

fn firmware_runtime() -> &'static CurrentFiberAsyncSingleton {
    FIRMWARE_RUNTIME_SLOT.get_or_init(|| {
        ensure_root_courier().expect("root courier should register");
        CurrentFiberAsyncSingleton::new()
            .with_courier_plan(
                fusion_sys::courier::CourierPlan::new(4, 4)
                    .with_planned_fiber_capacity(4)
                    .with_dynamic_fiber_capacity(4)
                    .with_async_capacity(4)
                    .with_runnable_capacity(4)
                    .with_app_metadata_capacity(32)
                    .with_obligation_capacity(16)
                    .with_recent_dead_depth(8),
            )
            .with_initial_fiber_capacity(1)
            .with_fiber_capacity(4)
            .with_courier_id(FIRMWARE_COURIER_ID)
            .with_context_id(FIRMWARE_CONTEXT_ID)
            .with_runtime_sink(runtime_sink())
            .with_launch_control(launch_control())
            .with_child_launch(firmware_child_launch_request())
    })
}

fn driver_runtime() -> &'static CurrentFiberAsyncSingleton {
    DRIVER_RUNTIME_SLOT.get_or_init(|| {
        CurrentFiberAsyncSingleton::new()
            .with_courier_plan(
                fusion_sys::courier::CourierPlan::new(1, 4)
                    .with_planned_fiber_capacity(4)
                    .with_dynamic_fiber_capacity(4)
                    .with_async_capacity(4)
                    .with_runnable_capacity(4)
                    .with_app_metadata_capacity(32)
                    .with_obligation_capacity(16)
                    .with_recent_dead_depth(8),
            )
            .with_initial_fiber_capacity(1)
            .with_fiber_capacity(4)
            .with_courier_id(CYW43439_BLUETOOTH_DRIVER_COURIER_ID)
            .with_context_id(CYW43439_BLUETOOTH_DRIVER_CONTEXT_ID)
            .with_runtime_sink(runtime_sink())
            .with_launch_control(launch_control())
            .with_child_launch(cyw43439_bluetooth_driver_launch_request())
    })
}

fn ensure_firmware_supervisor() -> Result<(), BluetoothError> {
    loop {
        match FIRMWARE_SUPERVISOR_STATE.load(Ordering::Acquire) {
            FIRMWARE_SUPERVISOR_READY => return Ok(()),
            FIRMWARE_SUPERVISOR_UNINITIALIZED => {
                if FIRMWARE_SUPERVISOR_STATE
                    .compare_exchange(
                        FIRMWARE_SUPERVISOR_UNINITIALIZED,
                        FIRMWARE_SUPERVISOR_RUNNING,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_err()
                {
                    continue;
                }
                firmware_runtime()
                    .spawn_fiber_with_stack::<FIRMWARE_COURIER_STACK_BYTES, _, _>(|| {})
                    .map_err(bluetooth_error_from_fiber)?;
                CYW43439_COURIER_PHASE.store(1, Ordering::Release);
                if wait_for_courier_registration(FIRMWARE_COURIER_ID).is_ok() {
                    let _ = upsert_courier_metadata(FIRMWARE_COURIER_ID, "role", "firmware", 1);
                    CYW43439_COURIER_PHASE.store(2, Ordering::Release);
                } else {
                    CYW43439_COURIER_PHASE.store(3, Ordering::Release);
                }
                FIRMWARE_RUNTIME_SLOT.request_if_ready();
                FIRMWARE_SUPERVISOR_STATE.store(FIRMWARE_SUPERVISOR_READY, Ordering::Release);
                return Ok(());
            }
            FIRMWARE_SUPERVISOR_RUNNING => wait_for_firmware_progress(),
            _ => wait_for_firmware_progress(),
        }
    }
}

fn ensure_driver_service_spawned(
    service: &'static BluetoothCourierService,
) -> Result<(), BluetoothError> {
    driver_runtime()
        .spawn_fiber_with_stack::<DRIVER_COURIER_STACK_BYTES, _, _>(move || {
            run_bluetooth_service(service)
        })
        .map_err(bluetooth_error_from_fiber)?;
    CYW43439_COURIER_PHASE.store(4, Ordering::Release);
    if wait_for_courier_registration(CYW43439_BLUETOOTH_DRIVER_COURIER_ID).is_ok() {
        let _ = upsert_courier_metadata(
            CYW43439_BLUETOOTH_DRIVER_COURIER_ID,
            "role",
            "driver.bluetooth",
            2,
        );
        let _ = upsert_courier_metadata(
            CYW43439_BLUETOOTH_DRIVER_COURIER_ID,
            "driver.key",
            "net.bluetooth.infineon.cyw43439",
            2,
        );
        CYW43439_COURIER_PHASE.store(5, Ordering::Release);
    } else {
        CYW43439_COURIER_PHASE.store(6, Ordering::Release);
    }
    Ok(())
}

fn wait_for_courier_registration(
    courier: fusion_sys::domain::CourierId,
) -> Result<(), BluetoothError> {
    for _ in 0..16_384 {
        if crate::sys::hal::runtime::courier_pedigree::<4>(courier).is_ok() {
            return Ok(());
        }
        wait_for_firmware_progress();
    }
    Err(BluetoothError::busy())
}

fn run_bluetooth_service(service: &'static BluetoothCourierService) {
    CYW43439_COURIER_SERVICE_PHASE.store(1, Ordering::Release);
    if let Ok(snapshot) = service.client.commands.debug_state() {
        CYW43439_COURIER_SERVICE_COMMAND_CHANNEL_ADDR
            .store(snapshot.channel_addr as u32, Ordering::Release);
    }
    if let Ok(snapshot) = service.client.statuses.debug_state() {
        CYW43439_COURIER_SERVICE_STATUS_CHANNEL_ADDR
            .store(snapshot.channel_addr as u32, Ordering::Release);
    }
    let mut bluetooth_storage = StackDriverStorage::<BLUETOOTH_DRIVER_STORAGE_WORDS>::new();
    CYW43439_COURIER_SERVICE_ADDR.store(service as *const _ as u32, Ordering::Release);
    CYW43439_COURIER_SERVICE_STACK_DRIVER_STORAGE_ADDR
        .store((&bluetooth_storage as *const _) as u32, Ordering::Release);
    let mut bluetooth = match activate_bluetooth_adapter(bluetooth_storage.slot()) {
        Ok(bluetooth) => bluetooth,
        Err(error) => {
            BLUETOOTH_SERVICE_ERROR_KIND.store(encode_error_kind(error.kind()), Ordering::Release);
            BLUETOOTH_SERVICE_STATE.store(SERVICE_FAILED, Ordering::Release);
            CYW43439_COURIER_SERVICE_PHASE.store(0xE1, Ordering::Release);
            return;
        }
    };
    BLUETOOTH_SERVICE_ERROR_KIND.store(0, Ordering::Release);
    BLUETOOTH_SERVICE_STATE.store(SERVICE_READY, Ordering::Release);
    CYW43439_COURIER_SERVICE_PHASE.store(2, Ordering::Release);

    let mut scratch = [0_u8; BLUETOOTH_SEND_SCRATCH_BYTES];
    let mut read_buffer = [0_u8; BLUETOOTH_FRAME_MAX_BYTES];
    CYW43439_COURIER_SERVICE_SCRATCH_ADDR.store((&scratch as *const _) as u32, Ordering::Release);
    CYW43439_COURIER_SERVICE_READ_BUFFER_ADDR
        .store((&read_buffer as *const _) as u32, Ordering::Release);

    loop {
        CYW43439_COURIER_SERVICE_PHASE.store(3, Ordering::Release);
        while let Some(command) = match service
            .client
            .commands
            .try_receive(service.command_consumer)
        {
            Ok(command) => command,
            Err(error) if error.kind() == ChannelErrorKind::Busy => None,
            Err(_) => None,
        } {
            CYW43439_COURIER_LAST_REQUEST_ID.store(command.request_id, Ordering::Release);
            CYW43439_COURIER_SERVICE_PHASE.store(4, Ordering::Release);
            let status = match handle_bluetooth_command(
                bluetooth.adapter_mut(),
                command,
                &mut scratch,
                &mut read_buffer,
            ) {
                Ok(status) => status,
                Err(error) => BluetoothCourierStatus::Failed {
                    request_id: command.request_id,
                    kind: error.kind(),
                },
            };
            CYW43439_COURIER_SERVICE_PHASE.store(5, Ordering::Release);
            loop {
                match service
                    .client
                    .statuses
                    .try_send(service.status_producer, status)
                {
                    Ok(()) => {
                        record_service_status_after_send(service.client.statuses.debug_state());
                        if let Some(request_id) = status_request_id(status) {
                            CYW43439_COURIER_LAST_STATUS_REQUEST_ID
                                .store(request_id, Ordering::Release);
                        }
                        CYW43439_COURIER_SERVICE_PHASE.store(6, Ordering::Release);
                        break;
                    }
                    Err(error) if error.kind() == ChannelErrorKind::Busy => {
                        CYW43439_COURIER_SERVICE_PHASE.store(7, Ordering::Release);
                        if fiber_yield_now().is_err() {
                            let _ = system_monotonic_time().sleep_for(Duration::from_micros(50));
                        }
                    }
                    Err(_) => {
                        CYW43439_COURIER_SERVICE_PHASE.store(0xE2, Ordering::Release);
                        break;
                    }
                }
            }
        }
        CYW43439_COURIER_SERVICE_PHASE.store(8, Ordering::Release);
        record_service_status_idle(service.client.statuses.debug_state());
        let _ = fiber_yield_now();
    }
}

fn handle_bluetooth_command(
    bluetooth: &mut dyn fusion_hal::contract::drivers::net::bluetooth::BluetoothAdapterContract,
    command: BluetoothCourierCommand,
    scratch: &mut [u8],
    read_buffer: &mut [u8],
) -> Result<BluetoothCourierStatus, BluetoothError> {
    match command.kind {
        BluetoothCourierCommandKind::SetPowered { powered } => {
            bluetooth.set_powered(powered)?;
            Ok(BluetoothCourierStatus::Ack {
                request_id: command.request_id,
            })
        }
        BluetoothCourierCommandKind::IsPowered => Ok(BluetoothCourierStatus::Powered {
            request_id: command.request_id,
            powered: bluetooth.is_powered()?,
        }),
        BluetoothCourierCommandKind::StartAdvertising {
            parameters,
            data_len,
            data,
            has_scan_response,
            scan_response_len,
            scan_response,
        } => {
            let advertising_set = bluetooth.start_advertising(
                parameters,
                &data[..usize::from(data_len)],
                has_scan_response.then_some(&scan_response[..usize::from(scan_response_len)]),
            )?;
            Ok(BluetoothCourierStatus::AdvertisingStarted {
                request_id: command.request_id,
                advertising_set,
            })
        }
        BluetoothCourierCommandKind::StopAdvertising { advertising_set } => {
            bluetooth.stop_advertising(advertising_set)?;
            Ok(BluetoothCourierStatus::Ack {
                request_id: command.request_id,
            })
        }
        BluetoothCourierCommandKind::WaitFrame {
            timeout_ms,
            has_timeout,
        } => Ok(BluetoothCourierStatus::FrameAvailable {
            request_id: command.request_id,
            available: bluetooth.wait_frame(has_timeout.then_some(timeout_ms))?,
        }),
        BluetoothCourierCommandKind::SendHci {
            packet_type,
            len,
            bytes,
        } => {
            let len = usize::from(len);
            bluetooth.send_frame(
                BluetoothCanonicalFrame::Hci(BluetoothHciFrameView::Opaque(BluetoothHciFrame {
                    packet_type,
                    bytes: &bytes[..len],
                })),
                scratch,
            )?;
            Ok(BluetoothCourierStatus::Ack {
                request_id: command.request_id,
            })
        }
        BluetoothCourierCommandKind::ReceiveHci => {
            let Some(frame) = bluetooth.recv_frame(read_buffer)? else {
                return Ok(BluetoothCourierStatus::NoFrame {
                    request_id: command.request_id,
                });
            };
            let (packet_type, len, bytes) = flatten_hci_frame(frame)?;
            Ok(BluetoothCourierStatus::ReceivedHci {
                request_id: command.request_id,
                packet_type,
                len,
                bytes,
            })
        }
    }
}

impl BluetoothCanonicalFrameControlContract for SystemBluetoothCourier {
    fn wait_frame(&mut self, timeout_ms: Option<u32>) -> Result<bool, BluetoothError> {
        match self.perform(BluetoothCourierCommandKind::WaitFrame {
            timeout_ms: timeout_ms.unwrap_or(0),
            has_timeout: timeout_ms.is_some(),
        })? {
            BluetoothCourierStatus::FrameAvailable { available, .. } => Ok(available),
            BluetoothCourierStatus::Failed { kind, .. } => Err(bluetooth_error_from_kind(kind)),
            _ => Err(BluetoothError::state_conflict()),
        }
    }

    fn send_frame(
        &mut self,
        frame: BluetoothCanonicalFrame<'_>,
        _scratch: &mut [u8],
    ) -> Result<(), BluetoothError> {
        let (packet_type, len, bytes) = flatten_hci_frame(frame)?;
        match self.perform(BluetoothCourierCommandKind::SendHci {
            packet_type,
            len,
            bytes,
        })? {
            BluetoothCourierStatus::Ack { .. } => Ok(()),
            BluetoothCourierStatus::Failed { kind, .. } => Err(bluetooth_error_from_kind(kind)),
            _ => Err(BluetoothError::state_conflict()),
        }
    }

    fn recv_frame<'a>(
        &mut self,
        out: &'a mut [u8],
    ) -> Result<Option<BluetoothCanonicalFrame<'a>>, BluetoothError> {
        match self.perform(BluetoothCourierCommandKind::ReceiveHci)? {
            BluetoothCourierStatus::ReceivedHci {
                packet_type,
                len,
                bytes,
                ..
            } => {
                let len = usize::from(len);
                if out.len() < len {
                    return Err(BluetoothError::resource_exhausted());
                }
                out[..len].copy_from_slice(&bytes[..len]);
                Ok(Some(BluetoothCanonicalFrame::Hci(
                    BluetoothHciFrameView::Opaque(BluetoothHciFrame {
                        packet_type,
                        bytes: &out[..len],
                    }),
                )))
            }
            BluetoothCourierStatus::NoFrame { .. } => Ok(None),
            BluetoothCourierStatus::Failed { kind, .. } => Err(bluetooth_error_from_kind(kind)),
            _ => Err(BluetoothError::state_conflict()),
        }
    }
}

impl BluetoothOwnedAdapterContract for SystemBluetoothCourier {
    fn descriptor(&self) -> &'static BluetoothAdapterDescriptor {
        static DESCRIPTOR: BluetoothAdapterDescriptor = BluetoothAdapterDescriptor {
            id: BluetoothAdapterId(0),
            name: "firmware-bluetooth-courier",
            vendor_identity: None,
            shared_chipset: true,
            address: None,
            version: BluetoothVersionRange {
                minimum: BluetoothVersion::new(5, 2),
                maximum: BluetoothVersion::new(5, 2),
            },
            support: BluetoothAdapterSupport {
                transports: BluetoothTransportCaps::LE,
                roles: BluetoothRoleCaps::from_bits_retain(
                    BluetoothRoleCaps::PERIPHERAL.bits() | BluetoothRoleCaps::BROADCASTER.bits(),
                ),
                le_phys: BluetoothLePhyCaps::LE_1M,
                advertising: BluetoothAdvertisingCaps::from_bits_retain(
                    BluetoothAdvertisingCaps::LEGACY.bits()
                        | BluetoothAdvertisingCaps::CONNECTABLE.bits()
                        | BluetoothAdvertisingCaps::SCANNABLE.bits(),
                ),
                scanning: BluetoothScanningCaps::empty(),
                connection: BluetoothConnectionCaps::empty(),
                security: BluetoothSecurityCaps::empty(),
                l2cap: BluetoothL2capCaps::empty(),
                att: BluetoothAttCaps::empty(),
                gatt: BluetoothGattCaps::empty(),
                iso: BluetoothIsoCaps::empty(),
                max_connections: 0,
                max_advertising_sets: 1,
                max_periodic_advertising_sets: 0,
                max_att_mtu: 0,
                max_attribute_value_len: 0,
                max_l2cap_channels: 0,
                max_l2cap_sdu_len: 0,
            },
        };

        &DESCRIPTOR
    }
}

impl BluetoothRadioControlContract for SystemBluetoothCourier {
    fn set_powered(&mut self, powered: bool) -> Result<(), BluetoothError> {
        match self.perform(BluetoothCourierCommandKind::SetPowered { powered })? {
            BluetoothCourierStatus::Ack { .. } => Ok(()),
            BluetoothCourierStatus::Failed { kind, .. } => Err(bluetooth_error_from_kind(kind)),
            _ => Err(BluetoothError::state_conflict()),
        }
    }

    fn is_powered(&self) -> Result<bool, BluetoothError> {
        match self.perform(BluetoothCourierCommandKind::IsPowered)? {
            BluetoothCourierStatus::Powered { powered, .. } => Ok(powered),
            BluetoothCourierStatus::Failed { kind, .. } => Err(bluetooth_error_from_kind(kind)),
            _ => Err(BluetoothError::state_conflict()),
        }
    }
}

impl BluetoothAdvertisingControlContract for SystemBluetoothCourier {
    fn start_advertising(
        &mut self,
        parameters: BluetoothAdvertisingParameters,
        data: &[u8],
        scan_response: Option<&[u8]>,
    ) -> Result<BluetoothAdvertisingSetId, BluetoothError> {
        if data.len() > BLUETOOTH_LE_ADVERTISING_DATA_BYTES {
            return Err(BluetoothError::invalid());
        }
        if scan_response.is_some_and(|bytes| bytes.len() > BLUETOOTH_LE_ADVERTISING_DATA_BYTES) {
            return Err(BluetoothError::invalid());
        }

        let mut data_buffer = [0_u8; BLUETOOTH_LE_ADVERTISING_DATA_BYTES];
        data_buffer[..data.len()].copy_from_slice(data);
        let mut scan_response_buffer = [0_u8; BLUETOOTH_LE_ADVERTISING_DATA_BYTES];
        let (has_scan_response, scan_response_len) = if let Some(bytes) = scan_response {
            scan_response_buffer[..bytes.len()].copy_from_slice(bytes);
            (true, bytes.len() as u8)
        } else {
            (false, 0)
        };

        match self.perform(BluetoothCourierCommandKind::StartAdvertising {
            parameters,
            data_len: data.len() as u8,
            data: data_buffer,
            has_scan_response,
            scan_response_len,
            scan_response: scan_response_buffer,
        })? {
            BluetoothCourierStatus::AdvertisingStarted {
                advertising_set, ..
            } => Ok(advertising_set),
            BluetoothCourierStatus::Failed { kind, .. } => Err(bluetooth_error_from_kind(kind)),
            _ => Err(BluetoothError::state_conflict()),
        }
    }

    fn stop_advertising(
        &mut self,
        advertising_set: BluetoothAdvertisingSetId,
    ) -> Result<(), BluetoothError> {
        match self.perform(BluetoothCourierCommandKind::StopAdvertising { advertising_set })? {
            BluetoothCourierStatus::Ack { .. } => Ok(()),
            BluetoothCourierStatus::Failed { kind, .. } => Err(bluetooth_error_from_kind(kind)),
            _ => Err(BluetoothError::state_conflict()),
        }
    }
}

impl SystemBluetoothCourier {
    fn perform(
        &self,
        kind: BluetoothCourierCommandKind,
    ) -> Result<BluetoothCourierStatus, BluetoothError> {
        let request_id = self.client.next_request_id();
        CYW43439_COURIER_CLIENT_PHASE.store(1, Ordering::Release);
        if let Ok(snapshot) = self.client.commands.debug_state() {
            CYW43439_COURIER_CLIENT_COMMAND_CHANNEL_ADDR
                .store(snapshot.channel_addr as u32, Ordering::Release);
        }
        if let Ok(snapshot) = self.client.statuses.debug_state() {
            CYW43439_COURIER_CLIENT_STATUS_CHANNEL_ADDR
                .store(snapshot.channel_addr as u32, Ordering::Release);
        }
        let command = BluetoothCourierCommand { request_id, kind };

        loop {
            match self
                .client
                .commands
                .try_send(self.client.command_producer, command)
            {
                Ok(()) => {
                    CYW43439_COURIER_CLIENT_PHASE.store(2, Ordering::Release);
                    break;
                }
                Err(error) if error.kind() == ChannelErrorKind::Busy => {
                    CYW43439_COURIER_CLIENT_PHASE.store(3, Ordering::Release);
                    wait_for_firmware_progress()
                }
                Err(error) => return Err(bluetooth_error_from_channel(error)),
            }
        }

        loop {
            CYW43439_COURIER_CLIENT_PHASE.store(4, Ordering::Release);
            match BLUETOOTH_SERVICE_STATE.load(Ordering::Acquire) {
                SERVICE_FAILED => return Err(service_error_from_state()),
                SERVICE_READY | SERVICE_STARTING | SERVICE_UNINITIALIZED => {}
                _ => {}
            }
            record_client_status_before_receive(self.client.statuses.debug_state());
            match self
                .client
                .statuses
                .try_receive(self.client.status_consumer)
            {
                Ok(Some(status)) => {
                    if let Some(observed) = status_request_id(status) {
                        CYW43439_COURIER_CLIENT_LAST_STATUS_REQUEST_ID
                            .store(observed, Ordering::Release);
                        CYW43439_COURIER_CLIENT_PHASE.store(5, Ordering::Release);
                        if observed == request_id {
                            return Ok(status);
                        }
                        return Err(BluetoothError::state_conflict());
                    }
                    return Err(BluetoothError::state_conflict());
                }
                Ok(None) => {
                    record_client_status_after_none(self.client.statuses.debug_state());
                    CYW43439_COURIER_CLIENT_PHASE.store(6, Ordering::Release);
                    wait_for_firmware_progress()
                }
                Err(error) if error.kind() == ChannelErrorKind::Busy => {
                    CYW43439_COURIER_CLIENT_PHASE.store(7, Ordering::Release);
                    wait_for_firmware_progress()
                }
                Err(error) => return Err(bluetooth_error_from_channel(error)),
            }
        }
    }
}

fn flatten_hci_frame(
    frame: BluetoothCanonicalFrame<'_>,
) -> Result<(BluetoothHciPacketType, u16, [u8; BLUETOOTH_FRAME_MAX_BYTES]), BluetoothError> {
    let mut out = [0_u8; BLUETOOTH_FRAME_MAX_BYTES];
    let (packet_type, len) = match frame {
        BluetoothCanonicalFrame::Hci(frame) => flatten_hci_frame_view(frame, &mut out)?,
        _ => return Err(BluetoothError::unsupported()),
    };
    Ok((packet_type, len as u16, out))
}

fn record_service_status_after_send(snapshot: Result<LocalChannelDebugState, ChannelError>) {
    if let Ok(snapshot) = snapshot {
        CYW43439_COURIER_SERVICE_STATUS_CHANNEL_ADDR
            .store(snapshot.channel_addr as u32, Ordering::Release);
        CYW43439_COURIER_SERVICE_STATUS_PENDING_AFTER_SEND
            .store(snapshot.pending_len as u32, Ordering::Release);
        CYW43439_COURIER_SERVICE_STATUS_HEAD_AFTER_SEND
            .store(snapshot.head as u32, Ordering::Release);
        CYW43439_COURIER_SERVICE_STATUS_TAIL_AFTER_SEND
            .store(snapshot.tail as u32, Ordering::Release);
        CYW43439_COURIER_SERVICE_STATUS_HEAD_OCCUPIED_AFTER_SEND
            .store(u32::from(snapshot.head_occupied), Ordering::Release);
    }
}

fn record_client_status_before_receive(snapshot: Result<LocalChannelDebugState, ChannelError>) {
    if let Ok(snapshot) = snapshot {
        CYW43439_COURIER_CLIENT_STATUS_CHANNEL_ADDR
            .store(snapshot.channel_addr as u32, Ordering::Release);
        CYW43439_COURIER_CLIENT_STATUS_PENDING_BEFORE_RECV
            .store(snapshot.pending_len as u32, Ordering::Release);
        CYW43439_COURIER_CLIENT_STATUS_HEAD_BEFORE_RECV
            .store(snapshot.head as u32, Ordering::Release);
        CYW43439_COURIER_CLIENT_STATUS_TAIL_BEFORE_RECV
            .store(snapshot.tail as u32, Ordering::Release);
        CYW43439_COURIER_CLIENT_STATUS_HEAD_OCCUPIED_BEFORE_RECV
            .store(u32::from(snapshot.head_occupied), Ordering::Release);
    }
}

fn record_client_status_after_none(snapshot: Result<LocalChannelDebugState, ChannelError>) {
    if let Ok(snapshot) = snapshot {
        CYW43439_COURIER_CLIENT_STATUS_PENDING_AFTER_NONE
            .store(snapshot.pending_len as u32, Ordering::Release);
        CYW43439_COURIER_CLIENT_STATUS_HEAD_AFTER_NONE
            .store(snapshot.head as u32, Ordering::Release);
        CYW43439_COURIER_CLIENT_STATUS_TAIL_AFTER_NONE
            .store(snapshot.tail as u32, Ordering::Release);
        CYW43439_COURIER_CLIENT_STATUS_HEAD_OCCUPIED_AFTER_NONE
            .store(u32::from(snapshot.head_occupied), Ordering::Release);
    }
}

fn record_service_status_idle(snapshot: Result<LocalChannelDebugState, ChannelError>) {
    if let Ok(snapshot) = snapshot {
        CYW43439_COURIER_SERVICE_STATUS_PENDING_IDLE
            .store(snapshot.pending_len as u32, Ordering::Release);
        CYW43439_COURIER_SERVICE_STATUS_HEAD_IDLE.store(snapshot.head as u32, Ordering::Release);
        CYW43439_COURIER_SERVICE_STATUS_TAIL_IDLE.store(snapshot.tail as u32, Ordering::Release);
        CYW43439_COURIER_SERVICE_STATUS_HEAD_OCCUPIED_IDLE
            .store(u32::from(snapshot.head_occupied), Ordering::Release);
    }
}

fn flatten_hci_frame_view(
    frame: BluetoothHciFrameView<'_>,
    out: &mut [u8; BLUETOOTH_FRAME_MAX_BYTES],
) -> Result<(BluetoothHciPacketType, usize), BluetoothError> {
    match frame {
        BluetoothHciFrameView::Command(BluetoothHciCommandFrame { header, parameters }) => {
            let len = BluetoothHciCommandHeader::ENCODED_LEN + parameters.len();
            if len > out.len() {
                return Err(BluetoothError::resource_exhausted());
            }
            out[..BluetoothHciCommandHeader::ENCODED_LEN].copy_from_slice(&header.encode());
            out[BluetoothHciCommandHeader::ENCODED_LEN..len].copy_from_slice(parameters);
            Ok((BluetoothHciPacketType::Command, len))
        }
        BluetoothHciFrameView::Event(BluetoothHciEventFrame { header, parameters }) => {
            let len = BluetoothHciEventHeader::ENCODED_LEN + parameters.len();
            if len > out.len() {
                return Err(BluetoothError::resource_exhausted());
            }
            out[..BluetoothHciEventHeader::ENCODED_LEN].copy_from_slice(&header.encode());
            out[BluetoothHciEventHeader::ENCODED_LEN..len].copy_from_slice(parameters);
            Ok((BluetoothHciPacketType::Event, len))
        }
        BluetoothHciFrameView::Acl(BluetoothHciAclFrame { header, payload }) => {
            let encoded = header.encode();
            let len = encoded.len() + payload.len();
            if len > out.len() {
                return Err(BluetoothError::resource_exhausted());
            }
            out[..encoded.len()].copy_from_slice(&encoded);
            out[encoded.len()..len].copy_from_slice(payload);
            Ok((BluetoothHciPacketType::AclData, len))
        }
        BluetoothHciFrameView::Sco(bytes) => {
            if bytes.len() > out.len() {
                return Err(BluetoothError::resource_exhausted());
            }
            out[..bytes.len()].copy_from_slice(bytes);
            Ok((BluetoothHciPacketType::ScoData, bytes.len()))
        }
        BluetoothHciFrameView::Iso(bytes) => {
            if bytes.len() > out.len() {
                return Err(BluetoothError::resource_exhausted());
            }
            out[..bytes.len()].copy_from_slice(bytes);
            Ok((BluetoothHciPacketType::IsoData, bytes.len()))
        }
        BluetoothHciFrameView::Opaque(BluetoothHciFrame { packet_type, bytes }) => {
            if bytes.len() > out.len() {
                return Err(BluetoothError::resource_exhausted());
            }
            out[..bytes.len()].copy_from_slice(bytes);
            Ok((packet_type, bytes.len()))
        }
    }
}

fn status_request_id(status: BluetoothCourierStatus) -> Option<u32> {
    Some(match status {
        BluetoothCourierStatus::Ack { request_id }
        | BluetoothCourierStatus::AdvertisingStarted { request_id, .. }
        | BluetoothCourierStatus::Powered { request_id, .. }
        | BluetoothCourierStatus::FrameAvailable { request_id, .. }
        | BluetoothCourierStatus::ReceivedHci { request_id, .. }
        | BluetoothCourierStatus::NoFrame { request_id }
        | BluetoothCourierStatus::Failed { request_id, .. } => request_id,
    })
}

fn wait_for_firmware_progress() {
    FIRMWARE_RUNTIME_SLOT.request_if_ready();
    DRIVER_RUNTIME_SLOT.request_if_ready();
    if system_thread().yield_now().is_ok() {
        return;
    }
    let _ = system_monotonic_time().sleep_for(Duration::from_micros(50));
}

fn encode_error_kind(kind: BluetoothErrorKind) -> u8 {
    match kind {
        BluetoothErrorKind::Unsupported => 1,
        BluetoothErrorKind::Invalid => 2,
        BluetoothErrorKind::Busy => 3,
        BluetoothErrorKind::ResourceExhausted => 4,
        BluetoothErrorKind::StateConflict => 5,
        BluetoothErrorKind::Disconnected => 6,
        BluetoothErrorKind::TimedOut => 7,
        BluetoothErrorKind::PermissionDenied => 8,
        BluetoothErrorKind::Platform(_) => u8::MAX,
    }
}

fn service_error_from_state() -> BluetoothError {
    match BLUETOOTH_SERVICE_ERROR_KIND.load(Ordering::Acquire) {
        1 => BluetoothError::unsupported(),
        2 => BluetoothError::invalid(),
        3 => BluetoothError::busy(),
        4 => BluetoothError::resource_exhausted(),
        5 => BluetoothError::state_conflict(),
        6 => BluetoothError::disconnected(),
        7 => BluetoothError::timed_out(),
        8 => BluetoothError::permission_denied(),
        _ => BluetoothError::state_conflict(),
    }
}

const fn bluetooth_error_from_kind(kind: BluetoothErrorKind) -> BluetoothError {
    match kind {
        BluetoothErrorKind::Unsupported => BluetoothError::unsupported(),
        BluetoothErrorKind::Invalid => BluetoothError::invalid(),
        BluetoothErrorKind::Busy => BluetoothError::busy(),
        BluetoothErrorKind::ResourceExhausted => BluetoothError::resource_exhausted(),
        BluetoothErrorKind::StateConflict => BluetoothError::state_conflict(),
        BluetoothErrorKind::Disconnected => BluetoothError::disconnected(),
        BluetoothErrorKind::TimedOut => BluetoothError::timed_out(),
        BluetoothErrorKind::PermissionDenied => BluetoothError::permission_denied(),
        BluetoothErrorKind::Platform(code) => BluetoothError::platform(code),
    }
}

fn bluetooth_error_from_channel(error: ChannelError) -> BluetoothError {
    match error.kind() {
        ChannelErrorKind::Unsupported => BluetoothError::unsupported(),
        ChannelErrorKind::Invalid => BluetoothError::invalid(),
        ChannelErrorKind::Busy => BluetoothError::busy(),
        ChannelErrorKind::PermissionDenied => BluetoothError::permission_denied(),
        ChannelErrorKind::ResourceExhausted => BluetoothError::resource_exhausted(),
        ChannelErrorKind::StateConflict => BluetoothError::state_conflict(),
        ChannelErrorKind::ProtocolMismatch | ChannelErrorKind::TransportDenied => {
            BluetoothError::state_conflict()
        }
        ChannelErrorKind::Platform(code) => BluetoothError::platform(code),
    }
}

fn bluetooth_error_from_transport(error: TransportError) -> BluetoothError {
    match error.kind() {
        fusion_pal::contract::pal::interconnect::transport::TransportErrorKind::Unsupported => {
            BluetoothError::unsupported()
        }
        fusion_pal::contract::pal::interconnect::transport::TransportErrorKind::Invalid => {
            BluetoothError::invalid()
        }
        fusion_pal::contract::pal::interconnect::transport::TransportErrorKind::Busy => {
            BluetoothError::busy()
        }
        fusion_pal::contract::pal::interconnect::transport::TransportErrorKind::ResourceExhausted => {
            BluetoothError::resource_exhausted()
        }
        fusion_pal::contract::pal::interconnect::transport::TransportErrorKind::StateConflict => {
            BluetoothError::state_conflict()
        }
        fusion_pal::contract::pal::interconnect::transport::TransportErrorKind::PermissionDenied => {
            BluetoothError::permission_denied()
        }
        fusion_pal::contract::pal::interconnect::transport::TransportErrorKind::NotAttached => {
            BluetoothError::state_conflict()
        }
        fusion_pal::contract::pal::interconnect::transport::TransportErrorKind::Platform(code) => {
            BluetoothError::platform(code)
        }
    }
}

fn bluetooth_error_from_fiber(error: fusion_sys::fiber::FiberError) -> BluetoothError {
    match error.kind() {
        fusion_sys::fiber::FiberErrorKind::Unsupported => BluetoothError::unsupported(),
        fusion_sys::fiber::FiberErrorKind::Invalid => BluetoothError::invalid(),
        fusion_sys::fiber::FiberErrorKind::ResourceExhausted => {
            BluetoothError::resource_exhausted()
        }
        fusion_sys::fiber::FiberErrorKind::DeadlineExceeded => BluetoothError::timed_out(),
        fusion_sys::fiber::FiberErrorKind::StateConflict => BluetoothError::state_conflict(),
        fusion_sys::fiber::FiberErrorKind::Context(_) => BluetoothError::state_conflict(),
    }
}
