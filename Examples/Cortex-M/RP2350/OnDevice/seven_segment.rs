//! RP2350 example-side multiplexed seven-segment display service.
//!
//! This fiber owns the refresh cadence and one four-glyph framebuffer. One caller updates glyphs
//! or one hexadecimal value over a channel, and the service keeps the display alive by streaming
//! one digit frame at a time through the fiber-owned `74HC595` service.

use core::sync::atomic::{
    AtomicU32,
    Ordering,
};

use crate::runtime::{
    request_runtime_dispatch,
    spawn,
    wait_for_runtime_progress,
};
use fusion_hal::contract::drivers::bus::gpio::{
    GpioError,
    GpioErrorKind,
};
use fusion_hal::drivers::peripheral::{
    SevenSegmentGlyph,
    SevenSegmentPolarity,
};
use fusion_std::thread::{
    GreenHandle,
    yield_now,
};
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

use crate::shift_register_74hc595::{
    RP2350_SHIFT_REGISTER_FRAME_CYCLE_LEN,
    Rp2350FiberShiftRegister74hc595,
};

const REQUEST_ID_WRAP_SENTINEL: u32 = u32::MAX;
// Keep several full scans in one service turn, but stream them as one shift-register request so
// the display path does not pay one command/status round-trip per digit slice.
const DISPLAY_REFRESH_FRAMES_PER_TURN: u8 = 12;

#[unsafe(no_mangle)]
pub static RP2350_DISPLAY_SERVICE_HEARTBEAT: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static RP2350_DISPLAY_ACTIVE_DIGIT: AtomicU32 = AtomicU32::new(u32::MAX);
#[unsafe(no_mangle)]
pub static RP2350_DISPLAY_LAST_SEGMENTS: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static RP2350_DISPLAY_LAST_DIGITS: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone, Copy)]
enum Rp2350SevenSegmentCommandKind {
    SetGlyphs {
        glyphs: [SevenSegmentGlyph; 4],
    },
    SetHex {
        value: u16,
    },
    Clear,
}

#[derive(Debug, Clone, Copy)]
struct Rp2350SevenSegmentCommand {
    request_id: u32,
    kind: Rp2350SevenSegmentCommandKind,
}

#[derive(Debug, Clone, Copy)]
enum Rp2350SevenSegmentStatus {
    Completed {
        request_id: u32,
    },
    Failed {
        request_id: u32,
        kind: GpioErrorKind,
    },
}

struct Rp2350SevenSegmentCommandProtocol;

impl ProtocolContract for Rp2350SevenSegmentCommandProtocol {
    type Message = Rp2350SevenSegmentCommand;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x7f0d_1c0f_4b7b_4e7d_8e11_0000_0000_0201),
        version: ProtocolVersion::new(1, 0, 0),
        caps: ProtocolCaps::empty(),
        bootstrap: ProtocolBootstrapKind::Immediate,
        debug_view: ProtocolDebugView::Structured,
        transport: ProtocolTransportRequirements::message_local(),
        implementation: ProtocolImplementationKind::Native,
    };
}

struct Rp2350SevenSegmentStatusProtocol;

impl ProtocolContract for Rp2350SevenSegmentStatusProtocol {
    type Message = Rp2350SevenSegmentStatus;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x7f0d_1c0f_4b7b_4e7d_8e11_0000_0000_0202),
        version: ProtocolVersion::new(1, 0, 0),
        caps: ProtocolCaps::empty(),
        bootstrap: ProtocolBootstrapKind::Immediate,
        debug_view: ProtocolDebugView::Structured,
        transport: ProtocolTransportRequirements::message_local(),
        implementation: ProtocolImplementationKind::Native,
    };
}

struct Rp2350SevenSegmentClientIo<const COMMAND_CAPACITY: usize, const STATUS_CAPACITY: usize> {
    commands: LocalChannel<Rp2350SevenSegmentCommandProtocol, COMMAND_CAPACITY, 1>,
    statuses: LocalChannel<Rp2350SevenSegmentStatusProtocol, STATUS_CAPACITY, 1>,
    command_producer: usize,
    status_consumer: usize,
    next_request_id: AtomicU32,
}

