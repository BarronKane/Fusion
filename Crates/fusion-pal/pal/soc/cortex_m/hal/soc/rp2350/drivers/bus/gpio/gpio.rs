//! RP2350 GPIO hardware substrate implementing the generic `fusion-hal` GPIO contract.

use core::ptr;
use core::sync::atomic::{
    AtomicU32,
    Ordering,
};

use fusion_hal::contract::drivers::bus::gpio::{
    GpioCapabilities,
    GpioDriveStrength,
    GpioError,
    GpioFunction,
    GpioImplementationKind,
    GpioPinDescriptor,
    GpioProviderCaps,
    GpioPull,
    GpioSupport,
};
use fusion_hal::drivers::bus::gpio::interface::contract::{
    GpioHardware as GpioHardwareContract,
    GpioHardwarePin as GpioHardwarePinContract,
};

use crate::pal::soc::cortex_m::hal::soc::rp2350::RP2350_PICO2W_RESERVED_GPIO_PINS;
use crate::pal::soc::cortex_m::hal::soc::board;

const RP2350_GPIO_COUNT: u8 = 30 - RP2350_PICO2W_RESERVED_GPIO_PINS.len() as u8;
const RP2350_PAD_PDE_BIT: u32 = 1 << 2;
const RP2350_PAD_PUE_BIT: u32 = 1 << 3;
const RP2350_PAD_DRIVE_LSB: u32 = 4;
const RP2350_PAD_IE_BIT: u32 = 1 << 6;
const RP2350_PAD_OD_BIT: u32 = 1 << 7;
const RP2350_PAD_ISO_BIT: u32 = 1 << 8;
const RP2350_RESET_DONE_OFFSET: usize = 0x08;
const RP2350_REG_ALIAS_CLR_OFFSET: usize = 0x3000;
const RP2350_RESET_IO_BANK0: u32 = 1 << 6;
const RP2350_RESET_PADS_BANK0: u32 = 1 << 9;
const RP2350_SIO_GPIO_IN_OFFSET: usize = 0x04;
const RP2350_SIO_GPIO_OUT_SET_OFFSET: usize = 0x18;
const RP2350_SIO_GPIO_OUT_CLR_OFFSET: usize = 0x20;
const RP2350_SIO_GPIO_OE_SET_OFFSET: usize = 0x38;
const RP2350_SIO_GPIO_OE_CLR_OFFSET: usize = 0x40;
const RP2350_GPIO_CTRL_STRIDE: usize = 8;
const RP2350_GPIO_CTRL_FUNCSEL_OFFSET: usize = 4;
const RP2350_PAD_STRIDE: usize = 4;
const RP2350_PADS_BANK0_FIRST_PAD_OFFSET: usize = 0x04;
const RP2350_SIO_FUNCSEL: u32 = 5;

const RP2350_GPIO_CAPABILITIES: GpioCapabilities = GpioCapabilities::INPUT
    .union(GpioCapabilities::OUTPUT)
    .union(GpioCapabilities::ALTERNATE_FUNCTIONS)
    .union(GpioCapabilities::PULLS)
    .union(GpioCapabilities::DRIVE_STRENGTH)
    .union(GpioCapabilities::INTERRUPTS);

static CLAIMED_GPIO: AtomicU32 = AtomicU32::new(0);

macro_rules! rp2350_gpio_descriptors {
    ($($pin:literal),* $(,)?) => {
        [
            $(
                GpioPinDescriptor {
                    pin: $pin,
                    name: concat!("gpio", stringify!($pin)),
                    capabilities: RP2350_GPIO_CAPABILITIES,
                },
            )*
        ]
    };
}

static RP2350_GPIO_PINS: [GpioPinDescriptor; RP2350_GPIO_COUNT as usize] = rp2350_gpio_descriptors![
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 26, 27, 28,
];

/// RP2350 hardware-facing GPIO provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct GpioHardware;

/// RP2350 hardware-owned GPIO pin.
#[derive(Debug)]
pub struct GpioPinHardware {
    pin: u8,
}

