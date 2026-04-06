//! RP2350 example-side GPIO composition over one fiber-owned channel service.

use core::array;
use core::pin::Pin;
use core::sync::atomic::{
    AtomicU32,
    Ordering,
};

use fusion_hal::contract::drivers::bus::gpio::{
    GpioCapabilities,
    GpioControlContract,
    GpioDriveStrength,
    GpioError,
    GpioErrorKind,
    GpioOwnedPinContract,
    GpioOutputPinContract,
};
use fusion_firmware::sys::hal::drivers::bus::gpio::{
    SystemGpioPin,
    system_gpio,
};
use fusion_std::thread::yield_now;
use fusion_sys::channel::{
    ChannelError,
    ChannelErrorKind,
    ChannelReceiveContract,
    ChannelSendContract,
    LocalChannel,
};
use fusion_sys::fiber::{
    FiberError,
    FiberErrorKind,
};
use fusion_sys::transport::protocol::{
    ProtocolContract,
    ProtocolBootstrapKind,
    ProtocolCaps,
    ProtocolDebugView,
    ProtocolDescriptor,
    ProtocolId,
    ProtocolImplementationKind,
    ProtocolTransportRequirements,
    ProtocolVersion,
};
use fusion_sys::transport::{
    TransportAttachmentControlContract,
    TransportAttachmentLaw,
    TransportAttachmentRequest,
    TransportError,
    TransportErrorKind,
};

use crate::runtime::{
    spawn_with_stack,
    wait_for_runtime_progress,
};

type SelectedHardwarePin = SystemGpioPin;

const REQUEST_ID_WRAP_SENTINEL: u32 = u32::MAX;
pub const RP2350_GPIO_BATCH_MAX_OPS: usize = 96;

#[unsafe(no_mangle)]
pub static RP2350_GPIO_SERVICE_HEARTBEAT: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static RP2350_GPIO_LAST_REQUEST_ID: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static RP2350_GPIO_LAST_COMMAND_KIND: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static RP2350_GPIO_CLIENT_REQUEST_ID: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static RP2350_GPIO_CLIENT_PHASE: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone, Copy)]
enum Rp2350GpioBatchOp {
    SetLevel {
        slot_index: u8,
        high: bool,
    },
    Pause {
        spins: u8,
    },
}

#[derive(Debug, Clone, Copy)]
enum Rp2350GpioCommandKind {
    ConfigureOutput {
        initial_high: bool,
    },
    SetLevel {
        high: bool,
    },
    SetLevelsBatch {
        count: u8,
        ops: [Rp2350GpioBatchOp; RP2350_GPIO_BATCH_MAX_OPS],
    },
}

#[derive(Debug, Clone, Copy)]
struct Rp2350GpioCommand {
    request_id: u32,
    slot_index: u8,
    kind: Rp2350GpioCommandKind,
}

#[derive(Debug, Clone, Copy)]
enum Rp2350GpioStatus {
    Completed {
        request_id: u32,
    },
    Failed {
        request_id: u32,
        kind: GpioErrorKind,
    },
}

struct Rp2350GpioCommandProtocol;

impl ProtocolContract for Rp2350GpioCommandProtocol {
    type Message = Rp2350GpioCommand;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x7f0d_1c0f_4b7b_4e7d_8e11_0000_0000_0001),
        version: ProtocolVersion::new(1, 0, 0),
        caps: ProtocolCaps::empty(),
        bootstrap: ProtocolBootstrapKind::Immediate,
        debug_view: ProtocolDebugView::Structured,
        transport: ProtocolTransportRequirements::message_local(),
        implementation: ProtocolImplementationKind::Native,
    };
}

struct Rp2350GpioStatusProtocol;

impl ProtocolContract for Rp2350GpioStatusProtocol {
    type Message = Rp2350GpioStatus;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x7f0d_1c0f_4b7b_4e7d_8e11_0000_0000_0002),
        version: ProtocolVersion::new(1, 0, 0),
        caps: ProtocolCaps::empty(),
        bootstrap: ProtocolBootstrapKind::Immediate,
        debug_view: ProtocolDebugView::Structured,
        transport: ProtocolTransportRequirements::message_local(),
        implementation: ProtocolImplementationKind::Native,
    };
}

