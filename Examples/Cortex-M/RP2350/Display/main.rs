#![no_std]
#![no_main]

use core::mem::MaybeUninit;
use core::panic::PanicInfo;
use core::pin::pin;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, Ordering};
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use core::time::Duration;

use cortex_m_rt::{ExceptionFrame, entry, exception};
use fusion_example_rp2350_on_device::pcu::{
    PcuPioOnDeviceEvent,
    PcuPioOnDeviceFailure,
    run_pcu_pio_smoke_suite,
    suite_pass_display_code,
};
use fusion_sys::hardware::peripheral::{
    SevenSegmentGlyph,
    SevenSegmentPolarity,
    ShiftedFourDigitSevenSegmentDisplay,
};
use fusion_std::gpio::{Gpio, GpioDriveStrength, GpioPin};
use fusion_std::pcu::PCU;
use fusion_std::thread::yield_now;
use fusion_sys::thread::system_monotonic_time;

mod backend {
    include!(concat!(env!("OUT_DIR"), "/rp2350_backing.rs"));
}
use backend::{drive_once, spawn};

const DISPLAY_DATA_PIN: u8 = 12;
const DISPLAY_ENABLE_PIN: u8 = 13;
const DISPLAY_LATCH_PIN: u8 = 14;
const DISPLAY_SHIFT_CLOCK_PIN: u8 = 15;

const LEFT_FIZZBUZZ_PERIOD: Duration = Duration::from_millis(200);
const RIGHT_FIZZBUZZ_PERIOD: Duration = Duration::from_millis(300);
const STARTUP_PHASE_PERIOD: Duration = Duration::from_millis(500);
const TEST_PHASE_PERIOD: Duration = Duration::from_millis(250);
const PANIC_PHASE_PERIOD: Duration = Duration::from_millis(500);
const DISPLAY_REFRESH_PERIOD: Duration = Duration::from_millis(2);
const QUIESCE_IRQS: &[u16] = &[10, 11, 12, 13, 15, 16, 17, 18, 19, 20];

// Flip this if your module turns out to be the opposite electrical contract. The pinout is the
// same on both variants because apparently the universe enjoys cheap practical jokes.
const DISPLAY_POLARITY: SevenSegmentPolarity = SevenSegmentPolarity::common_cathode();

type PicoDisplay = ShiftedFourDigitSevenSegmentDisplay<GpioPin, GpioPin, GpioPin, GpioPin>;

static mut DISPLAY_STORAGE: MaybeUninit<PicoDisplay> = MaybeUninit::uninit();
static DISPLAY_READY: AtomicBool = AtomicBool::new(false);
static DISPLAY_GLYPHS: [AtomicU8; 4] = [const { AtomicU8::new(0) }; 4];
static PANIC_DISPLAY_CODE: AtomicU16 = AtomicU16::new(0xDEAD);

fn configure_output_pin(pin: u8) -> GpioPin {
    let mut gpio = Gpio::take(pin).expect("gpio pin should be claimable");
    gpio.set_drive_strength(GpioDriveStrength::MilliAmps4)
        .expect("drive strength should be configurable");
    gpio.configure_output(false)
        .expect("gpio pin should configure for output");
    gpio
}

fn init_display() -> &'static mut PicoDisplay {
    unsafe {
        let display = core::ptr::addr_of_mut!(DISPLAY_STORAGE).cast::<PicoDisplay>();
        display.write(
            ShiftedFourDigitSevenSegmentDisplay::with_output_enable(
                configure_output_pin(DISPLAY_DATA_PIN),
                configure_output_pin(DISPLAY_SHIFT_CLOCK_PIN),
                configure_output_pin(DISPLAY_LATCH_PIN),
                configure_output_pin(DISPLAY_ENABLE_PIN),
                DISPLAY_POLARITY,
            )
            .expect("shifted display should configure"),
        );
        DISPLAY_READY.store(true, Ordering::Release);
        &mut *display
    }
}

