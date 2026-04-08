//! RP2350 example-side retained four-digit seven-segment display driven by one timer alarm.
//!
//! Callers update one retained framebuffer and the timer IRQ replays the current scan forever.
//! That gives the examples immediate-mode semantics while the lower layer keeps the multiplexed
//! hardware alive without one dedicated refresh fiber.

use core::cell::UnsafeCell;
use core::hint::spin_loop;
use core::mem::MaybeUninit;
use core::num::NonZeroUsize;
use core::ptr;
use core::ptr::NonNull;
use core::sync::atomic::{
    AtomicU16,
    AtomicU32,
    AtomicU8,
    Ordering,
};

use fusion_firmware::sys::hal::drivers::bus::gpio::{
    SystemGpioPin,
    system_gpio,
};
use fusion_hal::contract::drivers::bus::gpio::{
    GpioControlContract,
    GpioDriveStrength,
    GpioError,
};
use fusion_hal::drivers::peripheral::{
    SevenSegmentGlyph,
    SevenSegmentPolarity,
    ShiftRegister74hc595,
};
use fusion_pal::contract::pal::{
    HardwareError,
    HardwareErrorKind,
};
use fusion_pal::sys::soc::cortex_m::hal::soc::board as cortex_m_soc_board;
use fusion_pal::sys::vector::{
    VectorInlineReservedStack,
    VectorInlineStackPolicy,
};
use fusion_std::thread::{
    RedInterrupt,
    RedInterruptConfig,
};
use fusion_sys::thread::ThreadErrorKind;

const DISPLAY_TIMER_IRQN: u16 = 2;
const DISPLAY_TIMER0_BASE: usize = 0x400b_0000;
const DISPLAY_TIMER_ALARM_INDEX: usize = 2;
const DISPLAY_TIMER_ALARM0_OFFSET: usize = 0x10;
const DISPLAY_TIMER_INTE_OFFSET: usize = 0x40;
const DISPLAY_SLICE_PERIOD_MICROS: u32 = 500;
const DISPLAY_FRAME_BANKS: usize = 2;
const DISPLAY_DIGITS: usize = 4;
const DISPLAY_INTERRUPT_STACK_BYTES: usize = 1024;
const DISPLAY_STATE_UNINITIALIZED: u8 = 0;
const DISPLAY_STATE_INITIALIZING: u8 = 1;
const DISPLAY_STATE_READY: u8 = 2;
const DISPLAY_BLANK_FRAME: u16 = u16::from_le_bytes([0xff, 0x00]);

type DisplayRegister =
    ShiftRegister74hc595<SystemGpioPin, SystemGpioPin, SystemGpioPin, SystemGpioPin>;

struct DisplayHardware {
    register: DisplayRegister,
    interrupt: RedInterrupt,
}

struct DisplayController {
    state: AtomicU8,
    active_bank: AtomicU8,
    next_digit: AtomicU8,
    frames: [[AtomicU16; DISPLAY_DIGITS]; DISPLAY_FRAME_BANKS],
    hardware: UnsafeCell<MaybeUninit<DisplayHardware>>,
}