struct Rp2350GpioClientIo<const COMMAND_CAPACITY: usize, const STATUS_CAPACITY: usize> {
    commands: LocalChannel<Rp2350GpioCommandProtocol, COMMAND_CAPACITY, 1>,
    statuses: LocalChannel<Rp2350GpioStatusProtocol, STATUS_CAPACITY, 1>,
    command_producer: usize,
    status_consumer: usize,
    next_request_id: AtomicU32,
}

impl<const COMMAND_CAPACITY: usize, const STATUS_CAPACITY: usize>
    Rp2350GpioClientIo<COMMAND_CAPACITY, STATUS_CAPACITY>
{
    fn new() -> Result<(Self, usize, usize), GpioError> {
        let request = TransportAttachmentRequest::same_courier()
            .with_requested_law(TransportAttachmentLaw::ExclusiveSpsc);
        let commands = LocalChannel::<Rp2350GpioCommandProtocol, COMMAND_CAPACITY, 1>::new_with_attachment_law(
            TransportAttachmentLaw::ExclusiveSpsc,
        )
        .map_err(gpio_error_from_channel)?;
        let statuses = LocalChannel::<Rp2350GpioStatusProtocol, STATUS_CAPACITY, 1>::new_with_attachment_law(
            TransportAttachmentLaw::ExclusiveSpsc,
        )
        .map_err(gpio_error_from_channel)?;
        let command_producer = commands.attach_producer(request).map_err(gpio_error_from_transport)?;
        let command_consumer = commands.attach_consumer(request).map_err(gpio_error_from_transport)?;
        let status_producer = statuses.attach_producer(request).map_err(gpio_error_from_transport)?;
        let status_consumer = statuses.attach_consumer(request).map_err(gpio_error_from_transport)?;

        Ok((
            Self {
                commands,
                statuses,
                command_producer,
                status_consumer,
                next_request_id: AtomicU32::new(0),
            },
            command_consumer,
            status_producer,
        ))
    }

    fn next_request_id(&self) -> u32 {
        let next = self.next_request_id.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        if next == 0 || next == REQUEST_ID_WRAP_SENTINEL {
            self.next_request_id.store(1, Ordering::Release);
            1
        } else {
            next
        }
    }
}

/// Single-client channel-backed RP2350 GPIO service for example peripherals.
///
/// This stays intentionally narrow:
/// - output pins only
/// - one client controller path
/// - one service fiber owns the real hardware pins
pub struct Rp2350FiberGpioService<
    const MAX_PINS: usize,
    const COMMAND_CAPACITY: usize = 16,
    const STATUS_CAPACITY: usize = 16,
> {
    client: Rp2350GpioClientIo<COMMAND_CAPACITY, STATUS_CAPACITY>,
    command_consumer: usize,
    status_producer: usize,
    claimed_pins: [Option<SelectedHardwarePin>; MAX_PINS],
    pin_numbers: [u8; MAX_PINS],
    claimed_count: usize,
    spawned: bool,
}

/// One channel-backed RP2350 GPIO output pin.
#[derive(Clone, Copy)]
pub struct Rp2350FiberGpioOutputPin<const COMMAND_CAPACITY: usize, const STATUS_CAPACITY: usize> {
    client: &'static Rp2350GpioClientIo<COMMAND_CAPACITY, STATUS_CAPACITY>,
    slot_index: u8,
    pin: u8,
}

/// One batched GPIO transaction over the example-side fiber GPIO service.
pub struct Rp2350FiberGpioBatch<const COMMAND_CAPACITY: usize, const STATUS_CAPACITY: usize> {
    client: &'static Rp2350GpioClientIo<COMMAND_CAPACITY, STATUS_CAPACITY>,
    ops: [Rp2350GpioBatchOp; RP2350_GPIO_BATCH_MAX_OPS],
    count: u8,
}