fn panic_display() -> Option<&'static mut PicoDisplay> {
    if !DISPLAY_READY.load(Ordering::Acquire) {
        return None;
    }
    unsafe {
        let display = core::ptr::addr_of_mut!(DISPLAY_STORAGE).cast::<PicoDisplay>();
        Some(&mut *display)
    }
}

fn refresh_cycles(duration: Duration) -> usize {
    let slice_ms = DISPLAY_REFRESH_PERIOD.as_millis().max(1);
    let total_ms = duration.as_millis().max(slice_ms);
    usize::try_from(total_ms.div_ceil(slice_ms)).unwrap_or(usize::MAX)
}

fn blocking_display_pause(display: &mut PicoDisplay, duration: Duration) {
    for _ in 0..refresh_cycles(duration) {
        display.refresh_next().expect("display scan should refresh");
        system_monotonic_time()
            .sleep_for(DISPLAY_REFRESH_PERIOD)
            .expect("display pause should complete");
    }
}

fn panic_display_pause(display: &mut PicoDisplay, duration: Duration) {
    for _ in 0..refresh_cycles(duration) {
        let _ = display.refresh_next();
        if system_monotonic_time()
            .sleep_for(DISPLAY_REFRESH_PERIOD)
            .is_ok()
        {
            continue;
        }
        for _ in 0..50_000 {
            core::hint::spin_loop();
        }
    }
}

fn startup_sequence(display: &mut PicoDisplay) {
    for value in [0x0001, 0x0002, 0x0003, 0x0000] {
        display.set_hex(value);
        blocking_display_pause(display, STARTUP_PHASE_PERIOD);
    }
}

fn display_code(display: &mut PicoDisplay, code: u16, duration: Duration) {
    display.set_hex(code);
    blocking_display_pause(display, duration);
}

fn display_failure_loop(display: &mut PicoDisplay, failure: PcuPioOnDeviceFailure) -> ! {
    loop {
        display.set_hex(failure.display_code());
        blocking_display_pause(display, PANIC_PHASE_PERIOD);
        display.clear();
        blocking_display_pause(display, PANIC_PHASE_PERIOD);
    }
}

fn startup_pcu_self_test(display: &mut PicoDisplay) {
    let result = run_pcu_pio_smoke_suite(|event| match event {
        PcuPioOnDeviceEvent::Starting { code } => {
            display_code(display, 0x1000 | code, TEST_PHASE_PERIOD)
        }
        PcuPioOnDeviceEvent::Passed { code } => {
            display_code(display, 0xA000 | code, TEST_PHASE_PERIOD)
        }
        PcuPioOnDeviceEvent::Failed { failure } => display_failure_loop(display, failure),
    });

    if let Err(failure) = result {
        display_failure_loop(display, failure);
    }
    display_code(display, suite_pass_display_code(), STARTUP_PHASE_PERIOD);
}

fn quiesce_nonessential_irqs() {
    for &irqn in QUIESCE_IRQS {
        let _ = fusion_pal::sys::soc::cortex_m::rp2350::irq_disable(irqn);
        let _ = fusion_pal::sys::soc::cortex_m::rp2350::irq_clear_pending(irqn);
        if fusion_pal::sys::soc::cortex_m::rp2350::irq_acknowledge_supported(irqn) {
            let _ = fusion_pal::sys::soc::cortex_m::rp2350::irq_acknowledge(irqn);
        }
    }
}

fn display_exception_loop(code: u16) -> ! {
    loop {
        if let Some(display) = panic_display() {
            display.set_hex(code);
            panic_display_pause(display, PANIC_PHASE_PERIOD);
            display.clear();
            panic_display_pause(display, PANIC_PHASE_PERIOD);
            continue;
        }
        cortex_m::asm::wfi();
    }
}

fn repeated_nibble_code(nibble: u8) -> u16 {
    let digit = u16::from(nibble & 0x0f);
    digit | (digit << 4) | (digit << 8) | (digit << 12)
}

