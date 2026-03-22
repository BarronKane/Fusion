#![no_std]
#![no_main]

use core::panic::PanicInfo;
use core::ptr;
use cortex_m_rt::entry;

// ---- RP2350 Register Addresses ----

// Resets (APB peripheral)
const RESETS_BASE: usize = 0x4002_0000;
// Atomic clear alias — writing bits here clears them in RESET register
const RESETS_CLR: *mut u32 = (RESETS_BASE + 0x3000) as *mut u32;
const RESET_DONE: *const u32 = (RESETS_BASE + 0x08) as *const u32;
const RESET_IO_BANK0: u32 = 1 << 6;
const RESET_PADS_BANK0: u32 = 1 << 9;

// IO Bank 0 — function select for GPIO 0-47
const IO_BANK0_BASE: usize = 0x4002_8000;

// Pads Bank 0 — electrical pad configuration
const PADS_BANK0_BASE: usize = 0x4003_8000;

// SIO — Single-cycle I/O (GPIO output/enable for bank 0, GPIO 0-31)
// NOTE: RP2350 offsets differ from RP2040 due to 48-GPIO bank split
const SIO_BASE: usize = 0xD000_0000;
const SIO_GPIO_OUT_SET: *mut u32 = (SIO_BASE + 0x18) as *mut u32;
const SIO_GPIO_OUT_CLR: *mut u32 = (SIO_BASE + 0x20) as *mut u32;
const SIO_GPIO_OUT_XOR: *mut u32 = (SIO_BASE + 0x28) as *mut u32;
const SIO_GPIO_OE_SET: *mut u32 = (SIO_BASE + 0x38) as *mut u32;

// Target: GP15 (physical pin 20) -> 250Ω -> LED -> GND (pin 38)
const LED_PIN: u32 = 15;

// ---- GPIO Primitives ----

/// Release IO_BANK0 and PADS_BANK0 from reset, configure `pin` as SIO output.
///
/// Performs direct MMIO register access. Call once during init.
fn gpio_init_output(pin: u32) {
    // Release peripherals from reset via atomic clear alias
    unsafe { ptr::write_volatile(RESETS_CLR, RESET_IO_BANK0 | RESET_PADS_BANK0) };

    // Spin until both peripherals report reset complete
    while unsafe { ptr::read_volatile(RESET_DONE) } & (RESET_IO_BANK0 | RESET_PADS_BANK0)
        != (RESET_IO_BANK0 | RESET_PADS_BANK0)
    {}

    // Pad config — clear OD (output disable, bit 7). Default pad state is fine otherwise.
    let pad = (PADS_BANK0_BASE + 0x04 + (pin as usize) * 4) as *mut u32;
    let v = unsafe { ptr::read_volatile(pad) };
    unsafe { ptr::write_volatile(pad, v & !(1 << 7)) };

    // Function select — SIO is function 5
    let ctrl = (IO_BANK0_BASE + (pin as usize) * 8 + 4) as *mut u32;
    unsafe { ptr::write_volatile(ctrl, 5) };

    // Enable output, start low
    unsafe {
        ptr::write_volatile(SIO_GPIO_OE_SET, 1 << pin);
        ptr::write_volatile(SIO_GPIO_OUT_CLR, 1 << pin);
    }
}

#[inline(always)]
fn led_on() {
    unsafe { ptr::write_volatile(SIO_GPIO_OUT_SET, 1 << LED_PIN) };
}

#[inline(always)]
fn led_off() {
    unsafe { ptr::write_volatile(SIO_GPIO_OUT_CLR, 1 << LED_PIN) };
}

#[inline(always)]
fn led_toggle() {
    unsafe { ptr::write_volatile(SIO_GPIO_OUT_XOR, 1 << LED_PIN) };
}

// ---- Timing ----

/// Busy-wait delay. At ring oscillator (~6.5 MHz, no PLL), each iteration
/// takes ~7 cycles ≈ ~1µs. At 150 MHz (with PLL), ~46ns/iter.
/// Tune by observation — this is pre-timer.
fn delay(iterations: u32) {
    for _ in 0..iterations {
        cortex_m::asm::nop();
    }
}

// ---- Blink Patterns ----
// Each pattern is a function that runs one blink cycle.
// Structured as standalone fns so they slot directly into fiber bodies later.

/// Short rapid strobe — ~23ms on/off at ring osc
fn pattern_strobe() {
    led_on();
    delay(500_000);
    led_off();
    delay(500_000);
}

/// Heartbeat — quick flash, pause, quick flash, long pause
fn pattern_heartbeat() {
    led_on();
    delay(300_000);
    led_off();
    delay(300_000);
    led_on();
    delay(300_000);
    led_off();
    delay(2_000_000);
}

/// Slow calm blink — ~230ms on/off at ring osc
fn pattern_slow() {
    led_on();
    delay(5_000_000);
    led_off();
    delay(5_000_000);
}

/// Morse SOS — ... --- ...
fn pattern_sos() {
    let dot = 300_000_u32;
    let dash = 900_000_u32;
    let gap = 300_000_u32;
    let letter_gap = 900_000_u32;

    // S: ...
    for _ in 0..3_u32 {
        led_on();
        delay(dot);
        led_off();
        delay(gap);
    }
    delay(letter_gap);

    // O: ---
    for _ in 0..3_u32 {
        led_on();
        delay(dash);
        led_off();
        delay(gap);
    }
    delay(letter_gap);

    // S: ...
    for _ in 0..3_u32 {
        led_on();
        delay(dot);
        led_off();
        delay(gap);
    }
    delay(letter_gap * 2);
}

const PATTERN_COUNT: u32 = 4;
const PATTERNS: [fn(); 4] = [pattern_strobe, pattern_heartbeat, pattern_slow, pattern_sos];

// ---- Fibonacci Selector ----

/// Returns fib(n) % m. Pisano period guarantees a deterministic repeating
/// cycle over the pattern table — visually verifiable.
const fn fib_mod(n: u32, m: u32) -> u32 {
    if m <= 1 {
        return 0;
    }
    let mut a: u32 = 0;
    let mut b: u32 = 1;
    let mut i = 0;
    while i < n {
        let next = (a + b) % m;
        a = b;
        b = next;
        i += 1;
    }
    a % m
}

// ---- Entry ----

#[entry]
fn main() -> ! {
    gpio_init_output(LED_PIN);

    // Quick sanity blink — 3 fast toggles to confirm GPIO is alive
    for _ in 0..3_u32 {
        led_toggle();
        delay(1_000_000);
    }
    led_off();
    delay(2_000_000);

    // Fibonacci-modulo pattern loop
    // fib(n) mod 4 cycles through: 0,1,1,2,3,1,0,1,1,2,3,1,... (Pisano period 6)
    // So the visual pattern repeats every 6 cycles:
    //   strobe → heartbeat → heartbeat → slow → SOS → heartbeat → (repeat)
    let mut cycle: u32 = 0;
    loop {
        let idx = fib_mod(cycle, PATTERN_COUNT) as usize;
        PATTERNS[idx]();
        cycle = cycle.wrapping_add(1);
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        cortex_m::asm::wfi();
    }
}