impl<const MAX_PINS: usize, const COMMAND_CAPACITY: usize, const STATUS_CAPACITY: usize>
    Rp2350FiberGpioService<MAX_PINS, COMMAND_CAPACITY, STATUS_CAPACITY>
{
    /// Builds one new RP2350 GPIO service with exclusive SPSC request/response lanes.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the backing channels cannot be created or attached.
    pub fn new() -> Result<Self, GpioError> {
        let (client, command_consumer, status_producer) = Rp2350GpioClientIo::new()?;
        Ok(Self {
            client,
            command_consumer,
            status_producer,
            claimed_pins: array::from_fn(|_| None),
            pin_numbers: [u8::MAX; MAX_PINS],
            claimed_count: 0,
            spawned: false,
        })
    }

    /// Claims one RP2350 GPIO pin for channel-backed output composition.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the pin cannot be claimed, configured, or tracked.
    pub fn claim_output_pin(
        self: Pin<&'static mut Self>,
        pin: u8,
        drive_strength: GpioDriveStrength,
    ) -> Result<Rp2350FiberGpioOutputPin<COMMAND_CAPACITY, STATUS_CAPACITY>, GpioError> {
        // SAFETY: this service is pinned in static storage by the examples and never moved again.
        let this = unsafe { self.get_unchecked_mut() };
        if this.spawned {
            return Err(GpioError::state_conflict());
        }
        if this.claimed_count == MAX_PINS {
            return Err(GpioError::resource_exhausted());
        }

        let gpio = system_gpio()?;
        let mut hardware_pin = gpio.take_pin(pin)?;
        hardware_pin.set_drive_strength(drive_strength)?;

        let slot_index = this.claimed_count;
        this.pin_numbers[slot_index] = pin;
        this.claimed_pins[slot_index] = Some(hardware_pin);
        this.claimed_count += 1;

        Ok(Rp2350FiberGpioOutputPin {
            client: &this.client,
            slot_index: slot_index as u8,
            pin,
        })
    }

    /// Spawns the hardware-owning GPIO service fiber.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the service fiber cannot be admitted.
    pub fn spawn<const STACK_BYTES: usize>(self: Pin<&'static mut Self>) -> Result<(), GpioError> {
        // SAFETY: this service is pinned in static storage by the examples and never moved again.
        let this = unsafe { self.get_unchecked_mut() };
        if this.spawned {
            return Err(GpioError::state_conflict());
        }
        this.spawned = true;

        let service_addr = this as *mut Self as usize;
        spawn_with_stack::<STACK_BYTES, _, _>(move || {
            run_gpio_service::<MAX_PINS, COMMAND_CAPACITY, STATUS_CAPACITY>(service_addr)
        })
            .map_err(gpio_error_from_fiber)?;
        Ok(())
    }

    fn pump(&mut self) -> Result<(), GpioError> {
        while let Some(command) = self
            .client
            .commands
            .try_receive(self.command_consumer)
            .map_err(gpio_error_from_channel)?
        {
            RP2350_GPIO_SERVICE_HEARTBEAT.fetch_add(1, Ordering::AcqRel);
            RP2350_GPIO_LAST_REQUEST_ID.store(command.request_id, Ordering::Release);
            RP2350_GPIO_LAST_COMMAND_KIND.store(
                match command.kind {
                    Rp2350GpioCommandKind::ConfigureOutput { .. } => 1,
                    Rp2350GpioCommandKind::SetLevel { .. } => 2,
                    Rp2350GpioCommandKind::SetLevelsBatch { .. } => 3,
                },
                Ordering::Release,
            );
            let status = match self.handle_command(command) {
                Ok(()) => Rp2350GpioStatus::Completed {
                    request_id: command.request_id,
                },
                Err(error) => Rp2350GpioStatus::Failed {
                    request_id: command.request_id,
                    kind: error.kind(),
                },
            };
            self.send_status(status)?;
        }

        Ok(())
    }

    fn handle_command(&mut self, command: Rp2350GpioCommand) -> Result<(), GpioError> {
        let slot_index = usize::from(command.slot_index);
        match command.kind {
            Rp2350GpioCommandKind::ConfigureOutput { initial_high } => {
                let pin = self.pin_for_slot(slot_index)?;
                pin.configure_output(initial_high)
            }
            Rp2350GpioCommandKind::SetLevel { high } => {
                let pin = self.pin_for_slot(slot_index)?;
                pin.set_level(high)
            }
            Rp2350GpioCommandKind::SetLevelsBatch { count, ops } => {
                let mut index = 0usize;
                let count = usize::from(count);
                while index < count {
                    match ops[index] {
                        Rp2350GpioBatchOp::SetLevel { slot_index, high } => {
                            let pin = self.pin_for_slot(usize::from(slot_index))?;
                            pin.set_level(high)?;
                        }
                        Rp2350GpioBatchOp::Pause { spins } => {
                            let mut remaining = usize::from(spins);
                            while remaining != 0 {
                                core::hint::spin_loop();
                                remaining -= 1;
                            }
                        }
                    }
                    index += 1;
                }
                Ok(())
            }
        }
    }

    fn pin_for_slot(&mut self, slot_index: usize) -> Result<&mut SelectedHardwarePin, GpioError> {
        if slot_index >= self.claimed_count {
            return Err(GpioError::invalid());
        }
        self.claimed_pins[slot_index]
            .as_mut()
            .ok_or_else(GpioError::state_conflict)
    }

    fn send_status(&self, status: Rp2350GpioStatus) -> Result<(), GpioError> {
        loop {
            match self.client.statuses.try_send(self.status_producer, status) {
                Ok(()) => return Ok(()),
                Err(error) if error.kind() == ChannelErrorKind::Busy => service_wait_for_client()?,
                Err(error) => return Err(gpio_error_from_channel(error)),
            }
        }
    }
}