impl GpioHardwareContract for GpioHardware {
    type Pin = GpioPinHardware;

    fn support() -> GpioSupport {
        if !rp2350_gpio_topology_available() {
            return GpioSupport::unsupported();
        }

        GpioSupport {
            caps: GpioProviderCaps::ENUMERATE
                | GpioProviderCaps::CLAIM
                | GpioProviderCaps::STATIC_TOPOLOGY
                | GpioProviderCaps::INPUT
                | GpioProviderCaps::OUTPUT
                | GpioProviderCaps::ALTERNATE_FUNCTIONS
                | GpioProviderCaps::PULLS
                | GpioProviderCaps::DRIVE_STRENGTH
                | GpioProviderCaps::INTERRUPTS,
            implementation: GpioImplementationKind::Native,
            pin_count: RP2350_GPIO_PINS.len() as u16,
        }
    }

    fn pins() -> &'static [GpioPinDescriptor] {
        if rp2350_gpio_topology_available() {
            &RP2350_GPIO_PINS
        } else {
            &[]
        }
    }

    fn claim_pin(pin: u8) -> Result<Self::Pin, GpioError> {
        claim(pin)?;
        Ok(Self::Pin { pin })
    }
}

impl GpioHardwarePinContract for GpioPinHardware {
    fn pin(&self) -> u8 {
        self.pin
    }

    fn capabilities(&self) -> GpioCapabilities {
        GpioHardware::pins()
            .iter()
            .find(|descriptor| descriptor.pin == self.pin)
            .map(|descriptor| descriptor.capabilities)
            .unwrap_or_else(GpioCapabilities::empty)
    }

    fn set_function(&mut self, function: GpioFunction) -> Result<(), GpioError> {
        set_function(self.pin, function)
    }

    fn configure_input(&mut self) -> Result<(), GpioError> {
        configure_input(self.pin)
    }

    fn read_level(&self) -> Result<bool, GpioError> {
        read(self.pin)
    }

    fn configure_output(&mut self, initial_high: bool) -> Result<(), GpioError> {
        configure_output(self.pin, initial_high)
    }

    fn set_level(&mut self, high: bool) -> Result<(), GpioError> {
        write(self.pin, high)
    }

    fn set_pull(&mut self, pull: GpioPull) -> Result<(), GpioError> {
        set_pull(self.pin, pull)
    }

    fn set_drive_strength(&mut self, strength: GpioDriveStrength) -> Result<(), GpioError> {
        set_drive_strength(self.pin, strength)
    }
}

impl Drop for GpioPinHardware {
    fn drop(&mut self) {
        release(self.pin);
    }
}

fn validate_pin(pin: u8) -> Result<(), GpioError> {
    if GpioHardware::pins()
        .iter()
        .any(|descriptor| descriptor.pin == pin)
    {
        Ok(())
    } else {
        Err(GpioError::invalid())
    }
}

fn validate_board_owned_pin(pin: u8) -> Result<(), GpioError> {
    if RP2350_PICO2W_RESERVED_GPIO_PINS.contains(&pin) {
        Ok(())
    } else {
        Err(GpioError::invalid())
    }
}

fn claim(pin: u8) -> Result<(), GpioError> {
    validate_pin(pin)?;
    let mask = 1_u32 << pin;
    let mut claimed = CLAIMED_GPIO.load(Ordering::Acquire);
    loop {
        if claimed & mask != 0 {
            return Err(GpioError::state_conflict());
        }
        match CLAIMED_GPIO.compare_exchange_weak(
            claimed,
            claimed | mask,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return Ok(()),
            Err(observed) => claimed = observed,
        }
    }
}

fn claim_any(pin: u8) -> Result<(), GpioError> {
    let mask = 1_u32 << pin;
    let mut claimed = CLAIMED_GPIO.load(Ordering::Acquire);
    loop {
        if claimed & mask != 0 {
            return Err(GpioError::state_conflict());
        }
        match CLAIMED_GPIO.compare_exchange_weak(
            claimed,
            claimed | mask,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return Ok(()),
            Err(observed) => claimed = observed,
        }
    }
}

