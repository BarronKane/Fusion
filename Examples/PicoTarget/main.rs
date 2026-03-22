#![no_std]
#![no_main]

use core::cell::UnsafeCell;
use core::panic::PanicInfo;
use core::ptr::{self, NonNull};

use cortex_m_rt::entry;
use fusion_sys::fiber::{Fiber, FiberReturn, FiberStack, FiberYield, yield_now};

const RESETS_BASE: usize = 0x4002_0000;
const RESETS_CLR: *mut u32 = (RESETS_BASE + 0x3000) as *mut u32;
const RESET_DONE: *const u32 = (RESETS_BASE + 0x08) as *const u32;
const RESET_IO_BANK0: u32 = 1 << 6;
const RESET_PADS_BANK0: u32 = 1 << 9;

const IO_BANK0_BASE: usize = 0x4002_8000;
const PADS_BANK0_BASE: usize = 0x4003_8000;
const PADS_GPIO_IE_BIT: u32 = 1 << 6;
const PADS_GPIO_OD_BIT: u32 = 1 << 7;
const PADS_GPIO_ISO_BIT: u32 = 1 << 8;

const SIO_BASE: usize = 0xD000_0000;
const SIO_GPIO_OUT_SET: *mut u32 = (SIO_BASE + 0x18) as *mut u32;
const SIO_GPIO_OUT_CLR: *mut u32 = (SIO_BASE + 0x20) as *mut u32;
const SIO_GPIO_OE_SET: *mut u32 = (SIO_BASE + 0x38) as *mut u32;

const BLUE_LED_PIN: u32 = 28;
const RED_LED_PIN: u32 = 27;
const FIBER_STACK_BYTES: usize = 4096;
const STEP_DELAY_CYCLES: u32 = 1_000_000;

#[repr(align(16))]
struct AlignedBytes<const N: usize>([u8; N]);

struct StaticFiberStack<const N: usize>(UnsafeCell<AlignedBytes<N>>);

impl<const N: usize> StaticFiberStack<N> {
    const fn new() -> Self {
        Self(UnsafeCell::new(AlignedBytes([0; N])))
    }

    fn fiber_stack(&self) -> FiberStack {
        // SAFETY: the backing storage is static, aligned, and never moved after creation.
        let bytes = unsafe { &mut (*self.0.get()).0 };
        FiberStack::new(
            // SAFETY: static storage is always non-null.
            unsafe { NonNull::new_unchecked(bytes.as_mut_ptr()) },
            bytes.len(),
        )
        .expect("static fiber stack should always be valid")
    }
}

// SAFETY: the example is single-process and only hands these stacks to one fiber each.
unsafe impl<const N: usize> Sync for StaticFiberStack<N> {}

static FIBONACCI_STACK: StaticFiberStack<FIBER_STACK_BYTES> = StaticFiberStack::new();
static DISPATCH_STACK: StaticFiberStack<FIBER_STACK_BYTES> = StaticFiberStack::new();

#[derive(Clone, Copy)]
struct LedCommand {
    blue_on: bool,
    red_on: bool,
}

struct SharedState {
    current: u64,
    next: u64,
    command: LedCommand,
    command_ready: bool,
}

impl SharedState {
    const fn new() -> Self {
        Self {
            current: 0,
            next: 1,
            command: LedCommand {
                blue_on: false,
                red_on: false,
            },
            command_ready: false,
        }
    }
}

struct FiberContext {
    shared: *mut SharedState,
}