impl<const COMMAND_CAPACITY: usize, const STATUS_CAPACITY: usize>
    Rp2350FiberGpioOutputPin<COMMAND_CAPACITY, STATUS_CAPACITY>
{
    fn perform(&self, kind: Rp2350GpioCommandKind) -> Result<(), GpioError> {
        let request_id = self.client.next_request_id();
        RP2350_GPIO_CLIENT_REQUEST_ID.store(request_id, Ordering::Release);
        RP2350_GPIO_CLIENT_PHASE.store(1, Ordering::Release);
        let command = Rp2350GpioCommand {
            request_id,
            slot_index: self.slot_index,
            kind,
        };

        loop {
            match self
                .client
                .commands
                .try_send(self.client.command_producer, command)
            {
                Ok(()) => {
                    RP2350_GPIO_CLIENT_PHASE.store(2, Ordering::Release);
                    break;
                }
                Err(error) if error.kind() == ChannelErrorKind::Busy => {
                    RP2350_GPIO_CLIENT_PHASE.store(3, Ordering::Release);
                    wait_for_service_progress()?
                }
                Err(error) => return Err(gpio_error_from_channel(error)),
            }
        }

        loop {
            RP2350_GPIO_CLIENT_PHASE.store(4, Ordering::Release);
            match self.client.statuses.try_receive(self.client.status_consumer) {
                Ok(Some(Rp2350GpioStatus::Completed {
                    request_id: observed,
                })) if observed == request_id => {
                    RP2350_GPIO_CLIENT_PHASE.store(5, Ordering::Release);
                    return Ok(());
                }
                Ok(Some(Rp2350GpioStatus::Failed {
                    request_id: observed,
                    kind,
                })) if observed == request_id => return Err(gpio_error_from_kind(kind)),
                Ok(Some(_)) => return Err(GpioError::state_conflict()),
                Ok(None) => {
                    RP2350_GPIO_CLIENT_PHASE.store(6, Ordering::Release);
                    wait_for_service_progress()?
                }
                Err(error) if error.kind() == ChannelErrorKind::Busy => {
                    RP2350_GPIO_CLIENT_PHASE.store(7, Ordering::Release);
                    wait_for_service_progress()?
                }
                Err(error) => return Err(gpio_error_from_channel(error)),
            }
        }
    }

    /// Starts one batched GPIO transaction rooted in this pin's service.
    #[must_use]
    pub fn begin_batch(&self) -> Rp2350FiberGpioBatch<COMMAND_CAPACITY, STATUS_CAPACITY> {
        Rp2350FiberGpioBatch {
            client: self.client,
            ops: [Rp2350GpioBatchOp::Pause { spins: 0 }; RP2350_GPIO_BATCH_MAX_OPS],
            count: 0,
        }
    }
}