impl DisplayController {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(DISPLAY_STATE_UNINITIALIZED),
            active_bank: AtomicU8::new(0),
            next_digit: AtomicU8::new(0),
            frames: [
                [const { AtomicU16::new(DISPLAY_BLANK_FRAME) }; DISPLAY_DIGITS],
                [const { AtomicU16::new(DISPLAY_BLANK_FRAME) }; DISPLAY_DIGITS],
            ],
            hardware: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    fn ensure_initialized(
        &self,
        data_pin: u8,
        output_enable_pin: u8,
        latch_pin: u8,
        shift_clock_pin: u8,
    ) -> Result<(), GpioError> {
        loop {
            match self.state.load(Ordering::Acquire) {
                DISPLAY_STATE_READY => return Ok(()),
                DISPLAY_STATE_INITIALIZING => spin_loop(),
                DISPLAY_STATE_UNINITIALIZED => {
                    if self
                        .state
                        .compare_exchange(
                            DISPLAY_STATE_UNINITIALIZED,
                            DISPLAY_STATE_INITIALIZING,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_err()
                    {
                        continue;
                    }

                    let result = self.initialize_hardware(
                        data_pin,
                        output_enable_pin,
                        latch_pin,
                        shift_clock_pin,
                    );
                    match result {
                        Ok(hardware) => {
                            unsafe { (*self.hardware.get()).write(hardware) };
                            self.stage_blank_banks();
                            self.active_bank.store(0, Ordering::Release);
                            self.next_digit.store(0, Ordering::Release);
                            self.state.store(DISPLAY_STATE_READY, Ordering::Release);
                            self.start_refresh()?;
                            RP2350_TIMER_DISPLAY_INIT_PHASE.store(6, Ordering::Release);
                            return Ok(());
                        }
                        Err(error) => {
                            self.state
                                .store(DISPLAY_STATE_UNINITIALIZED, Ordering::Release);
                            RP2350_TIMER_DISPLAY_INIT_PHASE.store(0xff, Ordering::Release);
                            return Err(error);
                        }
                    }
                }
                _ => return Err(GpioError::state_conflict()),
            }
        }
    }

    fn stage_blank_banks(&self) {
        for bank in &self.frames {
            for frame in bank {
                frame.store(DISPLAY_BLANK_FRAME, Ordering::Release);
            }
        }
    }

    fn start_refresh(&self) -> Result<(), GpioError> {
        let hardware = unsafe { (*self.hardware.get()).assume_init_mut() };
        arm_next_display_alarm()?;
        hardware.interrupt.enable().map_err(gpio_error_from_thread)?;
        Ok(())
    }

    fn initialize_hardware(
        &self,
        data_pin: u8,
        output_enable_pin: u8,
        latch_pin: u8,
        shift_clock_pin: u8,
    ) -> Result<DisplayHardware, GpioError> {
        RP2350_TIMER_DISPLAY_INIT_PHASE.store(1, Ordering::Release);
        let gpio = system_gpio()?;
        let mut data = gpio.take_pin(data_pin)?;
        let mut output_enable = gpio.take_pin(output_enable_pin)?;
        let mut latch = gpio.take_pin(latch_pin)?;
        let mut shift_clock = gpio.take_pin(shift_clock_pin)?;

        data.set_drive_strength(GpioDriveStrength::MilliAmps4)?;
        output_enable.set_drive_strength(GpioDriveStrength::MilliAmps4)?;
        latch.set_drive_strength(GpioDriveStrength::MilliAmps4)?;
        shift_clock.set_drive_strength(GpioDriveStrength::MilliAmps4)?;
        RP2350_TIMER_DISPLAY_INIT_PHASE.store(2, Ordering::Release);

        let register =
            ShiftRegister74hc595::with_output_enable(data, shift_clock, latch, output_enable)?;
        RP2350_TIMER_DISPLAY_INIT_PHASE.store(3, Ordering::Release);

        let interrupt = RedInterrupt::bind_runtime_owned(
            &RedInterruptConfig::new(DISPLAY_TIMER_IRQN)
                .with_priority(0x80)
                .with_stack(display_interrupt_stack_policy())
                .with_enable_on_bind(false),
            timer_display_alarm_irq,
        )
        .map_err(gpio_error_from_thread)?;
        RP2350_TIMER_DISPLAY_INIT_PHASE.store(4, Ordering::Release);
        RP2350_TIMER_DISPLAY_INIT_PHASE.store(5, Ordering::Release);

        Ok(DisplayHardware {
            register,
            interrupt,
        })
    }

    fn stage_glyphs(
        &self,
        polarity: SevenSegmentPolarity,
        glyphs: [SevenSegmentGlyph; DISPLAY_DIGITS],
    ) -> Result<(), GpioError> {
        if self.state.load(Ordering::Acquire) != DISPLAY_STATE_READY {
            return Err(GpioError::state_conflict());
        }

        let next_bank = usize::from(self.active_bank.load(Ordering::Acquire) ^ 1);
        for (index, glyph) in glyphs.into_iter().enumerate() {
            self.frames[next_bank][index]
                .store(encode_display_frame(index, glyph, polarity), Ordering::Release);
        }
        self.active_bank.store(next_bank as u8, Ordering::Release);
        RP2350_TIMER_DISPLAY_STAGED_BANK.store(next_bank as u32, Ordering::Release);
        Ok(())
    }

    fn on_alarm(&self) {
        if self.state.load(Ordering::Acquire) != DISPLAY_STATE_READY {
            return;
        }

        let start = match cortex_m_soc_board::monotonic_raw_now() {
            Ok(now) => now as u32,
            Err(_) => {
                RP2350_TIMER_DISPLAY_LAST_ERROR.store(4, Ordering::Release);
                return;
            }
        };
        let previous_start = RP2350_TIMER_DISPLAY_LAST_START_TICKS.swap(start, Ordering::AcqRel);
        if previous_start != 0 {
            RP2350_TIMER_DISPLAY_LAST_GAP_MICROS
                .store(start.wrapping_sub(previous_start), Ordering::Release);
        }

        if cortex_m_soc_board::irq_acknowledge(DISPLAY_TIMER_IRQN).is_err() {
            RP2350_TIMER_DISPLAY_LAST_ERROR.store(1, Ordering::Release);
            return;
        }
        if arm_next_display_alarm().is_err() {
            RP2350_TIMER_DISPLAY_LAST_ERROR.store(2, Ordering::Release);
            return;
        }

        let bank = usize::from(self.active_bank.load(Ordering::Acquire) & 1);
        let digit = usize::from(self.next_digit.load(Ordering::Acquire) as usize % DISPLAY_DIGITS);
        let frame = self.frames[bank][digit].load(Ordering::Acquire);

        let hardware = unsafe { (*self.hardware.get()).assume_init_mut() };
        if hardware.register.write_bytes_msb_first(&frame.to_le_bytes()).is_err() {
            RP2350_TIMER_DISPLAY_LAST_ERROR.store(3, Ordering::Release);
            return;
        }

        self.next_digit
            .store(((digit + 1) % DISPLAY_DIGITS) as u8, Ordering::Release);
        RP2350_TIMER_DISPLAY_HEARTBEAT.fetch_add(1, Ordering::AcqRel);
        RP2350_TIMER_DISPLAY_ACTIVE_DIGIT.store(digit as u32, Ordering::Release);
        RP2350_TIMER_DISPLAY_LAST_FRAME.store(frame as u32, Ordering::Release);
        if let Ok(end) = cortex_m_soc_board::monotonic_raw_now() {
            let service = (end as u32).wrapping_sub(start);
            RP2350_TIMER_DISPLAY_LAST_SERVICE_MICROS.store(service, Ordering::Release);
            update_max_service_micros(service);
        }
        RP2350_TIMER_DISPLAY_LAST_ERROR.store(0, Ordering::Release);
    }
}

unsafe impl Sync for DisplayController {}

static DISPLAY_CONTROLLER: DisplayController = DisplayController::new();
static mut DISPLAY_INTERRUPT_STACK: [u64; DISPLAY_INTERRUPT_STACK_BYTES / 8] =
    [0; DISPLAY_INTERRUPT_STACK_BYTES / 8];

#[unsafe(no_mangle)]
pub static RP2350_TIMER_DISPLAY_INIT_PHASE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static RP2350_TIMER_DISPLAY_HEARTBEAT: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static RP2350_TIMER_DISPLAY_ACTIVE_DIGIT: AtomicU32 = AtomicU32::new(u32::MAX);
#[unsafe(no_mangle)]
pub static RP2350_TIMER_DISPLAY_LAST_FRAME: AtomicU32 = AtomicU32::new(DISPLAY_BLANK_FRAME as u32);
#[unsafe(no_mangle)]
pub static RP2350_TIMER_DISPLAY_LAST_ALARM_DEADLINE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static RP2350_TIMER_DISPLAY_STAGED_BANK: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static RP2350_TIMER_DISPLAY_LAST_ERROR: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static RP2350_TIMER_DISPLAY_LAST_SERVICE_MICROS: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static RP2350_TIMER_DISPLAY_MAX_SERVICE_MICROS: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static RP2350_TIMER_DISPLAY_LAST_GAP_MICROS: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static RP2350_TIMER_DISPLAY_LAST_START_TICKS: AtomicU32 = AtomicU32::new(0);

/// One retained RP2350 timer-driven four-digit seven-segment display handle.
#[derive(Debug, Clone, Copy)]
pub struct Rp2350TimerFourDigitSevenSegmentDisplay {
    polarity: SevenSegmentPolarity,
}

impl Rp2350TimerFourDigitSevenSegmentDisplay {
    /// Claims one two-register shifted display path and starts one timer-driven refresh loop.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO/state error when the pins, vector slot, or timer substrate cannot
    /// be realized.
    pub fn new(
        data_pin: u8,
        output_enable_pin: u8,
        latch_pin: u8,
        shift_clock_pin: u8,
        polarity: SevenSegmentPolarity,
    ) -> Result<Self, GpioError> {
        DISPLAY_CONTROLLER.ensure_initialized(
            data_pin,
            output_enable_pin,
            latch_pin,
            shift_clock_pin,
        )?;
        Ok(Self { polarity })
    }

