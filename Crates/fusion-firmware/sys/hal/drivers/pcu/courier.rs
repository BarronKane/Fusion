//! Courier-backed RP2350 PIO PCU service.
//!
//! The service owns one driver courier and one service fiber that ingests narrow stream-model PCU
//! kernels and installs them onto the selected PIO substrate. This is intentionally not a generic
//! PCU daemon yet. The hardware only tells the truth for persistent stream installs, so the
//! firmware courier tells the same truth instead of hallucinating a universal execution service.

use core::array;
use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{
    AtomicU32,
    AtomicU8,
    Ordering,
};
use core::time::Duration;

use fusion_pal::sys::pcu::{
    PcuCommandSubmission,
    PcuDispatchContract,
    PcuDispatchSubmission,
    PcuError,
    PcuErrorKind,
    PcuFiniteHandle,
    PcuFiniteState,
    PcuInvocationBindings,
    PcuInvocationParameters,
    PcuKernelId,
    PcuPersistentHandle,
    PcuPersistentState,
    PcuPort,
    PcuSignalInstallation,
    PcuStreamCapabilities,
    PcuStreamInstallation,
    PcuStreamKernelIr,
    PcuStreamPattern,
    PcuStreamValueType,
    PcuTransactionSubmission,
    PlatformPcu,
    system_pcu,
};
use fusion_std::thread::{
    CurrentFiberAsyncSingleton,
    FUSION_GREEN_RESUME_ERROR_KIND,
    FUSION_GREEN_RESUME_PHASE,
    FUSION_GREEN_TASK_ENTRY_FAILURE_KIND,
    FUSION_GREEN_TASK_ENTRY_PHASE,
    is_in_green_context,
    yield_now as fiber_yield_now,
};
use fusion_sys::channel::{
    ChannelError,
    ChannelErrorKind,
    ChannelReceiveContract,
    ChannelSendContract,
    LocalChannel,
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
use fusion_sys::sync::{
    RawMutex,
    SpinMutex,
};

use crate::sys::hal::runtime::{
    ensure_root_courier,
    root_runtime,
};

const PIO_COURIER_DEBUG_MAGIC_START: u32 = 0x5049_4f30;
const PIO_COURIER_DEBUG_MAGIC_END: u32 = 0x5049_4f31;
const SERVICE_UNINITIALIZED: u8 = 0;
const SERVICE_STARTING: u8 = 1;
const SERVICE_READY: u8 = 2;
const SERVICE_FAILED: u8 = 3;

const DRIVER_COURIER_STACK_BYTES: usize = 32768;
const PIO_COMMAND_CAPACITY: usize = 8;
const PIO_STATUS_CAPACITY: usize = 8;
const PIO_STREAM_HANDLE_CAPACITY: usize = 4;

static PIO_SERVICE_SLOT: PioServiceSlot = PioServiceSlot::new();
static PIO_SERVICE_ERROR_KIND: AtomicU8 = AtomicU8::new(0);

#[repr(C)]
pub struct PioCourierDebugState {
    pub magic_start: u32,
    pub client_phase: AtomicU32,
    pub service_phase: AtomicU32,
    pub install_phase: AtomicU32,
    pub last_request_id: AtomicU32,
    pub last_status_request_id: AtomicU32,
    pub last_slot: AtomicU32,
    pub last_token: AtomicU32,
    pub error_kind: AtomicU32,
    pub driver_run_state: AtomicU32,
    pub driver_active_units: AtomicU32,
    pub driver_runnable_units: AtomicU32,
    pub driver_running_units: AtomicU32,
    pub driver_blocked_units: AtomicU32,
    pub driver_summary_error: AtomicU32,
    pub driver_runtime_realized: AtomicU32,
    pub last_spawn_phase: AtomicU32,
    pub green_task_entry_phase: AtomicU32,
    pub green_task_entry_failure_kind: AtomicU32,
    pub green_resume_phase: AtomicU32,
    pub green_resume_error_kind: AtomicU32,
    pub magic_end: u32,
}

#[unsafe(no_mangle)]
#[used]
pub static PIO_COURIER_DEBUG_STATE: PioCourierDebugState = PioCourierDebugState {
    magic_start: PIO_COURIER_DEBUG_MAGIC_START,
    client_phase: AtomicU32::new(0),
    service_phase: AtomicU32::new(0),
    install_phase: AtomicU32::new(0),
    last_request_id: AtomicU32::new(0),
    last_status_request_id: AtomicU32::new(0),
    last_slot: AtomicU32::new(u32::MAX),
    last_token: AtomicU32::new(0),
    error_kind: AtomicU32::new(0),
    driver_run_state: AtomicU32::new(0),
    driver_active_units: AtomicU32::new(0),
    driver_runnable_units: AtomicU32::new(0),
    driver_running_units: AtomicU32::new(0),
    driver_blocked_units: AtomicU32::new(0),
    driver_summary_error: AtomicU32::new(0),
    driver_runtime_realized: AtomicU32::new(0),
    last_spawn_phase: AtomicU32::new(0),
    green_task_entry_phase: AtomicU32::new(0),
    green_task_entry_failure_kind: AtomicU32::new(0),
    green_resume_phase: AtomicU32::new(0),
    green_resume_error_kind: AtomicU32::new(0),
    magic_end: PIO_COURIER_DEBUG_MAGIC_END,
};

type PlatformStreamHandle = <PlatformPcu as PcuDispatchContract>::StreamHandle;

struct PioCourierCommandProtocol;
struct PioCourierStatusProtocol;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PioStreamInstallSpec {
    kernel_id: PcuKernelId,
    pattern: PcuStreamPattern,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PioCourierCommandKind {
    InstallStream(PioStreamInstallSpec),
    StreamState { slot: u8, token: u32 },
    StartStream { slot: u8, token: u32 },
    StopStream { slot: u8, token: u32 },
    ProcessWord { slot: u8, token: u32, word: u32 },
    UninstallStream { slot: u8, token: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PioCourierCommand {
    request_id: u32,
    kind: PioCourierCommandKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PioCourierStatus {
    StreamInstalled {
        request_id: u32,
        slot: u8,
        token: u32,
    },
    StreamState {
        request_id: u32,
        state: PcuPersistentState,
    },
    WordProcessed {
        request_id: u32,
        word: u32,
    },
    Ack {
        request_id: u32,
    },
    Failed {
        request_id: u32,
        kind: PcuErrorKind,
    },
}

impl ProtocolContract for PioCourierCommandProtocol {
    type Message = PioCourierCommand;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x7f0d_1c0f_4b7b_4e7d_8e11_0000_0000_1301),
        version: ProtocolVersion::new(1, 0, 0),
        caps: ProtocolCaps::empty(),
        bootstrap: ProtocolBootstrapKind::Immediate,
        debug_view: ProtocolDebugView::Structured,
        transport: ProtocolTransportRequirements::message_local(),
        implementation: ProtocolImplementationKind::Native,
    };
}

impl ProtocolContract for PioCourierStatusProtocol {
    type Message = PioCourierStatus;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x7f0d_1c0f_4b7b_4e7d_8e11_0000_0000_1302),
        version: ProtocolVersion::new(1, 0, 0),
        caps: ProtocolCaps::empty(),
        bootstrap: ProtocolBootstrapKind::Immediate,
        debug_view: ProtocolDebugView::Structured,
        transport: ProtocolTransportRequirements::message_local(),
        implementation: ProtocolImplementationKind::Native,
    };
}

struct ClientIoGuard<'a> {
    lock: &'a SpinMutex,
}