impl<const COMMAND_CAPACITY: usize, const STATUS_CAPACITY: usize>
    Rp2350FiberGpioBatch<COMMAND_CAPACITY, STATUS_CAPACITY>
{
    /// Clears the queued batch operations.
    pub fn clear(&mut self) {
        self.count = 0;
    }

    /// Returns the number of queued operations.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count as usize
    }

    /// Returns the fixed batch capacity.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        RP2350_GPIO_BATCH_MAX_OPS
    }

    /// Queues one output-level change for one pin belonging to this batch's GPIO service.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the pin belongs to a different service or the batch is
    /// already full.
    pub fn push_set_level(
        &mut self,
        pin: &Rp2350FiberGpioOutputPin<COMMAND_CAPACITY, STATUS_CAPACITY>,
        high: bool,
    ) -> Result<(), GpioError> {
        if !core::ptr::eq(self.client, pin.client) {
            return Err(GpioError::state_conflict());
        }
        let index = usize::from(self.count);
        if index >= RP2350_GPIO_BATCH_MAX_OPS {
            return Err(GpioError::resource_exhausted());
        }
        self.ops[index] = Rp2350GpioBatchOp::SetLevel {
            slot_index: pin.slot_index,
            high,
        };
        self.count = self.count.saturating_add(1);
        Ok(())
    }

    /// Queues one short pause expressed as a fixed number of spin-loop iterations.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the batch is already full.
    pub fn push_pause(&mut self, spins: u8) -> Result<(), GpioError> {
        let index = usize::from(self.count);
        if index >= RP2350_GPIO_BATCH_MAX_OPS {
            return Err(GpioError::resource_exhausted());
        }
        self.ops[index] = Rp2350GpioBatchOp::Pause { spins };
        self.count = self.count.saturating_add(1);
        Ok(())
    }

    /// Executes the queued batch as one request over the GPIO fiber service.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the GPIO service cannot accept or execute the batch.
    pub fn execute(&self) -> Result<(), GpioError> {
        let request_id = self.client.next_request_id();
        let command = Rp2350GpioCommand {
            request_id,
            slot_index: 0,
            kind: Rp2350GpioCommandKind::SetLevelsBatch {
                count: self.count,
                ops: self.ops,
            },
        };

        loop {
            match self
                .client
                .commands
                .try_send(self.client.command_producer, command)
            {
                Ok(()) => break,
                Err(error) if error.kind() == ChannelErrorKind::Busy => wait_for_service_progress()?,
                Err(error) => return Err(gpio_error_from_channel(error)),
            }
        }

        loop {
            match self.client.statuses.try_receive(self.client.status_consumer) {
                Ok(Some(Rp2350GpioStatus::Completed {
                    request_id: observed,
                })) if observed == request_id => return Ok(()),
                Ok(Some(Rp2350GpioStatus::Failed {
                    request_id: observed,
                    kind,
                })) if observed == request_id => return Err(gpio_error_from_kind(kind)),
                Ok(Some(_)) => return Err(GpioError::state_conflict()),
                Ok(None) => wait_for_service_progress()?,
                Err(error) if error.kind() == ChannelErrorKind::Busy => wait_for_service_progress()?,
                Err(error) => return Err(gpio_error_from_channel(error)),
            }
        }
    }
}

impl<const COMMAND_CAPACITY: usize, const STATUS_CAPACITY: usize> GpioOwnedPinContract
    for Rp2350FiberGpioOutputPin<COMMAND_CAPACITY, STATUS_CAPACITY>
{
    fn pin(&self) -> u8 {
        self.pin
    }

    fn capabilities(&self) -> GpioCapabilities {
        GpioCapabilities::OUTPUT
    }
}

