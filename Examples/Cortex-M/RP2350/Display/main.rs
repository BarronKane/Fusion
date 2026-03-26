#![no_std]
#![no_main]

use core::mem::MaybeUninit;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicBool, Ordering};
use core::time::Duration;

use cortex_m_rt::{ExceptionFrame, entry, exception};
use fusion_example_rp2350_on_device::pcu::{
    PcuPioOnDeviceEvent,
    PcuPioOnDeviceFailure,
    run_pcu_pio_smoke_suite,
    suite_pass_display_code,
};
use fusion_std::component::{
    SevenSegmentGlyph,
    SevenSegmentPolarity,
    ShiftedFourDigitSevenSegmentDisplay,
};
use fusion_std::gpio::{Gpio, GpioDriveStrength, GpioPin};
use fusion_std::pcu::PCU;
use fusion_std::thread::async_sleep_for;
use fusion_sys::thread::system_monotonic_time;

mod support;
use support::main_runtime;

const DISPLAY_DATA_PIN: u8 = 12;
const DISPLAY_ENABLE_PIN: u8 = 13;
const DISPLAY_LATCH_PIN: u8 = 14;
const DISPLAY_SHIFT_CLOCK_PIN: u8 = 15;

const FIZZBUZZ_PERIOD: Duration = Duration::from_millis(300);
const STARTUP_PHASE_PERIOD: Duration = Duration::from_millis(500);
const TEST_PHASE_PERIOD: Duration = Duration::from_millis(250);
const PANIC_PHASE_PERIOD: Duration = Duration::from_millis(500);
const DISPLAY_REFRESH_PERIOD: Duration = Duration::from_millis(2);

// Flip this if your module turns out to be the opposite electrical contract. The pinout is the
// same on both variants because apparently the universe enjoys cheap practical jokes.
const DISPLAY_POLARITY: SevenSegmentPolarity = SevenSegmentPolarity::common_cathode();

type PicoDisplay = ShiftedFourDigitSevenSegmentDisplay<GpioPin, GpioPin, GpioPin, GpioPin>;

static mut DISPLAY_STORAGE: MaybeUninit<PicoDisplay> = MaybeUninit::uninit();
static DISPLAY_READY: AtomicBool = AtomicBool::new(false);

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

fn panic_glyphs() -> [SevenSegmentGlyph; 4] {
    [
        SevenSegmentGlyph::from_hex(0xD).expect("D"),
        SevenSegmentGlyph::from_hex(0xE).expect("E"),
        SevenSegmentGlyph::from_hex(0xA).expect("A"),
        SevenSegmentGlyph::from_hex(0xD).expect("D"),
    ]
}

fn step_glyphs(step: u32) -> [SevenSegmentGlyph; 4] {
    let mut glyphs = [
        SevenSegmentGlyph::from_hex(((step >> 12) & 0x0f) as u8).expect("hex nibble"),
        SevenSegmentGlyph::from_hex(((step >> 8) & 0x0f) as u8).expect("hex nibble"),
        SevenSegmentGlyph::from_hex(((step >> 4) & 0x0f) as u8).expect("hex nibble"),
        SevenSegmentGlyph::from_hex((step & 0x0f) as u8).expect("hex nibble"),
    ];
    if step.is_multiple_of(3) {
        glyphs[0] = glyphs[0].with_decimal_point(true);
    }
    if step.is_multiple_of(5) {
        glyphs[3] = glyphs[3].with_decimal_point(true);
    }
    glyphs
}

async fn display_async_pause(display: &mut PicoDisplay, duration: Duration) {
    for _ in 0..refresh_cycles(duration) {
        display.refresh_next().expect("display scan should refresh");
        async_sleep_for(DISPLAY_REFRESH_PERIOD)
            .await
            .expect("async display pause should complete");
    }
}

#[PCU]
fn pcu_increment_word(value: u32) -> u32 {
    value.wrapping_add(1)
}

async fn run_fizzbuzz(display: &mut PicoDisplay) -> ! {
    let mut step = 3_u32;
    loop {
        display.set_glyphs(step_glyphs(step));
        display_async_pause(display, FIZZBUZZ_PERIOD).await;
        step = pcu_increment_word(step);
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

    let runtime = main_runtime();
    let (fibers, runtime) = runtime.into_parts();

    let runner = fibers
        .spawn(move || {
            let runtime = runtime
                .build_explicit()
                .expect("fiber-owned async runtime should build");
            runtime
                .block_on(run_fizzbuzz(display))
                .expect("fiber-owned async runtime should drive fizzbuzz loop");
        })
        .expect("fiber-owned async loop should spawn");

    let _: () = runner
        .join()
        .expect("current-thread fiber join should drive the async loop");

    loop {
        cortex_m::asm::wfi();
    }
}

#[exception]
unsafe fn DefaultHandler(irqn: i16) {
    if irqn == 3 {
        fusion_pal::sys::cortex_m::hal::soc::board::service_event_timeout_irq()
            .expect("event-timeout irq should service");
        return;
    }
    display_exception_loop(0xD000 | (irqn as u16 & 0x0fff));
}

#[exception]
unsafe fn HardFault(_frame: &ExceptionFrame) -> ! {
    display_exception_loop(0xBADF);
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        if let Some(display) = panic_display() {
            display.set_glyphs(panic_glyphs());
            panic_display_pause(display, PANIC_PHASE_PERIOD);
            display.clear();
            panic_display_pause(display, PANIC_PHASE_PERIOD);
            continue;
        }
        cortex_m::asm::wfi();
    }
}
