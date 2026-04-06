//! RP2350 example-side 74HC595 composition over the fiber-backed GPIO service.
//!
//! This service owns one logical `74HC595` chain and turns one whole latched frame into one
//! batched GPIO request. That keeps the example stack honest:
//! - GPIO still lives behind the firmware-selected driver and the example GPIO fiber
//! - the hot display path stops paying one channel round-trip per pin toggle
//! - upper layers can treat the shift chain like one framed peripheral again

use core::sync::atomic::{
    AtomicU32,
    Ordering,
};

use fusion_hal::contract::drivers::bus::gpio::GpioOutputPinContract;
use crate::runtime::{
    spawn_with_stack,
    wait_for_runtime_progress,
};
use fusion_hal::contract::drivers::bus::gpio::{
    GpioError,
    GpioErrorKind,
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
use fusion_sys::transport::{
    TransportAttachmentControlContract,
    TransportAttachmentLaw,
    TransportAttachmentRequest,
    TransportError,
    TransportErrorKind,
};

use crate::gpio::Rp2350FiberGpioOutputPin;

type ExampleGpioPin = Rp2350FiberGpioOutputPin<16, 16>;

const REQUEST_ID_WRAP_SENTINEL: u32 = u32::MAX;
const SHIFT_REGISTER_DATA_SETUP_SPINS: u8 = 16;
const SHIFT_REGISTER_LATCH_HOLD_SPINS: u8 = 16;
pub const RP2350_SHIFT_REGISTER_FRAME_CYCLE_LEN: usize = 4;

#[unsafe(no_mangle)]
pub static RP2350_SHIFT_SERVICE_HEARTBEAT: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static RP2350_SHIFT_LAST_FRAME_LOW: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static RP2350_SHIFT_LAST_FRAME_HIGH: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static RP2350_SHIFT_INIT_PHASE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static RP2350_SHIFT_WRITE_PROGRESS: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone, Copy)]
enum Rp2350ShiftRegisterCommandKind<const FRAME_BYTES: usize> {
    WriteFrame {
        bytes: [u8; FRAME_BYTES],
    },
    WriteFrameCycle {
        frames: [[u8; FRAME_BYTES]; RP2350_SHIFT_REGISTER_FRAME_CYCLE_LEN],
        repeat_count: u8,
    },
    SetOutputsEnabled {
        enabled: bool,
    },
}

#[derive(Debug, Clone, Copy)]
struct Rp2350ShiftRegisterCommand<const FRAME_BYTES: usize> {
    request_id: u32,
    kind: Rp2350ShiftRegisterCommandKind<FRAME_BYTES>,
}

#[derive(Debug, Clone, Copy)]
enum Rp2350ShiftRegisterStatus {
    Completed {
        request_id: u32,
    },
    Failed {
        request_id: u32,
        kind: GpioErrorKind,
    },
}

struct Rp2350ShiftRegisterCommandProtocol<const FRAME_BYTES: usize>;

impl<const FRAME_BYTES: usize> ProtocolContract
    for Rp2350ShiftRegisterCommandProtocol<FRAME_BYTES>
{
    type Message = Rp2350ShiftRegisterCommand<FRAME_BYTES>;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x7f0d_1c0f_4b7b_4e7d_8e11_0000_0000_0101),
        version: ProtocolVersion::new(1, 0, 0),
        caps: ProtocolCaps::empty(),
        bootstrap: ProtocolBootstrapKind::Immediate,
        debug_view: ProtocolDebugView::Structured,
        transport: ProtocolTransportRequirements::message_local(),
        implementation: ProtocolImplementationKind::Native,
    };
}

struct Rp2350ShiftRegisterStatusProtocol;

impl ProtocolContract for Rp2350ShiftRegisterStatusProtocol {
    type Message = Rp2350ShiftRegisterStatus;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x7f0d_1c0f_4b7b_4e7d_8e11_0000_0000_0102),
        version: ProtocolVersion::new(1, 0, 0),
        caps: ProtocolCaps::empty(),
        bootstrap: ProtocolBootstrapKind::Immediate,
        debug_view: ProtocolDebugView::Structured,
        transport: ProtocolTransportRequirements::message_local(),
        implementation: ProtocolImplementationKind::Native,
    };
}

struct Rp2350ShiftRegisterClientIo<
    const FRAME_BYTES: usize,
    const COMMAND_CAPACITY: usize,
    const STATUS_CAPACITY: usize,