/// Claims one board-reserved GPIO pin for internal Pico 2 W wiring use.
///
/// # Errors
///
/// Returns an error when the pin is not part of the board-reserved radio wiring or is already
/// claimed.
pub(crate) fn claim_board_owned_pin(pin: u8) -> Result<GpioPinHardware, GpioError> {
    validate_board_owned_pin(pin)?;
    claim_any(pin)?;
    Ok(GpioPinHardware { pin })
}

fn release(pin: u8) {
    if GpioHardware::pins()
        .iter()
        .all(|descriptor| descriptor.pin != pin)
    {
        return;
    }
    CLAIMED_GPIO.fetch_and(!(1_u32 << pin), Ordering::AcqRel);
}

fn ensure_bank0_ready() -> Result<(), GpioError> {
    let resets_base = peripheral_base("resets").ok_or_else(GpioError::unsupported)?;
    let reset_clear = (rebase_mut(resets_base, RP2350_REG_ALIAS_CLR_OFFSET)) as *mut u32;
    let reset_done = rebase(resets_base, RP2350_RESET_DONE_OFFSET) as *const u32;

    // SAFETY: these are selected-SoC reset-controller MMIO registers surfaced by static topology.
    unsafe {
        ptr::write_volatile(reset_clear, RP2350_RESET_IO_BANK0 | RP2350_RESET_PADS_BANK0);
    }

    // SAFETY: this reads one selected-SoC reset-done register surfaced by static topology.
    while unsafe { ptr::read_volatile(reset_done) }
        & (RP2350_RESET_IO_BANK0 | RP2350_RESET_PADS_BANK0)
        != (RP2350_RESET_IO_BANK0 | RP2350_RESET_PADS_BANK0)
    {}

    Ok(())
}

fn set_function(pin: u8, function: GpioFunction) -> Result<(), GpioError> {
    validate_pin(pin)?;
    ensure_bank0_ready()?;
    let value = match function {
        GpioFunction::Sio => RP2350_SIO_FUNCSEL,
        GpioFunction::Raw(value) => u32::from(value),
    };
    let register = ctrl_register(pin)?;
    // SAFETY: this writes one selected-SoC GPIO control register.
    unsafe { ptr::write_volatile(register, value) };
    Ok(())
}

fn configure_input(pin: u8) -> Result<(), GpioError> {
    validate_pin(pin)?;
    ensure_bank0_ready()?;
    let pad = pad_register(pin)?;
    let sio_oe_clear = sio_register_mut(RP2350_SIO_GPIO_OE_CLR_OFFSET)?;
    // SAFETY: these are selected-SoC GPIO pad and SIO registers surfaced by static topology.
    unsafe {
        let pad_value = ptr::read_volatile(pad);
        ptr::write_volatile(
            pad,
            (pad_value | RP2350_PAD_IE_BIT) & !(RP2350_PAD_OD_BIT | RP2350_PAD_ISO_BIT),
        );
        ptr::write_volatile(sio_oe_clear, 1_u32 << pin);
    }
    Ok(())
}

fn read(pin: u8) -> Result<bool, GpioError> {
    validate_pin(pin)?;
    let sio_in = sio_register(RP2350_SIO_GPIO_IN_OFFSET)?;
    // SAFETY: this reads one selected-SoC SIO GPIO input register.
    Ok(unsafe { ptr::read_volatile(sio_in) } & (1_u32 << pin) != 0)
}

fn configure_output(pin: u8, initial_high: bool) -> Result<(), GpioError> {
    validate_pin(pin)?;
    ensure_bank0_ready()?;
    set_function(pin, GpioFunction::Sio)?;
    write(pin, initial_high)?;
    let sio_oe_set = sio_register_mut(RP2350_SIO_GPIO_OE_SET_OFFSET)?;
    // SAFETY: this writes one selected-SoC SIO GPIO output-enable register.
    unsafe { ptr::write_volatile(sio_oe_set, 1_u32 << pin) };
    Ok(())
}