impl<const COMMAND_CAPACITY: usize, const STATUS_CAPACITY: usize>
    Rp2350SevenSegmentClientIo<COMMAND_CAPACITY, STATUS_CAPACITY>
{
    fn new() -> Result<(Self, usize, usize), GpioError> {
        let request = TransportAttachmentRequest::same_courier()
            .with_requested_law(TransportAttachmentLaw::ExclusiveSpsc);
        let commands =
            LocalChannel::<Rp2350SevenSegmentCommandProtocol, COMMAND_CAPACITY, 1>::new_with_attachment_law(
                TransportAttachmentLaw::ExclusiveSpsc,
            )
            .map_err(gpio_error_from_channel)?;
        let statuses =
            LocalChannel::<Rp2350SevenSegmentStatusProtocol, STATUS_CAPACITY, 1>::new_with_attachment_law(
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

/// One channel-backed seven-segment display handle.
#[derive(Clone, Copy)]
pub struct Rp2350FiberFourDigitSevenSegmentDisplay<
    const COMMAND_CAPACITY: usize = 8,
    const STATUS_CAPACITY: usize = 8,
> {
    client: &'static Rp2350SevenSegmentClientIo<COMMAND_CAPACITY, STATUS_CAPACITY>,
}

/// One fiber-owned four-digit seven-segment refresh service.
pub struct Rp2350FiberFourDigitSevenSegmentDisplayService<
    const COMMAND_CAPACITY: usize = 8,
    const STATUS_CAPACITY: usize = 8,
> {
    client: Rp2350SevenSegmentClientIo<COMMAND_CAPACITY, STATUS_CAPACITY>,
    command_consumer: usize,
    status_producer: usize,
    register: Rp2350FiberShiftRegister74hc595<2>,
    polarity: SevenSegmentPolarity,
    glyphs: [SevenSegmentGlyph; 4],
    spawned: bool,
    service_handle: Option<GreenHandle<()>>,
}

impl<const COMMAND_CAPACITY: usize, const STATUS_CAPACITY: usize>
    Rp2350FiberFourDigitSevenSegmentDisplayService<COMMAND_CAPACITY, STATUS_CAPACITY>
{
    /// Creates one new seven-segment refresh service.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the command lanes cannot be created.
    pub fn new(
        register: Rp2350FiberShiftRegister74hc595<2>,
        polarity: SevenSegmentPolarity,
    ) -> Result<Self, GpioError> {
        let (client, command_consumer, status_producer) = Rp2350SevenSegmentClientIo::new()?;
        Ok(Self {
            client,
            command_consumer,
            status_producer,
            register,
            polarity,
            glyphs: [SevenSegmentGlyph::BLANK; 4],
            spawned: false,
            service_handle: None,
        })
    }

    /// Returns one channel-backed display handle.
    #[must_use]
    pub fn client_handle(
        &'static self,
    ) -> Rp2350FiberFourDigitSevenSegmentDisplay<COMMAND_CAPACITY, STATUS_CAPACITY> {
        Rp2350FiberFourDigitSevenSegmentDisplay {
            client: &self.client,
        }
    }

    /// Spawns the refresh-owning display service fiber.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the service fiber cannot be admitted.
    pub fn spawn(self: core::pin::Pin<&'static mut Self>) -> Result<(), GpioError> {
        // SAFETY: this service is pinned in static storage by the examples and never moved again.
        let this = unsafe { self.get_unchecked_mut() };
        if this.spawned {
            return Err(GpioError::state_conflict());
        }
        this.spawned = true;

        let service_addr = this as *mut Self as usize;
        let handle = spawn(move || {
            run_seven_segment_service::<COMMAND_CAPACITY, STATUS_CAPACITY>(service_addr)
        })
        .map_err(gpio_error_from_fiber)?;
        this.service_handle = Some(handle);
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
                Ok(()) => Rp2350SevenSegmentStatus::Completed {
                    request_id: command.request_id,
                },
                Err(error) => Rp2350SevenSegmentStatus::Failed {
                    request_id: command.request_id,
                    kind: error.kind(),
                },
            };
            self.send_status(status)?;
        }

        Ok(())
    }

    fn handle_command(&mut self, command: Rp2350SevenSegmentCommand) -> Result<(), GpioError> {
        match command.kind {
            Rp2350SevenSegmentCommandKind::SetGlyphs { glyphs } => {
                self.glyphs = glyphs;
                Ok(())
            }
            Rp2350SevenSegmentCommandKind::SetHex { value } => {
                self.glyphs = glyphs_from_hex(value);
                Ok(())
            }
            Rp2350SevenSegmentCommandKind::Clear => {
                self.glyphs = [SevenSegmentGlyph::BLANK; 4];
                Ok(())
            }
        }
    }

    fn refresh_turn(&mut self) -> Result<(), GpioError> {
        let mut frames = [[0_u8; 2]; RP2350_SHIFT_REGISTER_FRAME_CYCLE_LEN];
        for (index, frame) in frames.iter_mut().enumerate() {
            *frame = [
                digit_output_byte(Some(index), self.polarity),
                segment_output_byte(self.glyphs[index], self.polarity),
            ];
        }
        RP2350_DISPLAY_SERVICE_HEARTBEAT.fetch_add(1, Ordering::AcqRel);
        RP2350_DISPLAY_ACTIVE_DIGIT.store(3, Ordering::Release);
        RP2350_DISPLAY_LAST_DIGITS
            .store(u32::from(frames[RP2350_SHIFT_REGISTER_FRAME_CYCLE_LEN - 1][0]), Ordering::Release);
        RP2350_DISPLAY_LAST_SEGMENTS
            .store(u32::from(frames[RP2350_SHIFT_REGISTER_FRAME_CYCLE_LEN - 1][1]), Ordering::Release);
        self.register
            .write_frame_cycle(frames, DISPLAY_REFRESH_FRAMES_PER_TURN)?;
        Ok(())
    }

    fn send_status(&self, status: Rp2350SevenSegmentStatus) -> Result<(), GpioError> {
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

impl<const COMMAND_CAPACITY: usize, const STATUS_CAPACITY: usize>
    Rp2350FiberFourDigitSevenSegmentDisplay<COMMAND_CAPACITY, STATUS_CAPACITY>
{
    /// Sets the full display from one hexadecimal value.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the display service rejects the request.
    pub fn set_hex(&self, value: u16) -> Result<(), GpioError> {
        self.perform(Rp2350SevenSegmentCommandKind::SetHex { value })
    }

    /// Replaces the full four-glyph framebuffer.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the display service rejects the request.
    pub fn set_glyphs(&self, glyphs: [SevenSegmentGlyph; 4]) -> Result<(), GpioError> {
        self.perform(Rp2350SevenSegmentCommandKind::SetGlyphs { glyphs })
    }

    /// Clears the display framebuffer.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO error when the display service rejects the request.
    pub fn clear(&self) -> Result<(), GpioError> {
        self.perform(Rp2350SevenSegmentCommandKind::Clear)
    }

    fn perform(&self, kind: Rp2350SevenSegmentCommandKind) -> Result<(), GpioError> {
        let request_id = self.client.next_request_id();
        let command = Rp2350SevenSegmentCommand { request_id, kind };

        loop {
            match self
                .client
                .commands
                .try_send(self.client.command_producer, command)
            {
                Ok(()) => {
                    request_runtime_dispatch();
                    break;
                }
                Err(error) if error.kind() == ChannelErrorKind::Busy => {
                    wait_for_service_progress()?
                }
                Err(error) => return Err(gpio_error_from_channel(error)),
            }
        }

        loop {
            match self.client.statuses.try_receive(self.client.status_consumer) {
                Ok(Some(Rp2350SevenSegmentStatus::Completed {
                    request_id: observed,
                })) if observed == request_id => return Ok(()),
                Ok(Some(Rp2350SevenSegmentStatus::Failed {
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

fn run_seven_segment_service<const COMMAND_CAPACITY: usize, const STATUS_CAPACITY: usize>(
    service_addr: usize,
) -> ! {
    loop {
        let service_ptr = service_addr
            as *mut Rp2350FiberFourDigitSevenSegmentDisplayService<COMMAND_CAPACITY, STATUS_CAPACITY>;
        // SAFETY: the service lives in static storage for the life of the example process.
        let service = unsafe { &mut *service_ptr };
        let _ = service.pump();
        let _ = service.refresh_turn();
        let _ = yield_now();
    }
}

const fn glyphs_from_hex(value: u16) -> [SevenSegmentGlyph; 4] {
    [
        match SevenSegmentGlyph::from_hex(((value >> 12) & 0x0f) as u8) {
            Some(glyph) => glyph,
            None => SevenSegmentGlyph::BLANK,
        },
        match SevenSegmentGlyph::from_hex(((value >> 8) & 0x0f) as u8) {
            Some(glyph) => glyph,
            None => SevenSegmentGlyph::BLANK,
        },
        match SevenSegmentGlyph::from_hex(((value >> 4) & 0x0f) as u8) {
            Some(glyph) => glyph,
            None => SevenSegmentGlyph::BLANK,
        },
        match SevenSegmentGlyph::from_hex((value & 0x0f) as u8) {
            Some(glyph) => glyph,
            None => SevenSegmentGlyph::BLANK,
        },
    ]
}

const fn output_level(asserted: bool, active_high: bool) -> bool {
    if active_high { asserted } else { !asserted }
}

const fn segment_output_byte(glyph: SevenSegmentGlyph, polarity: SevenSegmentPolarity) -> u8 {
    if polarity.segment_active_high {
        glyph.raw()
    } else {
        !glyph.raw()
    }
}

const fn digit_output_byte(active: Option<usize>, polarity: SevenSegmentPolarity) -> u8 {
    let inactive = output_level(false, polarity.digit_active_high);
    let active_level = output_level(true, polarity.digit_active_high);
    let base = if inactive { 0xff } else { 0x00 };
    match active {
        Some(index) if index < 4 => {
            let bit = 1u8 << index;
            if active_level {
                (base & !bit) | bit
            } else {
                base & !bit
            }
        }
        _ => base,
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