fn display_irq_exception_loop(irqn: u16) -> ! {
    let high = ((irqn >> 4) & 0x0f) as u8;
    let low = (irqn & 0x0f) as u8;
    loop {
        if let Some(display) = panic_display() {
            display.set_hex(0xDDDD);
            panic_display_pause(display, PANIC_PHASE_PERIOD);
            display.set_hex(repeated_nibble_code(high));
            panic_display_pause(display, PANIC_PHASE_PERIOD);
            display.set_hex(repeated_nibble_code(low));
            panic_display_pause(display, PANIC_PHASE_PERIOD);
            display.clear();
            panic_display_pause(display, PANIC_PHASE_PERIOD);
            continue;
        }
        cortex_m::asm::wfi();
    }
}

fn set_panic_display_code(code: u16) {
    PANIC_DISPLAY_CODE.store(code, Ordering::Release);
}

fn panic_glyphs() -> [SevenSegmentGlyph; 4] {
    [
        SevenSegmentGlyph::from_hex(0xD).expect("D"),
        SevenSegmentGlyph::from_hex(0xE).expect("E"),
        SevenSegmentGlyph::from_hex(0xA).expect("A"),
        SevenSegmentGlyph::from_hex(0xD).expect("D"),
    ]
}

fn half_step_glyphs(step: u8) -> [SevenSegmentGlyph; 2] {
    let mut glyphs = [
        SevenSegmentGlyph::from_hex((step >> 4) & 0x0f).expect("upper nibble should be valid"),
        SevenSegmentGlyph::from_hex(step & 0x0f).expect("lower nibble should be valid"),
    ];
    if step.is_multiple_of(3) {
        glyphs[0] = glyphs[0].with_decimal_point(true);
    }
    if step.is_multiple_of(5) {
        glyphs[1] = glyphs[1].with_decimal_point(true);
    }
    glyphs
}

fn store_half_glyphs(offset: usize, glyphs: [SevenSegmentGlyph; 2]) {
    DISPLAY_GLYPHS[offset].store(glyphs[0].raw(), Ordering::Relaxed);
    DISPLAY_GLYPHS[offset + 1].store(glyphs[1].raw(), Ordering::Relaxed);
}

fn load_framebuffer_glyphs() -> [SevenSegmentGlyph; 4] {
    [
        SevenSegmentGlyph::from_raw(DISPLAY_GLYPHS[0].load(Ordering::Relaxed)),
        SevenSegmentGlyph::from_raw(DISPLAY_GLYPHS[1].load(Ordering::Relaxed)),
        SevenSegmentGlyph::from_raw(DISPLAY_GLYPHS[2].load(Ordering::Relaxed)),
        SevenSegmentGlyph::from_raw(DISPLAY_GLYPHS[3].load(Ordering::Relaxed)),
    ]
}

fn publish_counter_display(offset: usize, step: u8) {
    store_half_glyphs(offset, half_step_glyphs(step));
}

#[PCU]
fn pcu_increment_counter(value: u8) -> u8 {
    value.wrapping_add(1)
}

async fn run_half_fizzbuzz(slot: usize, offset: usize, period: Duration) -> ! {
    let mut step = 0_u8;
    loop {
        set_panic_display_code(0x2100 | slot as u16);
        publish_counter_display(offset, step);
        set_panic_display_code(0x2200 | slot as u16);
        MonotonicDelay::new(period).await;
        set_panic_display_code(0x2300 | slot as u16);
        step = pcu_increment_counter(step);
        set_panic_display_code(0x2400 | slot as u16);
    }
}

struct MonotonicDelay {
    period: Duration,
    deadline: Option<Duration>,
}

impl MonotonicDelay {
    const fn new(period: Duration) -> Self {
        Self {
            period,
            deadline: None,
        }
    }
}

impl core::future::Future for MonotonicDelay {
    type Output = ();