impl Drop for ClientIoGuard<'_> {
    fn drop(&mut self) {
        unsafe { self.lock.unlock_unchecked() };
    }
}

struct PioCourierClientIo {
    commands: LocalChannel<PioCourierCommandProtocol, PIO_COMMAND_CAPACITY, 1>,
    statuses: LocalChannel<PioCourierStatusProtocol, PIO_STATUS_CAPACITY, 1>,
    command_producer: usize,
    status_consumer: usize,
    next_request_id: UnsafeCell<u32>,
    request_lock: SpinMutex,
}

unsafe impl Sync for PioCourierClientIo {}

impl PioCourierClientIo {
    fn new() -> Result<(Self, usize, usize), PcuError> {
        let request = TransportAttachmentRequest::cross_courier()
            .with_requested_law(TransportAttachmentLaw::ExclusiveSpsc);
        let commands = LocalChannel::<PioCourierCommandProtocol, PIO_COMMAND_CAPACITY, 1>::new_with_attachment_law(
            TransportAttachmentLaw::ExclusiveSpsc,
        )
        .map_err(pcu_error_from_channel)?;
        let statuses = LocalChannel::<PioCourierStatusProtocol, PIO_STATUS_CAPACITY, 1>::new_with_attachment_law(
            TransportAttachmentLaw::ExclusiveSpsc,
        )
        .map_err(pcu_error_from_channel)?;
        let command_producer = commands
            .attach_producer(request)
            .map_err(pcu_error_from_transport)?;
        let command_consumer = commands
            .attach_consumer(request)
            .map_err(pcu_error_from_transport)?;
        let status_producer = statuses
            .attach_producer(request)
            .map_err(pcu_error_from_transport)?;
        let status_consumer = statuses
            .attach_consumer(request)
            .map_err(pcu_error_from_transport)?;

        Ok((
            Self {
                commands,
                statuses,
                command_producer,
                status_consumer,
                next_request_id: UnsafeCell::new(0),
                request_lock: SpinMutex::new(),
            },
            command_consumer,
            status_producer,
        ))
    }