> {
    commands: LocalChannel<Rp2350ShiftRegisterCommandProtocol<FRAME_BYTES>, COMMAND_CAPACITY, 1>,
    statuses: LocalChannel<Rp2350ShiftRegisterStatusProtocol, STATUS_CAPACITY, 1>,
    command_producer: usize,
    status_consumer: usize,
    next_request_id: AtomicU32,
}

impl<const FRAME_BYTES: usize, const COMMAND_CAPACITY: usize, const STATUS_CAPACITY: usize>
    Rp2350ShiftRegisterClientIo<FRAME_BYTES, COMMAND_CAPACITY, STATUS_CAPACITY>
{
    fn new() -> Result<(Self, usize, usize), GpioError> {
        let request = TransportAttachmentRequest::same_courier()
            .with_requested_law(TransportAttachmentLaw::ExclusiveSpsc);
        let commands = LocalChannel::<
            Rp2350ShiftRegisterCommandProtocol<FRAME_BYTES>,
            COMMAND_CAPACITY,
            1,
        >::new_with_attachment_law(TransportAttachmentLaw::ExclusiveSpsc)
        .map_err(gpio_error_from_channel)?;
        let statuses =
            LocalChannel::<Rp2350ShiftRegisterStatusProtocol, STATUS_CAPACITY, 1>::new_with_attachment_law(
                TransportAttachmentLaw::ExclusiveSpsc,
            )
            .map_err(gpio_error_from_channel)?;
        let command_producer = commands
            .attach_producer(request)
            .map_err(gpio_error_from_transport)?;
        let command_consumer = commands
            .attach_consumer(request)
            .map_err(gpio_error_from_transport)?;
        let status_producer = statuses
            .attach_producer(request)
            .map_err(gpio_error_from_transport)?;
        let status_consumer = statuses
            .attach_consumer(request)
            .map_err(gpio_error_from_transport)?;

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
        let next = self
            .next_request_id
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        if next == 0 || next == REQUEST_ID_WRAP_SENTINEL {
            self.next_request_id.store(1, Ordering::Release);
            1
        } else {
            next
        }
    }
}

/// One channel-backed 74HC595 client handle.
#[derive(Clone, Copy)]
pub struct Rp2350FiberShiftRegister74hc595<
    const FRAME_BYTES: usize,
    const COMMAND_CAPACITY: usize = 8,
    const STATUS_CAPACITY: usize = 8,
> {
    client: &'static Rp2350ShiftRegisterClientIo<FRAME_BYTES, COMMAND_CAPACITY, STATUS_CAPACITY>,
}

/// One fiber-owned 74HC595 service.
pub struct Rp2350FiberShiftRegister74hc595Service<
    const FRAME_BYTES: usize,
    const COMMAND_CAPACITY: usize = 8,
    const STATUS_CAPACITY: usize = 8,
> {
    client: Rp2350ShiftRegisterClientIo<FRAME_BYTES, COMMAND_CAPACITY, STATUS_CAPACITY>,
    command_consumer: usize,
    status_producer: usize,
    data: ExampleGpioPin,
    shift_clock: ExampleGpioPin,
    latch_clock: ExampleGpioPin,
    output_enable: ExampleGpioPin,
    spawned: bool,
}