fn gpio_init_output(pin: u32) {
    // SAFETY: these are RP2350 reset-controller MMIO registers.
    unsafe { ptr::write_volatile(RESETS_CLR, RESET_IO_BANK0 | RESET_PADS_BANK0) };

    while unsafe { ptr::read_volatile(RESET_DONE) } & (RESET_IO_BANK0 | RESET_PADS_BANK0)
        != (RESET_IO_BANK0 | RESET_PADS_BANK0)
    {}

    let pad = (PADS_BANK0_BASE + 0x04 + (pin as usize) * 4) as *mut u32;
    // SAFETY: `pad` points at the selected RP2350 pad register.
    let pad_value = unsafe { ptr::read_volatile(pad) };
    // SAFETY: we are programming the selected pad for normal GPIO use.
    unsafe {
        ptr::write_volatile(
            pad,
            (pad_value | PADS_GPIO_IE_BIT) & !(PADS_GPIO_OD_BIT | PADS_GPIO_ISO_BIT),
        )
    };

    let ctrl = (IO_BANK0_BASE + (pin as usize) * 8 + 4) as *mut u32;
    // SAFETY: function 5 selects SIO for RP2350 GPIOs.
    unsafe { ptr::write_volatile(ctrl, 5) };

    // SAFETY: SIO output and OE registers are write-only MMIO for GPIO ownership.
    unsafe {
        ptr::write_volatile(SIO_GPIO_OUT_CLR, 1 << pin);
        ptr::write_volatile(SIO_GPIO_OE_SET, 1 << pin);
    }
}

fn gpio_write(pin: u32, high: bool) {
    let mask = 1 << pin;
    // SAFETY: these are write-only RP2350 SIO GPIO registers.
    unsafe {
        if high {
            ptr::write_volatile(SIO_GPIO_OUT_SET, mask);
        } else {
            ptr::write_volatile(SIO_GPIO_OUT_CLR, mask);
        }
    }
}

unsafe fn fibonacci_fiber(context: *mut ()) -> FiberReturn {
    // SAFETY: the caller passes a live `FiberContext` for the lifetime of the fiber.
    let state = unsafe { &mut *(*(context.cast::<FiberContext>())).shared };

    loop {
        let value = state.current;
        let is_even = value & 1 == 0;
        state.command = LedCommand {
            blue_on: is_even,
            red_on: !is_even,
        };
        state.command_ready = true;

        let next = state.current.wrapping_add(state.next);
        state.current = state.next;
        state.next = next;

        yield_now().expect("fibonacci fiber should yield back to the main scheduler");
    }
}

unsafe fn dispatch_fiber(context: *mut ()) -> FiberReturn {
    // SAFETY: the caller passes a live `FiberContext` for the lifetime of the fiber.
    let state = unsafe { &mut *(*(context.cast::<FiberContext>())).shared };

    loop {
        if state.command_ready {
            gpio_write(BLUE_LED_PIN, state.command.blue_on);
            gpio_write(RED_LED_PIN, state.command.red_on);
            state.command_ready = false;
        }

        yield_now().expect("dispatch fiber should yield back to the main scheduler");
    }
}

fn run_once(fiber: &mut Fiber) {
    match fiber.resume().expect("fiber resume should succeed") {
        FiberYield::Yielded => {}
        FiberYield::Completed(_) => panic!("demo fibers are expected to run forever"),
    }
}

#[entry]
fn main() -> ! {
    gpio_init_output(BLUE_LED_PIN);
    gpio_init_output(RED_LED_PIN);
    gpio_write(BLUE_LED_PIN, false);
    gpio_write(RED_LED_PIN, false);

    let mut shared = SharedState::new();
    let mut fibonacci_context = FiberContext {
        shared: &mut shared,
    };
    let mut dispatch_context = FiberContext {
        shared: &mut shared,
    };

    let mut fibonacci = Fiber::new(
        FIBONACCI_STACK.fiber_stack(),
        fibonacci_fiber,
        (&raw mut fibonacci_context).cast(),
    )
    .expect("RP2350 should support low-level fibers");
    let mut dispatch = Fiber::new(
        DISPATCH_STACK.fiber_stack(),
        dispatch_fiber,
        (&raw mut dispatch_context).cast(),
    )
    .expect("RP2350 should support low-level fibers");

    loop {
        run_once(&mut fibonacci);
        run_once(&mut dispatch);
        cortex_m::asm::delay(STEP_DELAY_CYCLES);
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    gpio_write(BLUE_LED_PIN, false);
    gpio_write(RED_LED_PIN, true);
    loop {
        cortex_m::asm::wfi();
    }
}