fn write(pin: u8, high: bool) -> Result<(), GpioError> {
    validate_pin(pin)?;
    let register = if high {
        sio_register_mut(RP2350_SIO_GPIO_OUT_SET_OFFSET)?
    } else {
        sio_register_mut(RP2350_SIO_GPIO_OUT_CLR_OFFSET)?
    };
    // SAFETY: this writes one selected-SoC SIO GPIO output register alias.
    unsafe { ptr::write_volatile(register, 1_u32 << pin) };
    Ok(())
}

fn set_pull(pin: u8, pull: GpioPull) -> Result<(), GpioError> {
    validate_pin(pin)?;
    ensure_bank0_ready()?;
    let pad = pad_register(pin)?;
    // SAFETY: this reads and writes one selected-SoC pad-control register.
    unsafe {
        let mut value = ptr::read_volatile(pad);
        value &= !(RP2350_PAD_PUE_BIT | RP2350_PAD_PDE_BIT);
        value |= match pull {
            GpioPull::None => 0,
            GpioPull::Up => RP2350_PAD_PUE_BIT,
            GpioPull::Down => RP2350_PAD_PDE_BIT,
        };
        ptr::write_volatile(pad, value);
    }
    Ok(())
}

fn set_drive_strength(pin: u8, strength: GpioDriveStrength) -> Result<(), GpioError> {
    validate_pin(pin)?;
    ensure_bank0_ready()?;
    let pad = pad_register(pin)?;
    let drive_bits = match strength {
        GpioDriveStrength::MilliAmps2 => 0,
        GpioDriveStrength::MilliAmps4 => 1,
        GpioDriveStrength::MilliAmps8 => 2,
        GpioDriveStrength::MilliAmps12 => 3,
    };
    // SAFETY: this reads and writes one selected-SoC pad-control register.
    unsafe {
        let mut value = ptr::read_volatile(pad);
        value &= !(0b11 << RP2350_PAD_DRIVE_LSB);
        value |= drive_bits << RP2350_PAD_DRIVE_LSB;
        ptr::write_volatile(pad, value);
    }
    Ok(())
}

fn rp2350_gpio_topology_available() -> bool {
    peripheral_base("io_bank0").is_some()
        && peripheral_base("pads_bank0").is_some()
        && peripheral_base("sio").is_some()
        && peripheral_base("resets").is_some()
}

fn peripheral_base(name: &'static str) -> Option<usize> {
    board::peripherals()
        .iter()
        .find(|descriptor| descriptor.name == name)
        .map(|descriptor| descriptor.base)
}

fn ctrl_register(pin: u8) -> Result<*mut u32, GpioError> {
    let base = peripheral_base("io_bank0").ok_or_else(GpioError::unsupported)?;
    Ok(rebase_mut(
        base,
        usize::from(pin) * RP2350_GPIO_CTRL_STRIDE + RP2350_GPIO_CTRL_FUNCSEL_OFFSET,
    ) as *mut u32)
}

fn pad_register(pin: u8) -> Result<*mut u32, GpioError> {
    let base = peripheral_base("pads_bank0").ok_or_else(GpioError::unsupported)?;
    Ok(rebase_mut(
        base,
        RP2350_PADS_BANK0_FIRST_PAD_OFFSET + usize::from(pin) * RP2350_PAD_STRIDE,
    ) as *mut u32)
}

fn sio_register(offset: usize) -> Result<*const u32, GpioError> {
    let base = peripheral_base("sio").ok_or_else(GpioError::unsupported)?;
    Ok(rebase(base, offset) as *const u32)
}

fn sio_register_mut(offset: usize) -> Result<*mut u32, GpioError> {
    let base = peripheral_base("sio").ok_or_else(GpioError::unsupported)?;
    Ok(rebase_mut(base, offset) as *mut u32)
}

const fn rebase(base: usize, offset: usize) -> usize {
    base.wrapping_add(offset)
}

const fn rebase_mut(base: usize, offset: usize) -> usize {
    base.wrapping_add(offset)
}