impl<const FRAME_BYTES: usize, const COMMAND_CAPACITY: usize, const STATUS_CAPACITY: usize>
    Rp2350FiberShiftRegister74hc595Service<FRAME_BYTES, COMMAND_CAPACITY, STATUS_CAPACITY>
{
    /// Creates one new fiber-owned 74HC595 service over the example GPIO lane.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the command lanes cannot be created or the chain
    /// cannot be initialized.
    pub fn new(
        mut data: ExampleGpioPin,
        mut shift_clock: ExampleGpioPin,
        mut latch_clock: ExampleGpioPin,
        mut output_enable: ExampleGpioPin,
    ) -> Result<Self, GpioError> {
        RP2350_SHIFT_INIT_PHASE.store(1, Ordering::Release);
        if FRAME_BYTES == 0 {
            return Err(GpioError::invalid());
        }
        let (client, command_consumer, status_producer) = Rp2350ShiftRegisterClientIo::new()?;

        RP2350_SHIFT_INIT_PHASE.store(2, Ordering::Release);
        data.configure_output(false)?;
        RP2350_SHIFT_INIT_PHASE.store(3, Ordering::Release);
        shift_clock.configure_output(false)?;
        RP2350_SHIFT_INIT_PHASE.store(4, Ordering::Release);
        latch_clock.configure_output(false)?;
        RP2350_SHIFT_INIT_PHASE.store(5, Ordering::Release);
        output_enable.configure_output(true)?;
        RP2350_SHIFT_INIT_PHASE.store(6, Ordering::Release);

        let mut service = Self {
            client,
            command_consumer,
            status_producer,
            data,
            shift_clock,
            latch_clock,
            output_enable,
            spawned: false,
        };
        RP2350_SHIFT_INIT_PHASE.store(7, Ordering::Release);
        service.write_frame_internal(&[0; FRAME_BYTES])?;
        RP2350_SHIFT_INIT_PHASE.store(8, Ordering::Release);
        service.set_outputs_enabled_internal(true)?;
        RP2350_SHIFT_INIT_PHASE.store(9, Ordering::Release);
        Ok(service)
    }

    /// Returns one channel-backed client handle.
    #[must_use]
    pub fn client_handle(
        &'static self,
    ) -> Rp2350FiberShiftRegister74hc595<FRAME_BYTES, COMMAND_CAPACITY, STATUS_CAPACITY> {
        Rp2350FiberShiftRegister74hc595 {
            client: &self.client,
        }
    }

    /// Spawns the hardware-owning shift-register service fiber.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the service fiber cannot be admitted.
    pub fn spawn<const STACK_BYTES: usize>(
        self: core::pin::Pin<&'static mut Self>,
    ) -> Result<(), GpioError> {
        // SAFETY: this service is pinned in static storage by the examples and never moved again.
        let this = unsafe { self.get_unchecked_mut() };
        if this.spawned {
            return Err(GpioError::state_conflict());
        }
        this.spawned = true;

        let service_addr = this as *mut Self as usize;
        spawn_with_stack::<STACK_BYTES, _, _>(move || {
            run_shift_register_service::<FRAME_BYTES, COMMAND_CAPACITY, STATUS_CAPACITY>(service_addr)
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
            let status = match self.handle_command(command) {
                Ok(()) => Rp2350ShiftRegisterStatus::Completed {
                    request_id: command.request_id,
                },
                Err(error) => Rp2350ShiftRegisterStatus::Failed {
                    request_id: command.request_id,
                    kind: error.kind(),
                },
            };
            self.send_status(status)?;
        }

        Ok(())
    }

    fn handle_command(
        &mut self,
        command: Rp2350ShiftRegisterCommand<FRAME_BYTES>,
    ) -> Result<(), GpioError> {
        match command.kind {
            Rp2350ShiftRegisterCommandKind::WriteFrame { bytes } => self.write_frame_internal(&bytes),
            Rp2350ShiftRegisterCommandKind::WriteFrameCycle {
                frames,
                repeat_count,
            } => self.write_frame_cycle_internal(&frames, repeat_count),
            Rp2350ShiftRegisterCommandKind::SetOutputsEnabled { enabled } => {
                self.set_outputs_enabled_internal(enabled)
            }
        }
    }

    fn write_frame_internal(&mut self, bytes: &[u8; FRAME_BYTES]) -> Result<(), GpioError> {
        RP2350_SHIFT_SERVICE_HEARTBEAT.fetch_add(1, Ordering::AcqRel);
        RP2350_SHIFT_WRITE_PROGRESS.store(0, Ordering::Release);
        if FRAME_BYTES > 0 {
            RP2350_SHIFT_LAST_FRAME_LOW.store(u32::from(bytes[0]), Ordering::Release);
        }
        if FRAME_BYTES > 1 {
            RP2350_SHIFT_LAST_FRAME_HIGH.store(u32::from(bytes[1]), Ordering::Release);
        }
        let mut batch = self.data.begin_batch();
        for (byte_index, &byte) in bytes.iter().enumerate() {
            for bit in (0..8).rev() {
                let progress = ((byte_index as u32) << 8) | (u32::from((7 - bit) as u8) << 4);
                RP2350_SHIFT_WRITE_PROGRESS.store(progress | 1, Ordering::Release);
                batch.push_set_level(&self.data, ((byte >> bit) & 1) != 0)?;
                RP2350_SHIFT_WRITE_PROGRESS.store(progress | 2, Ordering::Release);
                batch.push_pause(SHIFT_REGISTER_DATA_SETUP_SPINS)?;
                batch.push_set_level(&self.shift_clock, true)?;
                RP2350_SHIFT_WRITE_PROGRESS.store(progress | 3, Ordering::Release);
                batch.push_pause(SHIFT_REGISTER_DATA_SETUP_SPINS)?;
                batch.push_set_level(&self.shift_clock, false)?;
                RP2350_SHIFT_WRITE_PROGRESS.store(progress | 4, Ordering::Release);
            }
        }
        RP2350_SHIFT_WRITE_PROGRESS.store(0x1000, Ordering::Release);
        batch.push_set_level(&self.latch_clock, true)?;
        RP2350_SHIFT_WRITE_PROGRESS.store(0x1001, Ordering::Release);
        batch.push_pause(SHIFT_REGISTER_LATCH_HOLD_SPINS)?;
        batch.push_set_level(&self.latch_clock, false)?;
        batch.execute()?;
        RP2350_SHIFT_WRITE_PROGRESS.store(0x1002, Ordering::Release);
        Ok(())
    }

    fn set_outputs_enabled_internal(&mut self, enabled: bool) -> Result<(), GpioError> {
        self.output_enable.set_level(!enabled)
    }

    fn write_frame_cycle_internal(
        &mut self,
        frames: &[[u8; FRAME_BYTES]; RP2350_SHIFT_REGISTER_FRAME_CYCLE_LEN],
        repeat_count: u8,
    ) -> Result<(), GpioError> {
        let repeats = usize::from(repeat_count.max(1));
        for _ in 0..repeats {
            for frame in frames {
                self.write_frame_internal(frame)?;
            }
        }
        Ok(())
    }

    fn send_status(&self, status: Rp2350ShiftRegisterStatus) -> Result<(), GpioError> {
        loop {
            match self.client.statuses.try_send(self.status_producer, status) {
                Ok(()) => return Ok(()),
                Err(error) if error.kind() == ChannelErrorKind::Busy => {
                    service_wait_for_client()?
                }
                Err(error) => return Err(gpio_error_from_channel(error)),
            }
        }
    }
}

impl<const FRAME_BYTES: usize, const COMMAND_CAPACITY: usize, const STATUS_CAPACITY: usize>
    Rp2350FiberShiftRegister74hc595<FRAME_BYTES, COMMAND_CAPACITY, STATUS_CAPACITY>
{
    /// Enables or disables the output stage.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the shift-register service rejects the request.
    pub fn set_outputs_enabled(&self, enabled: bool) -> Result<(), GpioError> {
        self.perform(Rp2350ShiftRegisterCommandKind::SetOutputsEnabled { enabled })
    }

    /// Writes one full latched frame in daisy-chain order.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the shift-register service rejects the request.
    pub fn write_frame(&self, bytes: [u8; FRAME_BYTES]) -> Result<(), GpioError> {
        self.perform(Rp2350ShiftRegisterCommandKind::WriteFrame { bytes })
    }

    /// Writes one fixed four-frame scan cycle and repeats it on the service side.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the shift-register service rejects the request.
    pub fn write_frame_cycle(
        &self,
        frames: [[u8; FRAME_BYTES]; RP2350_SHIFT_REGISTER_FRAME_CYCLE_LEN],
        repeat_count: u8,
    ) -> Result<(), GpioError> {
        self.perform(Rp2350ShiftRegisterCommandKind::WriteFrameCycle {
            frames,
            repeat_count,
        })
    }

    fn perform(&self, kind: Rp2350ShiftRegisterCommandKind<FRAME_BYTES>) -> Result<(), GpioError> {
        let request_id = self.client.next_request_id();
        let command = Rp2350ShiftRegisterCommand { request_id, kind };

        loop {
            match self
                .client
                .commands
                .try_send(self.client.command_producer, command)
            {
                Ok(()) => break,
                Err(error) if error.kind() == ChannelErrorKind::Busy => {
                    wait_for_service_progress()?
                }
                Err(error) => return Err(gpio_error_from_channel(error)),
            }
        }

        loop {
            match self.client.statuses.try_receive(self.client.status_consumer) {
                Ok(Some(Rp2350ShiftRegisterStatus::Completed {
                    request_id: observed,
                })) if observed == request_id => return Ok(()),
                Ok(Some(Rp2350ShiftRegisterStatus::Failed {
                    request_id: observed,
                    kind,
                })) if observed == request_id => return Err(gpio_error_from_kind(kind)),
                Ok(Some(_)) => return Err(GpioError::state_conflict()),
                Ok(None) => wait_for_service_progress()?,
                Err(error) if error.kind() == ChannelErrorKind::Busy => {
                    wait_for_service_progress()?
                }
                Err(error) => return Err(gpio_error_from_channel(error)),
            }
        }
    }
}

fn run_shift_register_service<
    const FRAME_BYTES: usize,
    const COMMAND_CAPACITY: usize,
    const STATUS_CAPACITY: usize,
>(
    service_addr: usize,
) -> ! {
    loop {
        let service_ptr = service_addr as *mut Rp2350FiberShiftRegister74hc595Service<
            FRAME_BYTES,
            COMMAND_CAPACITY,
            STATUS_CAPACITY,
        >;
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