impl<const COMMAND_CAPACITY: usize, const STATUS_CAPACITY: usize> GpioOutputPinContract
    for Rp2350FiberGpioOutputPin<COMMAND_CAPACITY, STATUS_CAPACITY>
{
    fn configure_output(&mut self, initial_high: bool) -> Result<(), GpioError> {
        self.perform(Rp2350GpioCommandKind::ConfigureOutput { initial_high })
    }

    fn set_level(&mut self, high: bool) -> Result<(), GpioError> {
        self.perform(Rp2350GpioCommandKind::SetLevel { high })
    }
}

fn run_gpio_service<
    const MAX_PINS: usize,
    const COMMAND_CAPACITY: usize,
    const STATUS_CAPACITY: usize,
>(
    service_addr: usize,
) -> ! {
    loop {
        let service_ptr =
            service_addr as *mut Rp2350FiberGpioService<MAX_PINS, COMMAND_CAPACITY, STATUS_CAPACITY>;
        // SAFETY: the service lives in static storage for the life of the example process.
        let service = unsafe { &mut *service_ptr };
        let _ = service.pump();
        let _ = yield_now();
    }
}

fn wait_for_service_progress() -> Result<(), GpioError> {
    if yield_now().is_ok() {
        return Ok(());
    }
    wait_for_runtime_progress();
    Ok(())
}

fn service_wait_for_client() -> Result<(), GpioError> {
    if yield_now().is_ok() {
        Ok(())
    } else {
        Err(GpioError::busy())
    }
}

const fn gpio_error_from_kind(kind: GpioErrorKind) -> GpioError {
    match kind {
        GpioErrorKind::Unsupported => GpioError::unsupported(),
        GpioErrorKind::Invalid => GpioError::invalid(),
        GpioErrorKind::Busy => GpioError::busy(),
        GpioErrorKind::ResourceExhausted => GpioError::resource_exhausted(),
        GpioErrorKind::StateConflict => GpioError::state_conflict(),
        GpioErrorKind::Platform(code) => GpioError::platform(code),
    }
}

const fn gpio_error_from_channel(error: ChannelError) -> GpioError {
    match error.kind() {
        ChannelErrorKind::Unsupported | ChannelErrorKind::ProtocolMismatch => GpioError::unsupported(),
        ChannelErrorKind::Invalid => GpioError::invalid(),
        ChannelErrorKind::Busy => GpioError::busy(),
        ChannelErrorKind::PermissionDenied | ChannelErrorKind::TransportDenied => {
            GpioError::state_conflict()
        }
        ChannelErrorKind::ResourceExhausted => GpioError::resource_exhausted(),
        ChannelErrorKind::StateConflict => GpioError::state_conflict(),
        ChannelErrorKind::Platform(code) => GpioError::platform(code),
    }
}

const fn gpio_error_from_transport(error: TransportError) -> GpioError {
    match error.kind() {
        TransportErrorKind::Unsupported => GpioError::unsupported(),
        TransportErrorKind::Invalid => GpioError::invalid(),
        TransportErrorKind::Busy => GpioError::busy(),
        TransportErrorKind::PermissionDenied | TransportErrorKind::NotAttached => {
            GpioError::state_conflict()
        }
        TransportErrorKind::ResourceExhausted => GpioError::resource_exhausted(),
        TransportErrorKind::StateConflict => GpioError::state_conflict(),
        TransportErrorKind::Platform(code) => GpioError::platform(code),
    }
}

const fn gpio_error_from_fiber(error: FiberError) -> GpioError {
    match error.kind() {
        FiberErrorKind::Unsupported => GpioError::unsupported(),
        FiberErrorKind::Invalid => GpioError::invalid(),
        FiberErrorKind::ResourceExhausted | FiberErrorKind::DeadlineExceeded => {
            GpioError::resource_exhausted()
        }
        FiberErrorKind::StateConflict | FiberErrorKind::Context(_) => GpioError::state_conflict(),
    }
}