    fn transact(&self, kind: PioCourierCommandKind) -> Result<PioCourierStatus, PcuError> {
        PIO_COURIER_DEBUG_STATE
            .client_phase
            .store(1, Ordering::Release);
        self.request_lock.lock().map_err(|_| PcuError::busy())?;
        let _guard = ClientIoGuard {
            lock: &self.request_lock,
        };
        let request_id = unsafe {
            let next = (*self.next_request_id.get()).wrapping_add(1).max(1);
            *self.next_request_id.get() = next;
            next
        };
        PIO_COURIER_DEBUG_STATE
            .last_request_id
            .store(request_id, Ordering::Release);
        let command = PioCourierCommand { request_id, kind };
        loop {
            match self.commands.try_send(self.command_producer, command) {
                Ok(()) => {
                    PIO_COURIER_DEBUG_STATE
                        .client_phase
                        .store(2, Ordering::Release);
                    break;
                }
                Err(error) if error.kind() == ChannelErrorKind::Busy => {
                    PIO_COURIER_DEBUG_STATE
                        .client_phase
                        .store(3, Ordering::Release);
                    wait_for_firmware_progress()
                }
                Err(error) => return Err(pcu_error_from_channel(error)),
            }
        }
        pump_pio_courier_progress();
        PIO_COURIER_DEBUG_STATE
            .client_phase
            .store(4, Ordering::Release);

        loop {
            match self.statuses.try_receive(self.status_consumer) {
                Ok(Some(status)) if status_request_id(status) == request_id => {
                    PIO_COURIER_DEBUG_STATE
                        .client_phase
                        .store(5, Ordering::Release);
                    PIO_COURIER_DEBUG_STATE
                        .last_status_request_id
                        .store(request_id, Ordering::Release);
                    return Ok(status);
                }
                Ok(Some(_)) => return Err(PcuError::state_conflict()),
                Ok(None) => {
                    PIO_COURIER_DEBUG_STATE
                        .client_phase
                        .store(6, Ordering::Release);
                    pump_pio_courier_progress();
                    wait_for_firmware_progress()
                }
                Err(error) if error.kind() == ChannelErrorKind::Busy => {
                    PIO_COURIER_DEBUG_STATE
                        .client_phase
                        .store(7, Ordering::Release);
                    pump_pio_courier_progress();
                    wait_for_firmware_progress()
                }
                Err(error) => return Err(pcu_error_from_channel(error)),
            }
        }
    }
}

struct PioCourierService {
    client: PioCourierClientIo,
    command_consumer: usize,
    status_producer: usize,
}

struct PioServiceSlot {
    state: AtomicU8,
    value: UnsafeCell<MaybeUninit<PioCourierService>>,
}

impl PioServiceSlot {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(SERVICE_UNINITIALIZED),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    fn get_or_init(&self) -> Result<&'static PioCourierService, PcuError> {
        loop {
            match self.state.load(Ordering::Acquire) {
                SERVICE_READY => return Ok(unsafe { &*(*self.value.get()).as_ptr() }),
                SERVICE_FAILED => return Err(service_error_from_state()),
                SERVICE_UNINITIALIZED => {
                    PIO_COURIER_DEBUG_STATE
                        .service_phase
                        .store(0x100, Ordering::Release);
                    PIO_COURIER_DEBUG_STATE
                        .install_phase
                        .store(0x100, Ordering::Release);
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
                    let initialized = (|| -> Result<&'static PioCourierService, PcuError> {
                        PIO_COURIER_DEBUG_STATE
                            .service_phase
                            .store(0x101, Ordering::Release);
                        PIO_COURIER_DEBUG_STATE
                            .install_phase
                            .store(0x101, Ordering::Release);
                        let (client, command_consumer, status_producer) =
                            PioCourierClientIo::new()?;
                        PIO_COURIER_DEBUG_STATE
                            .service_phase
                            .store(0x102, Ordering::Release);
                        PIO_COURIER_DEBUG_STATE
                            .install_phase
                            .store(0x102, Ordering::Release);
                        let service = PioCourierService {
                            client,
                            command_consumer,
                            status_producer,
                        };
                        unsafe { (*self.value.get()).write(service) };
                        let service = unsafe { &*(*self.value.get()).as_ptr() };
                        ensure_firmware_supervisor()?;
                        PIO_COURIER_DEBUG_STATE
                            .service_phase
                            .store(0x103, Ordering::Release);
                        PIO_COURIER_DEBUG_STATE
                            .install_phase
                            .store(0x103, Ordering::Release);
                        ensure_pio_service_spawned(service)?;
                        PIO_COURIER_DEBUG_STATE
                            .service_phase
                            .store(0x104, Ordering::Release);
                        PIO_COURIER_DEBUG_STATE
                            .install_phase
                            .store(0x104, Ordering::Release);
                        Ok(service)
                    })();
                    match initialized {
                        Ok(service) => {
                            self.state.store(SERVICE_READY, Ordering::Release);
                            PIO_COURIER_DEBUG_STATE
                                .service_phase
                                .store(0x105, Ordering::Release);
                            return Ok(service);
                        }
                        Err(error) => {
                            PIO_SERVICE_ERROR_KIND
                                .store(encode_error_kind(error.kind()), Ordering::Release);
                            PIO_COURIER_DEBUG_STATE.error_kind.store(
                                u32::from(encode_error_kind(error.kind())),
                                Ordering::Release,
                            );
                            self.state.store(SERVICE_FAILED, Ordering::Release);
                            PIO_COURIER_DEBUG_STATE
                                .service_phase
                                .store(0x1EE, Ordering::Release);
                            return Err(error);
                        }
                    }
                }
                _ => {
                    PIO_COURIER_DEBUG_STATE
                        .service_phase
                        .store(0x1FD, Ordering::Release);
                    wait_for_firmware_progress()
                }
            }
        }
    }
}

unsafe impl Sync for PioServiceSlot {}