    /// Creates one retained common-cathode display.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO/state error when the display substrate cannot start.
    pub fn common_cathode(
        data_pin: u8,
        output_enable_pin: u8,
        latch_pin: u8,
        shift_clock_pin: u8,
    ) -> Result<Self, GpioError> {
        Self::new(
            data_pin,
            output_enable_pin,
            latch_pin,
            shift_clock_pin,
            SevenSegmentPolarity::common_cathode(),
        )
    }

    /// Overwrites the retained four-glyph framebuffer.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO/state error when the retained display is unavailable.
    pub fn set_glyphs(&self, glyphs: [SevenSegmentGlyph; DISPLAY_DIGITS]) -> Result<(), GpioError> {
        DISPLAY_CONTROLLER.stage_glyphs(self.polarity, glyphs)
    }

    /// Writes one hexadecimal value into the retained framebuffer.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO/state error when the retained display is unavailable.
    pub fn set_hex(&self, value: u16) -> Result<(), GpioError> {
        self.set_glyphs(glyphs_from_hex(value))
    }

    /// Clears the retained framebuffer.
    ///
    /// # Errors
    ///
    /// Returns one honest GPIO/state error when the retained display is unavailable.
    pub fn clear(&self) -> Result<(), GpioError> {
        self.set_glyphs([SevenSegmentGlyph::BLANK; DISPLAY_DIGITS])
    }
}

unsafe extern "C" fn timer_display_alarm_irq() {
    DISPLAY_CONTROLLER.on_alarm();
}

fn display_interrupt_stack_policy() -> VectorInlineStackPolicy {
    let size_bytes = NonZeroUsize::new(DISPLAY_INTERRUPT_STACK_BYTES)
        .expect("display interrupt stack size should be non-zero");
    let base = unsafe {
        NonNull::new_unchecked(core::ptr::addr_of_mut!(DISPLAY_INTERRUPT_STACK).cast::<u8>())
    };
    VectorInlineStackPolicy::DedicatedReserved(VectorInlineReservedStack { base, size_bytes })
}

fn arm_next_display_alarm() -> Result<(), GpioError> {
    let now = cortex_m_soc_board::monotonic_raw_now().map_err(gpio_error_from_hardware)? as u32;
    let previous_deadline = RP2350_TIMER_DISPLAY_LAST_ALARM_DEADLINE.load(Ordering::Acquire);
    let mut deadline = if previous_deadline == 0 {
        now.wrapping_add(DISPLAY_SLICE_PERIOD_MICROS.max(1))
    } else {
        previous_deadline.wrapping_add(DISPLAY_SLICE_PERIOD_MICROS.max(1))
    };
    if deadline.wrapping_sub(now) > 0x8000_0000 || deadline == now {
        deadline = now.wrapping_add(DISPLAY_SLICE_PERIOD_MICROS.max(1));
    }
    let alarm = (DISPLAY_TIMER0_BASE
        + DISPLAY_TIMER_ALARM0_OFFSET
        + (DISPLAY_TIMER_ALARM_INDEX * 4)) as *mut u32;
    let inte = (DISPLAY_TIMER0_BASE + DISPLAY_TIMER_INTE_OFFSET) as *mut u32;
    let alarm_bit = 1_u32 << DISPLAY_TIMER_ALARM_INDEX;

    unsafe {
        ptr::write_volatile(alarm, deadline);
        let current = ptr::read_volatile(inte);
        ptr::write_volatile(inte, current | alarm_bit);
    }

    RP2350_TIMER_DISPLAY_LAST_ALARM_DEADLINE.store(deadline, Ordering::Release);
    Ok(())
}

fn update_max_service_micros(observed: u32) {
    let mut current = RP2350_TIMER_DISPLAY_MAX_SERVICE_MICROS.load(Ordering::Acquire);
    while observed > current {
        match RP2350_TIMER_DISPLAY_MAX_SERVICE_MICROS.compare_exchange(
            current,
            observed,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return,
            Err(next) => current = next,
        }
    }
}

const fn glyphs_from_hex(value: u16) -> [SevenSegmentGlyph; DISPLAY_DIGITS] {
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

const fn encode_display_frame(
    digit_index: usize,
    glyph: SevenSegmentGlyph,
    polarity: SevenSegmentPolarity,
) -> u16 {
    let digit = digit_output_byte(Some(digit_index), polarity);
    let segment = segment_output_byte(glyph, polarity);
    u16::from_le_bytes([digit, segment])
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
        Some(index) if index < DISPLAY_DIGITS => {
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

const fn gpio_error_from_thread(error: fusion_sys::thread::ThreadError) -> GpioError {
    match error.kind() {
        ThreadErrorKind::Unsupported => GpioError::unsupported(),
        ThreadErrorKind::Invalid
        | ThreadErrorKind::PermissionDenied
        | ThreadErrorKind::PlacementDenied
        | ThreadErrorKind::SchedulerDenied
        | ThreadErrorKind::StackDenied
        | ThreadErrorKind::Platform(_) => GpioError::invalid(),
        ThreadErrorKind::Busy | ThreadErrorKind::Timeout | ThreadErrorKind::StateConflict => {
            GpioError::state_conflict()
        }
        ThreadErrorKind::ResourceExhausted => GpioError::resource_exhausted(),
    }
}

const fn gpio_error_from_hardware(error: HardwareError) -> GpioError {
    match error.kind() {
        HardwareErrorKind::Unsupported => GpioError::unsupported(),
        HardwareErrorKind::Invalid => GpioError::invalid(),
        HardwareErrorKind::Busy => GpioError::busy(),
        HardwareErrorKind::StateConflict => GpioError::state_conflict(),
        HardwareErrorKind::ResourceExhausted => GpioError::resource_exhausted(),
        HardwareErrorKind::Platform(code) => GpioError::platform(code),
    }
}