    fn poll(mut self: core::pin::Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let monotonic = system_monotonic_time();
        let now = monotonic
            .now()
            .expect("counter delay should observe monotonic time");
        let deadline = match self.deadline {
            Some(deadline) => deadline,
            None => {
                let deadline = now
                    .checked_add(self.period)
                    .expect("counter delay deadline should remain representable");
                self.deadline = Some(deadline);
                deadline
            }
        };

        if now >= deadline {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

const NOOP_RAW_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    noop_raw_waker_clone,
    noop_raw_waker_wake,
    noop_raw_waker_wake,
    noop_raw_waker_drop,
);

const fn noop_raw_waker() -> RawWaker {
    RawWaker::new(core::ptr::null(), &NOOP_RAW_WAKER_VTABLE)
}

unsafe fn noop_raw_waker_clone(_data: *const ()) -> RawWaker {
    noop_raw_waker()
}

unsafe fn noop_raw_waker_wake(_data: *const ()) {}

unsafe fn noop_raw_waker_drop(_data: *const ()) {}

fn noop_waker() -> Waker {
    unsafe { Waker::from_raw(noop_raw_waker()) }
}

fn run_counter_fiber(slot: usize, offset: usize, period: Duration) -> ! {
    let waker = noop_waker();
    let mut context = Context::from_waker(&waker);
    let mut future = pin!(run_half_fizzbuzz(slot, offset, period));

    loop {
        set_panic_display_code(0x1100 | slot as u16);
        if matches!(future.as_mut().poll(&mut context), Poll::Ready(_)) {
            display_exception_loop(0xE100 | slot as u16);
        }
        set_panic_display_code(0x1200 | slot as u16);
        if yield_now().is_err() {
            display_exception_loop(0xE101 | slot as u16);
        }
    }
}

#[entry]
fn main() -> ! {
    let display = init_display();
    display.clear();
    display
        .disable()
        .expect("display should accept initial blanking");
    startup_sequence(display);
    startup_pcu_self_test(display);
    quiesce_nonessential_irqs();
    publish_counter_display(0, 0);
    publish_counter_display(2, 0);

    let _left = spawn(move || run_counter_fiber(0, 0, LEFT_FIZZBUZZ_PERIOD))
        .expect("left counter fiber should spawn");
    let _right = spawn(move || run_counter_fiber(1, 2, RIGHT_FIZZBUZZ_PERIOD))
        .expect("right counter fiber should spawn");

    loop {
        set_panic_display_code(0x3100);
        display.set_glyphs(load_framebuffer_glyphs());
        if display.refresh_next().is_err() {
            display_exception_loop(0xE400);
        }
        set_panic_display_code(0x3200);
        if drive_once().is_err() {
            display_exception_loop(0xE401);
        }
        set_panic_display_code(0x3300);
        if system_monotonic_time()
            .sleep_for(DISPLAY_REFRESH_PERIOD)
            .is_err()
        {
            display_exception_loop(0xE402);
        }
    }
}

#[exception]
unsafe fn DefaultHandler(irqn: i16) {
    if irqn == 3 {
        fusion_pal::sys::soc::cortex_m::rp2350::service_event_timeout_irq()
            .expect("event-timeout irq should service");
        return;
    }
    let irqn = irqn as u16;
    if fusion_pal::sys::soc::cortex_m::rp2350::irq_acknowledge_supported(irqn)
        && fusion_pal::sys::soc::cortex_m::rp2350::irq_acknowledge(irqn).is_ok()
    {
        if QUIESCE_IRQS.contains(&irqn) {
            let _ = fusion_pal::sys::soc::cortex_m::rp2350::irq_disable(irqn);
            let _ = fusion_pal::sys::soc::cortex_m::rp2350::irq_clear_pending(irqn);
        }
        return;
    }
    display_irq_exception_loop(irqn);
}

#[exception]
unsafe fn HardFault(_frame: &ExceptionFrame) -> ! {
    display_exception_loop(0xBADF);
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        if let Some(display) = panic_display() {
            let code = PANIC_DISPLAY_CODE.load(Ordering::Acquire);
            if code == 0xDEAD {
                display.set_glyphs(panic_glyphs());
            } else {
                display.set_hex(code);
            }
            panic_display_pause(display, PANIC_PHASE_PERIOD);
            display.clear();
            panic_display_pause(display, PANIC_PHASE_PERIOD);
            continue;
        }
        cortex_m::asm::wfi();
    }
}