#[derive(Clone, Copy)]
pub struct SystemPioCourier {
    client: &'static PioCourierClientIo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PioCourierUnsupportedFiniteHandle;

impl PcuFiniteHandle for PioCourierUnsupportedFiniteHandle {
    fn state(&self) -> Result<PcuFiniteState, PcuError> {
        Err(PcuError::unsupported())
    }

    fn wait(self) -> Result<(), PcuError> {
        Err(PcuError::unsupported())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PioCourierUnsupportedPersistentHandle;

impl PcuPersistentHandle for PioCourierUnsupportedPersistentHandle {
    fn state(&self) -> Result<PcuPersistentState, PcuError> {
        Err(PcuError::unsupported())
    }

    fn start(&mut self) -> Result<(), PcuError> {
        Err(PcuError::unsupported())
    }

    fn stop(&mut self) -> Result<(), PcuError> {
        Err(PcuError::unsupported())
    }

    fn uninstall(self) -> Result<(), PcuError> {
        Err(PcuError::unsupported())
    }
}

#[derive(Clone, Copy)]
pub struct PioCourierStreamHandle {
    client: &'static PioCourierClientIo,
    slot: u8,
    token: u32,
}

impl PcuPersistentHandle for PioCourierStreamHandle {
    fn state(&self) -> Result<PcuPersistentState, PcuError> {
        match self.client.transact(PioCourierCommandKind::StreamState {
            slot: self.slot,
            token: self.token,
        })? {
            PioCourierStatus::StreamState { state, .. } => Ok(state),
            PioCourierStatus::Failed { kind, .. } => Err(pcu_error_from_kind(kind)),
            _ => Err(PcuError::state_conflict()),
        }
    }

    fn start(&mut self) -> Result<(), PcuError> {
        match self.client.transact(PioCourierCommandKind::StartStream {
            slot: self.slot,
            token: self.token,
        })? {
            PioCourierStatus::Ack { .. } => Ok(()),
            PioCourierStatus::Failed { kind, .. } => Err(pcu_error_from_kind(kind)),
            _ => Err(PcuError::state_conflict()),
        }
    }

    fn stop(&mut self) -> Result<(), PcuError> {
        match self.client.transact(PioCourierCommandKind::StopStream {
            slot: self.slot,
            token: self.token,
        })? {
            PioCourierStatus::Ack { .. } => Ok(()),
            PioCourierStatus::Failed { kind, .. } => Err(pcu_error_from_kind(kind)),
            _ => Err(PcuError::state_conflict()),
        }
    }

    fn uninstall(self) -> Result<(), PcuError> {
        match self
            .client
            .transact(PioCourierCommandKind::UninstallStream {
                slot: self.slot,
                token: self.token,
            })? {
            PioCourierStatus::Ack { .. } => Ok(()),
            PioCourierStatus::Failed { kind, .. } => Err(pcu_error_from_kind(kind)),
            _ => Err(PcuError::state_conflict()),
        }
    }
}

impl PioCourierStreamHandle {
    /// Pushes one word through the installed PIO stream and returns the transformed output.
    ///
    /// # Errors
    ///
    /// Returns one honest courier, state, or PIO execution error.
    pub fn process_word(&mut self, word: u32) -> Result<u32, PcuError> {
        match self.client.transact(PioCourierCommandKind::ProcessWord {
            slot: self.slot,
            token: self.token,
            word,
        })? {
            PioCourierStatus::WordProcessed { word, .. } => Ok(word),
            PioCourierStatus::Failed { kind, .. } => Err(pcu_error_from_kind(kind)),
            _ => Err(PcuError::state_conflict()),
        }
    }
}

pub fn system_pio_courier() -> Result<SystemPioCourier, PcuError> {
    let service = PIO_SERVICE_SLOT.get_or_init()?;
    Ok(SystemPioCourier {
        client: &service.client,
    })
}

impl PcuDispatchContract for SystemPioCourier {
    type DispatchHandle = PioCourierUnsupportedFiniteHandle;
    type CommandHandle = PioCourierUnsupportedFiniteHandle;
    type TransactionHandle = PioCourierUnsupportedFiniteHandle;
    type StreamHandle = PioCourierStreamHandle;
    type SignalHandle = PioCourierUnsupportedPersistentHandle;

    fn submit_dispatch(
        &self,
        _submission: PcuDispatchSubmission<'_>,
        _bindings: PcuInvocationBindings<'_>,
        _parameters: PcuInvocationParameters<'_>,
    ) -> Result<Self::DispatchHandle, PcuError> {
        Err(PcuError::unsupported())
    }

    fn submit_command(
        &self,
        _submission: PcuCommandSubmission<'_>,
        _parameters: PcuInvocationParameters<'_>,
    ) -> Result<Self::CommandHandle, PcuError> {
        Err(PcuError::unsupported())
    }

    fn submit_transaction(
        &self,
        _submission: PcuTransactionSubmission<'_>,
        _bindings: PcuInvocationBindings<'_>,
        _parameters: PcuInvocationParameters<'_>,
    ) -> Result<Self::TransactionHandle, PcuError> {
        Err(PcuError::unsupported())
    }

    fn install_stream(
        &self,
        installation: PcuStreamInstallation<'_>,
        bindings: PcuInvocationBindings<'_>,
        parameters: PcuInvocationParameters<'_>,
    ) -> Result<Self::StreamHandle, PcuError> {
        let spec = stream_install_spec(installation, bindings, parameters)?;
        match self
            .client
            .transact(PioCourierCommandKind::InstallStream(spec))?
        {
            PioCourierStatus::StreamInstalled { slot, token, .. } => Ok(PioCourierStreamHandle {
                client: self.client,
                slot,
                token,
            }),
            PioCourierStatus::Failed { kind, .. } => Err(pcu_error_from_kind(kind)),
            _ => Err(PcuError::state_conflict()),
        }
    }

    fn install_signal(
        &self,
        _installation: PcuSignalInstallation<'_>,
        _parameters: PcuInvocationParameters<'_>,
    ) -> Result<Self::SignalHandle, PcuError> {
        Err(PcuError::unsupported())
    }
}

fn ensure_firmware_supervisor() -> Result<(), PcuError> {
    ensure_root_courier().map_err(|_| PcuError::state_conflict())
}

fn ensure_pio_service_spawned(service: &'static PioCourierService) -> Result<(), PcuError> {
    FUSION_GREEN_TASK_ENTRY_PHASE.store(0, Ordering::Release);
    FUSION_GREEN_TASK_ENTRY_FAILURE_KIND.store(0, Ordering::Release);
    FUSION_GREEN_RESUME_PHASE.store(0, Ordering::Release);
    FUSION_GREEN_RESUME_ERROR_KIND.store(0, Ordering::Release);
    PIO_COURIER_DEBUG_STATE
        .service_phase
        .store(0x110, Ordering::Release);
    PIO_COURIER_DEBUG_STATE
        .install_phase
        .store(0x110, Ordering::Release);
    let handle = root_runtime()
        .spawn_fiber_with_stack::<DRIVER_COURIER_STACK_BYTES, _, _>(move || {
            run_pio_service(service)
        })
        .map_err(pcu_error_from_fiber)?;
    PIO_COURIER_DEBUG_STATE
        .service_phase
        .store(0x111, Ordering::Release);
    PIO_COURIER_DEBUG_STATE
        .install_phase
        .store(0x111, Ordering::Release);
    // This is one process-lifetime service fiber. Dropping the only current-thread handle here is
    // not useful, and keeping it alive avoids accidental reclamation of the long-lived driver lane.
    core::mem::forget(handle);
    PIO_COURIER_DEBUG_STATE
        .service_phase
        .store(0x112, Ordering::Release);
    PIO_COURIER_DEBUG_STATE
        .install_phase
        .store(0x112, Ordering::Release);
    snapshot_driver_runtime_state();
    Ok(())
}

struct InstalledStreamHandle {
    token: u32,
    handle: PlatformStreamHandle,
}

fn run_pio_service(service: &'static PioCourierService) {
    PIO_COURIER_DEBUG_STATE
        .service_phase
        .store(1, Ordering::Release);
    let mut next_stream_token = 0_u32;
    let mut streams: [Option<InstalledStreamHandle>; PIO_STREAM_HANDLE_CAPACITY] =
        array::from_fn(|_| None);

    loop {
        PIO_COURIER_DEBUG_STATE
            .service_phase
            .store(2, Ordering::Release);
        while let Some(command) = match service
            .client
            .commands
            .try_receive(service.command_consumer)
        {
            Ok(command) => {
                PIO_COURIER_DEBUG_STATE
                    .service_phase
                    .store(3, Ordering::Release);
                command
            }
            Err(error) if error.kind() == ChannelErrorKind::Busy => {
                PIO_COURIER_DEBUG_STATE
                    .service_phase
                    .store(4, Ordering::Release);
                None
            }
            Err(_) => {
                PIO_COURIER_DEBUG_STATE
                    .service_phase
                    .store(0xE1, Ordering::Release);
                None
            }
        } {
            PIO_COURIER_DEBUG_STATE
                .service_phase
                .store(5, Ordering::Release);
            PIO_COURIER_DEBUG_STATE
                .last_request_id
                .store(command.request_id, Ordering::Release);
            let status = match handle_pio_command(&mut streams, &mut next_stream_token, command) {
                Ok(status) => status,
                Err(error) => PioCourierStatus::Failed {
                    request_id: command.request_id,
                    kind: error.kind(),
                },
            };
            PIO_COURIER_DEBUG_STATE
                .service_phase
                .store(6, Ordering::Release);
            loop {
                match service
                    .client
                    .statuses
                    .try_send(service.status_producer, status)
                {
                    Ok(()) => {
                        PIO_COURIER_DEBUG_STATE
                            .service_phase
                            .store(7, Ordering::Release);
                        PIO_COURIER_DEBUG_STATE
                            .last_status_request_id
                            .store(status_request_id(status), Ordering::Release);
                        break;
                    }
                    Err(error) if error.kind() == ChannelErrorKind::Busy => {
                        PIO_COURIER_DEBUG_STATE
                            .service_phase
                            .store(8, Ordering::Release);
                        if fiber_yield_now().is_err() {
                            let _ = system_monotonic_time().sleep_for(Duration::from_micros(50));
                        }
                    }
                    Err(_) => {
                        PIO_COURIER_DEBUG_STATE
                            .service_phase
                            .store(0xE2, Ordering::Release);
                        break;
                    }
                }
            }
        }
        PIO_COURIER_DEBUG_STATE
            .service_phase
            .store(9, Ordering::Release);
        let _ = fiber_yield_now();
    }
}

fn handle_pio_command(
    streams: &mut [Option<InstalledStreamHandle>; PIO_STREAM_HANDLE_CAPACITY],
    next_stream_token: &mut u32,
    command: PioCourierCommand,
) -> Result<PioCourierStatus, PcuError> {
    match command.kind {
        PioCourierCommandKind::InstallStream(spec) => {
            PIO_COURIER_DEBUG_STATE
                .install_phase
                .store(1, Ordering::Release);
            let slot = streams
                .iter()
                .position(Option::is_none)
                .ok_or_else(PcuError::resource_exhausted)?;
            PIO_COURIER_DEBUG_STATE
                .last_slot
                .store(slot as u32, Ordering::Release);
            PIO_COURIER_DEBUG_STATE
                .install_phase
                .store(2, Ordering::Release);
            let handle = install_stream_from_spec(spec)?;
            PIO_COURIER_DEBUG_STATE
                .install_phase
                .store(3, Ordering::Release);
            let token = next_token(next_stream_token);
            PIO_COURIER_DEBUG_STATE
                .last_token
                .store(token, Ordering::Release);
            streams[slot] = Some(InstalledStreamHandle { token, handle });
            PIO_COURIER_DEBUG_STATE
                .install_phase
                .store(4, Ordering::Release);
            Ok(PioCourierStatus::StreamInstalled {
                request_id: command.request_id,
                slot: slot as u8,
                token,
            })
        }
        PioCourierCommandKind::StreamState { slot, token } => {
            let entry = stream_entry_mut(streams, slot, token)?;
            Ok(PioCourierStatus::StreamState {
                request_id: command.request_id,
                state: entry.handle.state()?,
            })
        }
        PioCourierCommandKind::StartStream { slot, token } => {
            let entry = stream_entry_mut(streams, slot, token)?;
            entry.handle.start()?;
            Ok(PioCourierStatus::Ack {
                request_id: command.request_id,
            })
        }
        PioCourierCommandKind::StopStream { slot, token } => {
            let entry = stream_entry_mut(streams, slot, token)?;
            entry.handle.stop()?;
            Ok(PioCourierStatus::Ack {
                request_id: command.request_id,
            })
        }
        PioCourierCommandKind::ProcessWord { slot, token, word } => {
            let entry = stream_entry_mut(streams, slot, token)?;
            let word = process_stream_word(&mut entry.handle, word)?;
            Ok(PioCourierStatus::WordProcessed {
                request_id: command.request_id,
                word,
            })
        }
        PioCourierCommandKind::UninstallStream { slot, token } => {
            let entry = streams
                .get_mut(slot as usize)
                .ok_or_else(PcuError::state_conflict)?
                .take()
                .ok_or_else(PcuError::state_conflict)?;
            if entry.token != token {
                streams[slot as usize] = Some(entry);
                return Err(PcuError::state_conflict());
            }
            entry.handle.uninstall()?;
            Ok(PioCourierStatus::Ack {
                request_id: command.request_id,
            })
        }
    }
}

fn stream_install_spec(
    installation: PcuStreamInstallation<'_>,
    bindings: PcuInvocationBindings<'_>,
    parameters: PcuInvocationParameters<'_>,
) -> Result<PioStreamInstallSpec, PcuError> {
    if !bindings.is_empty() || !parameters.is_empty() || !installation.kernel.parameters.is_empty()
    {
        return Err(PcuError::unsupported());
    }
    let [pattern] = installation.kernel.patterns else {
        return Err(PcuError::unsupported());
    };
    if installation.kernel.simple_transform_type() != Some(PcuStreamValueType::U32) {
        return Err(PcuError::unsupported());
    }
    Ok(PioStreamInstallSpec {
        kernel_id: installation.kernel.id,
        pattern: *pattern,
    })
}

fn install_stream_from_spec(spec: PioStreamInstallSpec) -> Result<PlatformStreamHandle, PcuError> {
    PIO_COURIER_DEBUG_STATE
        .install_phase
        .store(10, Ordering::Release);
    let patterns = [spec.pattern];
    let ports = [
        PcuPort::stream_input(Some("in"), PcuStreamValueType::U32.as_value_type()),
        PcuPort::stream_output(Some("out"), PcuStreamValueType::U32.as_value_type()),
    ];
    let kernel = PcuStreamKernelIr {
        id: spec.kernel_id,
        entry_point: "pio.stream",
        bindings: &[],
        ports: &ports,
        parameters: &[],
        patterns: &patterns,
        capabilities: stream_capabilities(spec.pattern),
    };
    PIO_COURIER_DEBUG_STATE
        .install_phase
        .store(11, Ordering::Release);
    system_pcu().install_stream(
        PcuStreamInstallation { kernel: &kernel },
        PcuInvocationBindings::empty(),
        PcuInvocationParameters::empty(),
    )
}

fn stream_capabilities(pattern: PcuStreamPattern) -> PcuStreamCapabilities {
    PcuStreamCapabilities::FIFO_INPUT
        .union(PcuStreamCapabilities::FIFO_OUTPUT)
        .union(pattern.support_flag())
}

fn stream_entry_mut<'a>(
    streams: &'a mut [Option<InstalledStreamHandle>; PIO_STREAM_HANDLE_CAPACITY],
    slot: u8,
    token: u32,
) -> Result<&'a mut InstalledStreamHandle, PcuError> {
    let entry = streams
        .get_mut(slot as usize)
        .ok_or_else(PcuError::state_conflict)?
        .as_mut()
        .ok_or_else(PcuError::state_conflict)?;
    if entry.token != token {
        return Err(PcuError::state_conflict());
    }
    Ok(entry)
}

fn next_token(next: &mut u32) -> u32 {
    *next = next.wrapping_add(1).max(1);
    *next
}

const fn status_request_id(status: PioCourierStatus) -> u32 {
    match status {
        PioCourierStatus::StreamInstalled { request_id, .. }
        | PioCourierStatus::StreamState { request_id, .. }
        | PioCourierStatus::WordProcessed { request_id, .. }
        | PioCourierStatus::Ack { request_id }
        | PioCourierStatus::Failed { request_id, .. } => request_id,
    }
}

fn process_stream_word(handle: &mut PlatformStreamHandle, word: u32) -> Result<u32, PcuError> {
    loop {
        match handle.write_word(word) {
            Ok(()) => break,
            Err(error) if error.kind() == PcuErrorKind::Busy => wait_for_firmware_progress(),
            Err(error) => return Err(error),
        }
    }

    loop {
        match handle.read_word() {
            Ok(word) => return Ok(word),
            Err(error) if error.kind() == PcuErrorKind::Busy => wait_for_firmware_progress(),
            Err(error) => return Err(error),
        }
    }
}

fn wait_for_firmware_progress() {
    if is_in_green_context() {
        let _ = fiber_yield_now();
        snapshot_driver_runtime_state();
        return;
    }
    root_runtime().request_autonomous_dispatch();
    root_runtime().pump_autonomous_best_effort();
    snapshot_driver_runtime_state();
    if system_thread().yield_now().is_ok() {
        return;
    }
    let _ = system_monotonic_time().sleep_for(Duration::from_micros(50));
}

const fn encode_error_kind(kind: PcuErrorKind) -> u8 {
    match kind {
        PcuErrorKind::Unsupported => 1,
        PcuErrorKind::Invalid => 2,
        PcuErrorKind::Busy => 3,
        PcuErrorKind::ResourceExhausted => 4,
        PcuErrorKind::StateConflict => 5,
        PcuErrorKind::Platform(_) => u8::MAX,
    }
}

fn service_error_from_state() -> PcuError {
    let encoded = PIO_SERVICE_ERROR_KIND.load(Ordering::Acquire);
    PIO_COURIER_DEBUG_STATE
        .error_kind
        .store(u32::from(encoded), Ordering::Release);
    match encoded {
        1 => PcuError::unsupported(),
        2 => PcuError::invalid(),
        3 => PcuError::busy(),
        4 => PcuError::resource_exhausted(),
        _ => PcuError::state_conflict(),
    }
}

fn pump_pio_courier_progress() {
    if is_in_green_context() {
        let _ = fiber_yield_now();
    } else {
        root_runtime().pump_autonomous_best_effort();
    }
    snapshot_driver_runtime_state();
}

fn snapshot_driver_runtime_state() {
    let runtime = root_runtime();
    PIO_COURIER_DEBUG_STATE.driver_runtime_realized.store(
        u32::from(runtime.fiber_runtime_realized()),
        Ordering::Release,
    );
    PIO_COURIER_DEBUG_STATE.last_spawn_phase.store(
        CurrentFiberAsyncSingleton::debug_last_spawn_phase(),
        Ordering::Release,
    );
    let Ok(summary) = runtime.runtime_summary() else {
        PIO_COURIER_DEBUG_STATE
            .driver_summary_error
            .store(1, Ordering::Release);
        return;
    };
    let lane = summary
        .fiber_lane
        .unwrap_or(fusion_sys::courier::CourierLaneSummary::new(
            fusion_sys::courier::RunnableUnitKind::Fiber,
        ));
    PIO_COURIER_DEBUG_STATE
        .driver_summary_error
        .store(0, Ordering::Release);
    PIO_COURIER_DEBUG_STATE
        .driver_run_state
        .store(encode_run_state(summary.run_state), Ordering::Release);
    PIO_COURIER_DEBUG_STATE.driver_active_units.store(
        u32::try_from(lane.active_units).unwrap_or(u32::MAX),
        Ordering::Release,
    );
    PIO_COURIER_DEBUG_STATE.driver_runnable_units.store(
        u32::try_from(lane.runnable_units).unwrap_or(u32::MAX),
        Ordering::Release,
    );
    PIO_COURIER_DEBUG_STATE.driver_running_units.store(
        u32::try_from(lane.running_units).unwrap_or(u32::MAX),
        Ordering::Release,
    );
    PIO_COURIER_DEBUG_STATE.driver_blocked_units.store(
        u32::try_from(lane.blocked_units).unwrap_or(u32::MAX),
        Ordering::Release,
    );
    PIO_COURIER_DEBUG_STATE.green_task_entry_phase.store(
        FUSION_GREEN_TASK_ENTRY_PHASE.load(Ordering::Acquire),
        Ordering::Release,
    );
    PIO_COURIER_DEBUG_STATE.green_task_entry_failure_kind.store(
        FUSION_GREEN_TASK_ENTRY_FAILURE_KIND.load(Ordering::Acquire),
        Ordering::Release,
    );
    PIO_COURIER_DEBUG_STATE.green_resume_phase.store(
        FUSION_GREEN_RESUME_PHASE.load(Ordering::Acquire),
        Ordering::Release,
    );
    PIO_COURIER_DEBUG_STATE.green_resume_error_kind.store(
        FUSION_GREEN_RESUME_ERROR_KIND.load(Ordering::Acquire),
        Ordering::Release,
    );
}

const fn encode_run_state(state: fusion_sys::courier::CourierRunState) -> u32 {
    match state {
        fusion_sys::courier::CourierRunState::Idle => 0,
        fusion_sys::courier::CourierRunState::Runnable => 1,
        fusion_sys::courier::CourierRunState::Running => 2,
        fusion_sys::courier::CourierRunState::Stale => 3,
        fusion_sys::courier::CourierRunState::NonResponsive => 4,
    }
}

const fn pcu_error_from_kind(kind: PcuErrorKind) -> PcuError {
    match kind {
        PcuErrorKind::Unsupported => PcuError::unsupported(),
        PcuErrorKind::Invalid => PcuError::invalid(),
        PcuErrorKind::Busy => PcuError::busy(),
        PcuErrorKind::ResourceExhausted => PcuError::resource_exhausted(),
        PcuErrorKind::StateConflict => PcuError::state_conflict(),
        PcuErrorKind::Platform(code) => PcuError::platform(code),
    }
}

fn pcu_error_from_channel(error: ChannelError) -> PcuError {
    match error.kind() {
        ChannelErrorKind::Unsupported => PcuError::unsupported(),
        ChannelErrorKind::Invalid => PcuError::invalid(),
        ChannelErrorKind::Busy => PcuError::busy(),
        ChannelErrorKind::ResourceExhausted => PcuError::resource_exhausted(),
        ChannelErrorKind::StateConflict
        | ChannelErrorKind::PermissionDenied
        | ChannelErrorKind::ProtocolMismatch
        | ChannelErrorKind::TransportDenied => PcuError::state_conflict(),
        ChannelErrorKind::Platform(code) => PcuError::platform(code),
    }
}

fn pcu_error_from_transport(error: TransportError) -> PcuError {
    match error.kind() {
        fusion_pal::contract::pal::interconnect::transport::TransportErrorKind::Unsupported => {
            PcuError::unsupported()
        }
        fusion_pal::contract::pal::interconnect::transport::TransportErrorKind::Invalid => {
            PcuError::invalid()
        }
        fusion_pal::contract::pal::interconnect::transport::TransportErrorKind::Busy => {
            PcuError::busy()
        }
        fusion_pal::contract::pal::interconnect::transport::TransportErrorKind::ResourceExhausted => {
            PcuError::resource_exhausted()
        }
        fusion_pal::contract::pal::interconnect::transport::TransportErrorKind::StateConflict => {
            PcuError::state_conflict()
        }
        fusion_pal::contract::pal::interconnect::transport::TransportErrorKind::PermissionDenied => {
            PcuError::state_conflict()
        }
        fusion_pal::contract::pal::interconnect::transport::TransportErrorKind::NotAttached => {
            PcuError::state_conflict()
        }
        fusion_pal::contract::pal::interconnect::transport::TransportErrorKind::Platform(code) => {
            PcuError::platform(code)
        }
    }
}

fn pcu_error_from_fiber(error: fusion_sys::fiber::FiberError) -> PcuError {
    match error.kind() {
        fusion_sys::fiber::FiberErrorKind::Unsupported => PcuError::unsupported(),
        fusion_sys::fiber::FiberErrorKind::Invalid => PcuError::invalid(),
        fusion_sys::fiber::FiberErrorKind::ResourceExhausted => PcuError::resource_exhausted(),
        fusion_sys::fiber::FiberErrorKind::DeadlineExceeded => PcuError::busy(),
        fusion_sys::fiber::FiberErrorKind::StateConflict => PcuError::state_conflict(),
        fusion_sys::fiber::FiberErrorKind::Context(kind) => match kind {
            fusion_pal::contract::pal::runtime::context::ContextErrorKind::Unsupported => {
                PcuError::unsupported()
            }
            fusion_pal::contract::pal::runtime::context::ContextErrorKind::Invalid => {
                PcuError::invalid()
            }
            fusion_pal::contract::pal::runtime::context::ContextErrorKind::Busy => PcuError::busy(),
            fusion_pal::contract::pal::runtime::context::ContextErrorKind::PermissionDenied => {
                PcuError::state_conflict()
            }
            fusion_pal::contract::pal::runtime::context::ContextErrorKind::ResourceExhausted => {
                PcuError::resource_exhausted()
            }
            fusion_pal::contract::pal::runtime::context::ContextErrorKind::StateConflict => {
                PcuError::state_conflict()
            }
            fusion_pal::contract::pal::runtime::context::ContextErrorKind::Platform(code) => {
                PcuError::platform(code)
            }
        },
    }
}
